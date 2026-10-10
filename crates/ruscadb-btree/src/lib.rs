//! # ruscadb-btree
//!
//! **B+tree ordenado genérico** de RuscaDB: estructura de índice con nodos
//! internos (separadores) y hojas enlazadas, con `insert`/`get`/`remove`/`range`
//! y rebalanceo por *split* aguas arriba y *borrow*/*merge* al eliminar.
//!
//! Especificación: `specs/btree.md` (SPEC-0020).
//! Diseño: `docs/RuscaDB-roadmap.md` §5.4 (índice relacional B+tree) y ADR-003
//! (metadatos/grafo/FTS en estructuras ordenadas nativas).
//!
//! ## Invariantes estructurales
//!
//! 1. Todas las hojas están a la **misma profundidad** (árbol balanceado).
//! 2. Las hojas están **ordenadas** entre sí y **enlazadas** (`next`), de modo
//!    que `range` recorre el rango haciendo *scan* secuencial de hojas.
//! 3. Cada nodo salvo la raíz tiene entre `ceil(m/2) - 1` y `m - 1` claves,
//!    donde `m` es el [`order`](BPlusTree::order) (máximo de hijos).
//! 4. Un nodo interno con `k` claves tiene exactamente `k + 1` hijos; la clave
//!    `keys[j]` coincide con la clave mínima del subárbol `children[j + 1]`.
//!
//! La altura es `O(log n)`; `insert`/`get`/`remove` son `O(log n)` en el peor
//! caso y `range` es `O(log n + k)` para `k` elementos devueltos.

#![forbid(unsafe_code)]

use ruscadb_core::RuscaError;

/// Orden mínimo admitido (máximo de hijos por nodo) para un B+tree útil.
pub const MIN_ORDER: usize = 3;

/// B+tree ordenado genérico con **arena** de nodos referenciados por índice.
///
/// # Type Parameters
/// - `K`: tipo de clave, `Ord + Clone`.
/// - `V`: tipo de valor, `Clone`.
pub struct BPlusTree<K: Ord + Clone, V: Clone> {
    /// Máximo de hijos por nodo (orden del árbol).
    order: usize,
    /// Arena de nodos referenciados por índice (evita `Rc`/`RefCell` y `unsafe`).
    nodes: Vec<Node<K, V>>,
    /// Índice de la raíz en la arena, o `None` si el árbol está vacío.
    root: Option<usize>,
    /// Número de pares clave-valor almacenados.
    len: usize,
}

/// Nodo del árbol: interno (separadores) o hoja (pares + enlace siguiente).
enum Node<K, V> {
    /// Nodo interno: separadores y referencias a los hijos.
    Internal {
        /// Separadores ordenados; `keys[j]` es la clave mínima de `children[j + 1]`.
        keys: Vec<K>,
        /// Índices de los hijos en la arena (`children.len() == keys.len() + 1`).
        children: Vec<usize>,
    },
    /// Hoja: pares ordenados y enlace a la siguiente hoja.
    Leaf {
        /// Pares clave-valor ordenados de forma estrictamente creciente.
        values: Vec<(K, V)>,
        /// Índice de la siguiente hoja (para el *scan* secuencial de `range`).
        next: Option<usize>,
    },
}

/// Resultado interno de una inserción recursiva.
struct Insertion<K, V> {
    /// Valor reemplazado, si la clave ya existía en el árbol.
    previous: Option<V>,
    /// Separador y nodo derecho nuevos cuando el nodo se partió (*split*).
    split: Option<(K, usize)>,
}

impl<K: Ord + Clone, V: Clone> BPlusTree<K, V> {
    /// Crea un B+tree vacío.
    ///
    /// # Args
    /// - `order`: máximo de hijos por nodo; debe ser `>= MIN_ORDER` (`3`).
    ///
    /// # Returns
    /// Un árbol vacío listo para operar.
    ///
    /// # Raises
    /// - [`RuscaError::InvalidConfig`]: si `order < MIN_ORDER`.
    pub fn new(order: usize) -> Result<Self, RuscaError> {
        if order < MIN_ORDER {
            return Err(RuscaError::InvalidConfig(format!(
                "order={order} inválido: el B+tree requiere order >= {MIN_ORDER} (máximo de hijos por nodo)"
            )));
        }
        Ok(Self {
            order,
            nodes: Vec::new(),
            root: None,
            len: 0,
        })
    }

    /// Devuelve el orden configurado (máximo de hijos por nodo).
    ///
    /// # Returns
    /// El valor `m` tal que cada nodo salvo la raíz tiene entre `ceil(m/2) - 1`
    /// y `m - 1` claves.
    pub fn order(&self) -> usize {
        self.order
    }

    /// Número de pares clave-valor almacenados.
    ///
    /// # Returns
    /// La cardinalidad actual del árbol.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Indica si el árbol no contiene ningún par.
    ///
    /// # Returns
    /// `true` si `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Altura del árbol (número de niveles).
    ///
    /// # Returns
    /// `0` si el árbol está vacío; en caso contrario el número de niveles desde
    /// la raíz hasta las hojas (que están todas a la misma profundidad).
    pub fn height(&self) -> usize {
        let Some(mut idx) = self.root else {
            return 0;
        };
        let mut height = 1;
        while let Node::Internal { children, .. } = &self.nodes[idx] {
            idx = children[0];
            height += 1;
        }
        height
    }

    /// Inserta o sobrescribe el valor asociado a `key`.
    ///
    /// # Args
    /// - `key`: clave a insertar.
    /// - `value`: valor asociado.
    ///
    /// # Returns
    /// El valor previo si `key` ya existía; `None` si es nueva.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let root = match self.root {
            Some(idx) => idx,
            None => {
                let leaf = self.alloc(Node::Leaf {
                    values: Vec::new(),
                    next: None,
                });
                self.root = Some(leaf);
                leaf
            }
        };
        let Insertion { previous, split } = self.insert_rec(root, key, value);
        if let Some((separator, right)) = split {
            let new_root = self.alloc(Node::Internal {
                keys: vec![separator],
                children: vec![root, right],
            });
            self.root = Some(new_root);
        }
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    /// Obtiene el valor asociado a `key`.
    ///
    /// # Args
    /// - `key`: clave a consultar.
    ///
    /// # Returns
    /// Una referencia al valor, o `None` si la clave no existe.
    pub fn get(&self, key: &K) -> Option<&V> {
        let leaf = self.find_leaf(key)?;
        let values = match &self.nodes[leaf] {
            Node::Leaf { values, .. } => values,
            Node::Internal { .. } => return None,
        };
        values
            .binary_search_by(|(candidate, _)| candidate.cmp(key))
            .ok()
            .map(|position| &values[position].1)
    }

    /// Indica si `key` existe en el árbol.
    ///
    /// # Args
    /// - `key`: clave a comprobar.
    ///
    /// # Returns
    /// `true` si la clave está presente.
    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Elimina `key` y devuelve su valor.
    ///
    /// Si las hojas o nodos internos quedan subocupados, se rebalancean por
    /// *borrow* (préstamo de un hermano con holgura) o *merge* (fusión con un
    /// hermano). La raíz se colapsa si queda sin claves.
    ///
    /// # Args
    /// - `key`: clave a eliminar.
    ///
    /// # Returns
    /// El valor eliminado, o `None` si la clave no existía.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let root = self.root?;
        let removed = self.remove_rec(root, key)?;
        self.len -= 1;
        self.collapse_root();
        Some(removed)
    }

    /// Devuelve los pares con clave en el rango semiabierto `[start, end)`.
    ///
    /// # Args
    /// - `start`: cota inferior inclusiva.
    /// - `end`: cota superior exclusiva.
    ///
    /// # Returns
    /// Pares ordenados ascendentemente por clave; vacío si `start >= end`.
    pub fn range(&self, start: &K, end: &K) -> Vec<(K, V)> {
        let mut result = Vec::new();
        if start >= end {
            return result;
        }
        let Some(mut leaf) = self.find_leaf(start) else {
            return result;
        };
        loop {
            let next = match &self.nodes[leaf] {
                Node::Leaf { values, next } => {
                    let mut reached_end = false;
                    for (key, value) in values {
                        if key < start {
                            continue;
                        }
                        if key >= end {
                            reached_end = true;
                            break;
                        }
                        result.push((key.clone(), value.clone()));
                    }
                    if reached_end { None } else { *next }
                }
                Node::Internal { .. } => None,
            };
            let Some(next_leaf) = next else {
                return result;
            };
            leaf = next_leaf;
        }
    }

    /// Número mínimo de claves de un nodo no raíz: `ceil(order / 2) - 1`.
    fn min_keys(&self) -> usize {
        self.order.div_ceil(2) - 1
    }

    /// Reserva un nodo en la arena y devuelve su índice.
    fn alloc(&mut self, node: Node<K, V>) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Comprueba si el nodo en `idx` es una hoja.
    fn is_leaf(&self, idx: usize) -> bool {
        matches!(&self.nodes[idx], Node::Leaf { .. })
    }

    /// Número de claves (o pares, en hojas) del nodo `idx`.
    fn node_len(&self, idx: usize) -> usize {
        match &self.nodes[idx] {
            Node::Leaf { values, .. } => values.len(),
            Node::Internal { keys, .. } => keys.len(),
        }
    }

    /// Índice del hijo en `position` de un nodo interno `parent`.
    fn child_at(&self, parent: usize, position: usize) -> usize {
        match &self.nodes[parent] {
            Node::Internal { children, .. } => children[position],
            Node::Leaf { .. } => parent,
        }
    }

    /// Clave mínima almacenada en el subárbol del nodo `idx`.
    ///
    /// En un nodo interno la clave mínima **no** está en `keys[0]` (que es el
    /// separador del segundo hijo), sino en la hoja más a la izquierda del
    /// subárbol; por eso se desciende por `children[0]`.
    fn node_min(&self, idx: usize) -> K {
        let mut node = idx;
        while let Node::Internal { children, .. } = &self.nodes[node] {
            node = children[0];
        }
        match &self.nodes[node] {
            Node::Leaf { values, .. } => values[0].0.clone(),
            Node::Internal { keys, .. } => keys[0].clone(),
        }
    }

    /// Desciende desde la raíz hasta la hoja que contiene (o contendría) `key`.
    fn find_leaf(&self, key: &K) -> Option<usize> {
        let mut idx = self.root?;
        loop {
            match &self.nodes[idx] {
                Node::Leaf { .. } => return Some(idx),
                Node::Internal { keys, children } => {
                    let child = keys.partition_point(|candidate| candidate <= key);
                    idx = children[child];
                }
            }
        }
    }

    /// Inserción recursiva: devuelve el valor previo y, si hubo *split*, el
    /// separador y el nuevo nodo derecho que el padre debe incorporar.
    fn insert_rec(&mut self, idx: usize, key: K, value: V) -> Insertion<K, V> {
        if self.is_leaf(idx) {
            self.insert_into_leaf(idx, key, value)
        } else {
            self.insert_into_internal(idx, key, value)
        }
    }

    /// Inserta en una hoja y la parte si supera `order - 1` pares.
    fn insert_into_leaf(&mut self, idx: usize, key: K, value: V) -> Insertion<K, V> {
        let order = self.order;
        let split_values = {
            let Node::Leaf { values, .. } = &mut self.nodes[idx] else {
                return Insertion {
                    previous: None,
                    split: None,
                };
            };
            match values.binary_search_by(|(candidate, _)| candidate.cmp(&key)) {
                Ok(position) => {
                    let previous = std::mem::replace(&mut values[position].1, value);
                    return Insertion {
                        previous: Some(previous),
                        split: None,
                    };
                }
                Err(position) => values.insert(position, (key, value)),
            }
            if values.len() < order {
                return Insertion {
                    previous: None,
                    split: None,
                };
            }
            let mid = values.len() / 2;
            values.split_off(mid)
        };
        let Some((separator, _)) = split_values.first() else {
            return Insertion {
                previous: None,
                split: None,
            };
        };
        let separator = separator.clone();
        let old_next = match &self.nodes[idx] {
            Node::Leaf { next, .. } => *next,
            Node::Internal { .. } => None,
        };
        let right = self.alloc(Node::Leaf {
            values: split_values,
            next: old_next,
        });
        if let Node::Leaf { next, .. } = &mut self.nodes[idx] {
            *next = Some(right);
        }
        Insertion {
            previous: None,
            split: Some((separator, right)),
        }
    }

    /// Inserta descendiendo por un nodo interno y propaga el *split* del hijo.
    fn insert_into_internal(&mut self, idx: usize, key: K, value: V) -> Insertion<K, V> {
        let (child_position, child) = match &self.nodes[idx] {
            Node::Internal { keys, children } => {
                let position = keys.partition_point(|candidate| candidate <= &key);
                (position, children[position])
            }
            Node::Leaf { .. } => {
                return Insertion {
                    previous: None,
                    split: None,
                };
            }
        };
        let Insertion { previous, split } = self.insert_rec(child, key, value);
        let Some((separator, right)) = split else {
            return Insertion {
                previous,
                split: None,
            };
        };
        if let Node::Internal { keys, children } = &mut self.nodes[idx] {
            keys.insert(child_position, separator);
            children.insert(child_position + 1, right);
        }
        let overflow = match &self.nodes[idx] {
            Node::Internal { children, .. } => children.len() > self.order,
            Node::Leaf { .. } => false,
        };
        if !overflow {
            return Insertion {
                previous,
                split: None,
            };
        }
        let split_result = match &mut self.nodes[idx] {
            Node::Internal { keys, children } => {
                let mid = children.len() / 2;
                let tail_keys = keys.split_off(mid);
                let promoted = keys.pop();
                let right_children = children.split_off(mid);
                promoted.map(|promoted| (promoted, tail_keys, right_children))
            }
            Node::Leaf { .. } => None,
        };
        match split_result {
            Some((promoted, tail_keys, right_children)) => {
                let new_right = self.alloc(Node::Internal {
                    keys: tail_keys,
                    children: right_children,
                });
                Insertion {
                    previous,
                    split: Some((promoted, new_right)),
                }
            }
            None => Insertion {
                previous,
                split: None,
            },
        }
    }

    /// Eliminación recursiva con rebalanceo del hijo subocupado.
    fn remove_rec(&mut self, idx: usize, key: &K) -> Option<V> {
        if self.is_leaf(idx) {
            return self.remove_from_leaf(idx, key);
        }
        let (position, child) = match &self.nodes[idx] {
            Node::Internal { keys, children } => {
                let position = keys.partition_point(|candidate| candidate <= key);
                (position, children[position])
            }
            Node::Leaf { .. } => return None,
        };
        let removed = self.remove_rec(child, key)?;
        let primary = if self.node_len(child) < self.min_keys() {
            self.rebalance(idx, position)
        } else {
            position
        };
        self.refresh_separator(idx, primary);
        Some(removed)
    }

    /// Elimina el par de una hoja y devuelve su valor.
    fn remove_from_leaf(&mut self, idx: usize, key: &K) -> Option<V> {
        let Node::Leaf { values, .. } = &mut self.nodes[idx] else {
            return None;
        };
        match values.binary_search_by(|(candidate, _)| candidate.cmp(key)) {
            Ok(position) => Some(values.remove(position).1),
            Err(_) => None,
        }
    }

    /// Actualiza el separador del hijo en `position` para que refleje su nueva
    /// clave mínima (la raíz y la posición 0 no tienen separador a la izquierda).
    fn refresh_separator(&mut self, parent: usize, position: usize) {
        if position == 0 {
            return;
        }
        let child = self.child_at(parent, position);
        let new_separator = self.node_min(child);
        if let Node::Internal { keys, .. } = &mut self.nodes[parent] {
            keys[position - 1] = new_separator;
        }
    }

    /// Rebalancea el hijo `position` de `parent` tras un borrado. Devuelve la
    /// posición del nodo que conserva los datos (puede cambiar si hubo *merge*).
    fn rebalance(&mut self, parent: usize, position: usize) -> usize {
        let left = position.checked_sub(1);
        let right = {
            let count = match &self.nodes[parent] {
                Node::Internal { children, .. } => children.len(),
                Node::Leaf { .. } => 0,
            };
            (position + 1 < count).then_some(position + 1)
        };
        if let Some(left_position) = left {
            let left_idx = self.child_at(parent, left_position);
            if self.node_len(left_idx) > self.min_keys() {
                self.borrow_from_left(parent, position);
                return position;
            }
        }
        if let Some(right_position) = right {
            let right_idx = self.child_at(parent, right_position);
            if self.node_len(right_idx) > self.min_keys() {
                self.borrow_from_right(parent, position);
                return position;
            }
        }
        if left.is_some() {
            self.merge_into_left(parent, position);
            position - 1
        } else {
            self.merge_into_left(parent, position + 1);
            position
        }
    }

    /// Préstamo desde el hermano izquierdo hacia el hijo `position`.
    fn borrow_from_left(&mut self, parent: usize, position: usize) {
        let left = self.child_at(parent, position - 1);
        let child = self.child_at(parent, position);
        if self.is_leaf(left) {
            let entry = match &mut self.nodes[left] {
                Node::Leaf { values, .. } => values.pop(),
                Node::Internal { .. } => None,
            };
            if let Some(pair) = entry {
                if let Node::Leaf { values, .. } = &mut self.nodes[child] {
                    values.insert(0, pair);
                }
            }
        } else {
            let down_key = self.node_min(child);
            let (promoted, moved_child) = match &mut self.nodes[left] {
                Node::Internal { keys, children } => (keys.pop(), children.pop()),
                Node::Leaf { .. } => (None, None),
            };
            if let (Some(promoted), Some(moved_child)) = (promoted, moved_child) {
                if let Node::Internal { keys, children } = &mut self.nodes[child] {
                    keys.insert(0, down_key);
                    children.insert(0, moved_child);
                }
                if let Node::Internal { keys, .. } = &mut self.nodes[parent] {
                    keys[position - 1] = promoted;
                }
            }
        }
        self.refresh_separator(parent, position);
    }

    /// Préstamo desde el hermano derecho hacia el hijo `position`.
    fn borrow_from_right(&mut self, parent: usize, position: usize) {
        let child = self.child_at(parent, position);
        let right = self.child_at(parent, position + 1);
        if self.is_leaf(right) {
            // El hermano derecho tiene `> min_keys >= 1` claves por el invariante
            // de `rebalance`, así que la hoja nunca está vacía aquí.
            let entry = match &mut self.nodes[right] {
                Node::Leaf { values, .. } => Some(values.remove(0)),
                Node::Internal { .. } => None,
            };
            if let Some(pair) = entry {
                if let Node::Leaf { values, .. } = &mut self.nodes[child] {
                    values.push(pair);
                }
            }
            let new_separator = match &self.nodes[right] {
                Node::Leaf { values, .. } => values.first().map(|(key, _)| key.clone()),
                Node::Internal { .. } => None,
            };
            if let (Some(separator), Node::Internal { keys, .. }) =
                (new_separator, &mut self.nodes[parent])
            {
                keys[position] = separator;
            }
        } else {
            let down_key = self.node_min(right);
            let (promoted, moved_child) = match &mut self.nodes[right] {
                Node::Internal { keys, children } => {
                    let key = (!keys.is_empty()).then(|| keys.remove(0));
                    let child = (!children.is_empty()).then(|| children.remove(0));
                    (key, child)
                }
                Node::Leaf { .. } => (None, None),
            };
            if let (Some(promoted), Some(moved_child)) = (promoted, moved_child) {
                if let Node::Internal { keys, children } = &mut self.nodes[child] {
                    keys.push(down_key);
                    children.push(moved_child);
                }
                if let Node::Internal { keys, .. } = &mut self.nodes[parent] {
                    keys[position] = promoted;
                }
            }
        }
        self.refresh_separator(parent, position);
    }

    /// Fusiona el nodo `target` con su hermano izquierdo `target - 1`.
    fn merge_into_left(&mut self, parent: usize, target: usize) {
        let left = self.child_at(parent, target - 1);
        let right = self.child_at(parent, target);
        if self.is_leaf(left) {
            let (right_values, right_next) = match &mut self.nodes[right] {
                Node::Leaf { values, next } => (std::mem::take(values), *next),
                Node::Internal { .. } => (Vec::new(), None),
            };
            if let Node::Leaf { values, next } = &mut self.nodes[left] {
                values.extend(right_values);
                *next = right_next;
            }
        } else {
            let separator = self.node_min(right);
            let (right_keys, right_children) = match &mut self.nodes[right] {
                Node::Internal { keys, children } => {
                    (std::mem::take(keys), std::mem::take(children))
                }
                Node::Leaf { .. } => (Vec::new(), Vec::new()),
            };
            if let Node::Internal { keys, children } = &mut self.nodes[left] {
                keys.push(separator);
                keys.extend(right_keys);
                children.extend(right_children);
            }
        }
        if let Node::Internal { keys, children } = &mut self.nodes[parent] {
            keys.remove(target - 1);
            children.remove(target);
        }
    }

    /// Colapsa la raíz cuando queda sin claves: la sustituye por su único hijo
    /// (o vacía el árbol si la raíz es una hoja sin pares).
    fn collapse_root(&mut self) {
        while let Some(root) = self.root {
            let child = match &self.nodes[root] {
                Node::Internal { keys, children } if keys.is_empty() => Some(children[0]),
                _ => None,
            };
            match child {
                Some(next) => self.root = Some(next),
                None => break,
            }
        }
        if let Some(root) = self.root {
            let empty_leaf =
                matches!(&self.nodes[root], Node::Leaf { values, .. } if values.is_empty());
            if empty_leaf {
                self.root = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    /// Operación del proptest metamórfico (oráculo `BTreeMap`).
    #[derive(Debug, Clone)]
    enum Op {
        /// Inserta/sobrescribe la clave con valor `clave * 2`.
        Insert(i64),
        /// Elimina la clave.
        Remove(i64),
        /// Consulta la clave.
        Get(i64),
        /// Consulta el rango semiabierto `[a, b)`.
        Range(i64, i64),
    }

    /// Estrategia de operaciones sobre un espacio pequeño de claves.
    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0i64..48).prop_map(Op::Insert),
            (0i64..48).prop_map(Op::Remove),
            (0i64..48).prop_map(Op::Get),
            (0i64..48, 0i64..48).prop_map(|(start, end)| Op::Range(start, end)),
        ]
    }

    /// AC-0020-01 — insert/get devuelven el valor correcto y sobrescriben.
    #[test]
    // @spec AC-0020-01
    fn test_ac_0020_01_insert_get_overwrite() {
        let mut tree = BPlusTree::new(3).expect("orden válido");
        assert!(tree.is_empty());
        assert_eq!(tree.insert(1, "one"), None);
        assert_eq!(tree.insert(2, "two"), None);
        assert_eq!(tree.get(&1), Some(&"one"));
        assert_eq!(tree.get(&2), Some(&"two"));
        assert_eq!(tree.insert(1, "ONE"), Some("one"));
        assert_eq!(tree.get(&1), Some(&"ONE"));
        assert_eq!(tree.len(), 2);
        assert!(tree.contains_key(&2));
        assert!(!tree.contains_key(&99));
        tree.assert_invariants();
    }

    /// AC-0020-02 — remove elimina y las claves ausentes no alteran el árbol.
    #[test]
    // @spec AC-0020-02
    fn test_ac_0020_02_remove_and_missing() {
        let mut tree = BPlusTree::new(3).expect("orden válido");
        for key in 0..20 {
            tree.insert(key, key * 10);
        }
        assert_eq!(tree.remove(&5), Some(50));
        assert_eq!(tree.remove(&5), None);
        assert_eq!(tree.remove(&100), None);
        assert_eq!(tree.len(), 19);
        for key in 0..20 {
            if key != 5 {
                assert_eq!(tree.get(&key), Some(&(key * 10)));
            }
        }
        tree.assert_invariants();
    }

    /// AC-0020-03 — range devuelve pares ordenados y acotados `[start, end)`.
    #[test]
    // @spec AC-0020-03
    fn test_ac_0020_03_range_is_sorted_and_bounded() {
        let mut tree = BPlusTree::new(4).expect("orden válido");
        for key in [10, 3, 7, 1, 9, 5, 2, 8, 4, 6, 0] {
            tree.insert(key, key);
        }
        assert_eq!(tree.range(&3, &7), vec![(3, 3), (4, 4), (5, 5), (6, 6)]);
        assert_eq!(tree.range(&3, &4), vec![(3, 3)]);
        assert!(tree.range(&7, &3).is_empty());
        assert!(tree.range(&100, &200).is_empty());
        assert_eq!(tree.range(&0, &100).len(), 11);
        tree.assert_invariants();
    }

    /// AC-0020-04 — muchas claves sobreviven a splits en varios órdenes.
    #[test]
    // @spec AC-0020-04
    fn test_ac_0020_04_many_keys_survive_splits() {
        for order in [3usize, 4, 5, 8] {
            let mut tree = BPlusTree::new(order).expect("orden válido");
            for i in 0..500 {
                let key = (i * 37) % 500;
                tree.insert(key, key);
            }
            assert_eq!(tree.len(), 500);
            for key in 0..500 {
                assert_eq!(tree.get(&key), Some(&key));
            }
            tree.assert_invariants();
        }
    }

    /// AC-0020-05 — vaciar con rebalanceo y seguir operativo.
    #[test]
    // @spec AC-0020-05
    fn test_ac_0020_05_delete_to_empty_rebalances() {
        for order in [3usize, 4, 5] {
            let mut tree = BPlusTree::new(order).expect("orden válido");
            for i in 0..200 {
                tree.insert(i, i);
            }
            for i in 0..200 {
                let key = (i * 13) % 200;
                assert_eq!(tree.remove(&key), Some(key));
                tree.assert_invariants();
            }
            assert_eq!(tree.len(), 0);
            assert!(tree.is_empty());
            tree.insert(42, 42);
            assert_eq!(tree.get(&42), Some(&42));
            assert_eq!(tree.remove(&42), Some(42));
            assert!(tree.is_empty());
            tree.assert_invariants();
        }
    }

    /// BVA — `order` mínimo `3`; por debajo del mínimo se rechaza.
    #[test]
    fn test_bva_minimum_order_is_three() {
        assert_eq!(BPlusTree::<i32, i32>::new(0).is_err(), true);
        assert_eq!(BPlusTree::<i32, i32>::new(1).is_err(), true);
        assert_eq!(BPlusTree::<i32, i32>::new(2).is_err(), true);
        let tree = BPlusTree::<i32, i32>::new(3).expect("orden mínimo válido");
        assert_eq!(tree.order(), 3);
        assert!(tree.is_empty());
        assert_eq!(tree.height(), 0);
    }

    /// BVA — insertar claves duplicadas no incrementa `len`.
    #[test]
    fn test_bva_duplicate_keys_do_not_grow() {
        let mut tree = BPlusTree::new(3).expect("orden válido");
        for _ in 0..10 {
            tree.insert(7, 7);
        }
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.insert(7, 8), Some(7));
        assert_eq!(tree.get(&7), Some(&8));
        assert_eq!(tree.len(), 1);
        tree.assert_invariants();
    }

    /// BVA — inserciones descendentes y vaciado total dejan el árbol operativo.
    #[test]
    fn test_bva_descending_insertions_then_clear() {
        let mut tree = BPlusTree::new(3).expect("orden válido");
        for key in (0..300).rev() {
            tree.insert(key, key);
        }
        assert_eq!(tree.height() > 1, true);
        tree.assert_invariants();
        for key in 0..300 {
            assert_eq!(tree.get(&key), Some(&key));
        }
        for key in 0..300 {
            assert_eq!(tree.remove(&key), Some(key));
        }
        assert!(tree.is_empty());
        tree.assert_invariants();
    }

    /// Estructural — los invariantes se sostienen tras operaciones mixtas.
    #[test]
    fn test_structural_invariants_after_mixed_operations() {
        for order in [3usize, 4, 6, 9] {
            let mut tree = BPlusTree::new(order).expect("orden válido");
            for i in 0..400 {
                tree.insert((i * 17) % 400, i);
            }
            for i in 0..400 {
                tree.remove(&((i * 23) % 400));
            }
            for i in 0..100 {
                tree.insert(i, i);
            }
            tree.assert_invariants();
            let mut expected: Vec<(i32, i32)> = (0..100).map(|k| (k, k)).collect();
            while let Some((key, value)) = expected.pop() {
                assert_eq!(tree.get(&key), Some(&value));
            }
        }
    }

    proptest! {
        /// Metamórfico — el B+tree equivale a `BTreeMap` para toda la secuencia
        /// de insert/remove/get/range, y mantiene los invariantes estructurales.
        #[test]
        fn prop_btree_matches_btreemap_oracle(
            ops in prop::collection::vec(op_strategy(), 0..250),
            order in 3usize..7,
        ) {
            let mut tree = BPlusTree::new(order).expect("orden válido");
            let mut model: BTreeMap<i64, i64> = BTreeMap::new();
            for op in ops {
                match op {
                    Op::Insert(key) => {
                        prop_assert_eq!(tree.insert(key, key * 2), model.insert(key, key * 2));
                    }
                    Op::Remove(key) => {
                        prop_assert_eq!(tree.remove(&key), model.remove(&key));
                    }
                    Op::Get(key) => {
                        prop_assert_eq!(tree.get(&key), model.get(&key));
                    }
                    Op::Range(start, end) => {
                        let expected: Vec<(i64, i64)> = if start >= end {
                            Vec::new()
                        } else {
                            model.range(start..end).map(|(k, v)| (*k, *v)).collect()
                        };
                        prop_assert_eq!(tree.range(&start, &end), expected);
                    }
                }
                prop_assert_eq!(tree.len(), model.len());
                prop_assert_eq!(tree.is_empty(), model.is_empty());
            }
            tree.assert_invariants();
        }
    }

    #[cfg(test)]
    impl<K: Ord + Clone + std::fmt::Debug, V: Clone> BPlusTree<K, V> {
        /// Verifica los invariantes estructurales (solo disponible en tests).
        fn assert_invariants(&self) {
            let Some(root) = self.root else {
                assert_eq!(self.len, 0, "árbol sin raíz pero len != 0");
                return;
            };
            let depth = self.check_node_invariants(root, true);
            assert!(depth >= 1, "profundidad inválida");
            let mut leaf = self.leftmost_leaf();
            let mut previous: Option<K> = None;
            let mut count = 0usize;
            loop {
                let Node::Leaf { values, next } = &self.nodes[leaf] else {
                    panic!("la cadena de hojas alcanzó un nodo interno");
                };
                for (key, _) in values {
                    if let Some(prev) = &previous {
                        assert!(prev < key, "claves de hojas desordenadas");
                    }
                    previous = Some(key.clone());
                    count += 1;
                }
                let Some(next_leaf) = *next else { break };
                leaf = next_leaf;
            }
            assert_eq!(count, self.len, "len no coincide con las hojas");
        }

        /// Verifica recursivamente ocupación, orden y uniformidad de altura.
        fn check_node_invariants(&self, idx: usize, is_root: bool) -> usize {
            // La ocupación mínima se recalcula aquí (no vía `min_keys`) para que
            // el test sea un oráculo independiente del código de producción.
            let expected_min = self.order.div_ceil(2) - 1;
            match &self.nodes[idx] {
                Node::Leaf { values, .. } => {
                    if !is_root {
                        assert!(
                            values.len() >= expected_min,
                            "hoja subocupada: {} < {}",
                            values.len(),
                            expected_min
                        );
                    }
                    assert!(values.len() < self.order, "hoja sobreocupada");
                    for window in values.windows(2) {
                        assert!(window[0].0 < window[1].0, "claves de hoja no crecientes");
                    }
                    1
                }
                Node::Internal { keys, children } => {
                    if !is_root {
                        assert!(keys.len() >= expected_min, "nodo interno subocupado");
                    }
                    assert!(keys.len() < self.order, "nodo interno sobreocupado");
                    assert_eq!(children.len(), keys.len() + 1, "hijos != claves + 1");
                    for window in keys.windows(2) {
                        assert!(window[0] < window[1], "separadores no crecientes");
                    }
                    let child_depth = self.check_node_invariants(children[0], false);
                    for (position, child) in children.iter().enumerate() {
                        let depth = self.check_node_invariants(*child, false);
                        assert_eq!(depth, child_depth, "subárboles de distinta altura");
                        if position > 0 {
                            assert_eq!(
                                self.node_min(*child),
                                keys[position - 1],
                                "separador != min del subárbol derecho"
                            );
                        }
                    }
                    child_depth + 1
                }
            }
        }

        /// Índice de la hoja más a la izquierda del árbol.
        fn leftmost_leaf(&self) -> usize {
            let mut node = self.root.expect("raíz presente");
            while let Node::Internal { children, .. } = &self.nodes[node] {
                node = children[0];
            }
            node
        }
    }
}
