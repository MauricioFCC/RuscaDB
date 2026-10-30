//! Copy-on-write B-tree (SPEC-0130): arbol persistente con path copying.
//!
//! Implementa un **treap persistente**: cada nodo es inmutable (`Arc`) y una
//! escritura copia solo el camino raiz->hoja (`O(log n)` esperado), compartiendo
//! el resto del arbol con los snapshots. `fork()` es `O(1)` (clona el `Arc` de
//! la raiz), lo que da aislamiento de snapshots real sin bloquear lectores.
//!
//! Prioridad determinista `splitmix64(key)` (sin aleatoriedad en produccion).
//! Complejidad: `get`/`insert` `O(log n)` esperado; `fork` `O(1)`;
//! `materialize` `O(n)` solo para inspeccion/depuracion.
//!
//! Referencias: Okasaki, "Purely Functional Data Structures" (1998); Seidel &
//! Aragon, "Randomized Search Trees" (1989).

use std::sync::Arc;

/// Nodo inmutable del treap.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    key: i64,
    value: String,
    priority: u64,
    left: Option<Arc<Node>>,
    right: Option<Arc<Node>>,
}

/// Vista materializada del arbol (snapshot inspeccionable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CowBTreeNode {
    /// Claves en orden ascendente.
    pub keys: Vec<i64>,
    /// Valores alineados con `keys`.
    pub values: Vec<String>,
    /// Epoch de la vista.
    pub epoch: u64,
}

/// Arbol copy-on-write persistente.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CowBTree {
    root: Option<Arc<Node>>,
    epoch: u64,
    len: usize,
}

/// `splitmix64`: prioridad determinista y bien distribuida.
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Rotacion a la derecha (path copying).
fn rotate_right(node: Arc<Node>) -> Arc<Node> {
    let Some(left) = node.left.clone() else {
        return node;
    };
    let new_right = Arc::new(Node {
        key: node.key,
        value: node.value.clone(),
        priority: node.priority,
        left: left.right.clone(),
        right: node.right.clone(),
    });
    Arc::new(Node {
        key: left.key,
        value: left.value.clone(),
        priority: left.priority,
        left: left.left.clone(),
        right: Some(new_right),
    })
}

/// Rotacion a la izquierda (path copying).
fn rotate_left(node: Arc<Node>) -> Arc<Node> {
    let Some(right) = node.right.clone() else {
        return node;
    };
    let new_left = Arc::new(Node {
        key: node.key,
        value: node.value.clone(),
        priority: node.priority,
        left: node.left.clone(),
        right: right.left.clone(),
    });
    Arc::new(Node {
        key: right.key,
        value: right.value.clone(),
        priority: right.priority,
        left: Some(new_left),
        right: right.right.clone(),
    })
}

/// Inserta (o reemplaza) preservando la estructura por prioridad.
fn insert_node(node: Option<Arc<Node>>, key: i64, value: String) -> Arc<Node> {
    match node {
        None => Arc::new(Node {
            key,
            value,
            priority: splitmix64(key as u64),
            left: None,
            right: None,
        }),
        Some(current) => {
            if key == current.key {
                return Arc::new(Node {
                    key,
                    value,
                    priority: current.priority,
                    left: current.left.clone(),
                    right: current.right.clone(),
                });
            }
            if key < current.key {
                let new_left = insert_node(current.left.clone(), key, value);
                let candidate = Arc::new(Node {
                    key: current.key,
                    value: current.value.clone(),
                    priority: current.priority,
                    left: Some(new_left),
                    right: current.right.clone(),
                });
                if candidate
                    .left
                    .as_ref()
                    .is_some_and(|left| left.priority > candidate.priority)
                {
                    rotate_right(candidate)
                } else {
                    candidate
                }
            } else {
                let new_right = insert_node(current.right.clone(), key, value);
                let candidate = Arc::new(Node {
                    key: current.key,
                    value: current.value.clone(),
                    priority: current.priority,
                    left: current.left.clone(),
                    right: Some(new_right),
                });
                if candidate
                    .right
                    .as_ref()
                    .is_some_and(|right| right.priority > candidate.priority)
                {
                    rotate_left(candidate)
                } else {
                    candidate
                }
            }
        }
    }
}

impl CowBTree {
    /// Crea el arbol vacio.
    ///
    /// Returns:
    ///     Arbol sin claves.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserta o reemplaza una clave.
    ///
    /// Args:
    ///     key: Clave.
    ///     value: Valor.
    pub fn insert(&mut self, key: i64, value: String) {
        let existed = self.get(key).is_some();
        self.root = Some(insert_node(self.root.clone(), key, value));
        if !existed {
            self.len += 1;
        }
        self.epoch += 1;
    }

    /// Busca una clave.
    ///
    /// Args:
    ///     key: Clave.
    ///
    /// Returns:
    ///     Valor asociado, si existe.
    pub fn get(&self, key: i64) -> Option<String> {
        let mut cursor = self.root.as_ref();
        while let Some(node) = cursor {
            if key == node.key {
                return Some(node.value.clone());
            }
            cursor = if key < node.key {
                node.left.as_ref()
            } else {
                node.right.as_ref()
            };
        }
        None
    }

    /// Crea un snapshot en `O(1)` compartiendo el arbol (`Arc`).
    ///
    /// Returns:
    ///     Arbol con la misma raiz; las escrituras posteriores no lo afectan.
    pub fn fork(&self) -> Self {
        Self {
            root: self.root.clone(),
            epoch: self.epoch,
            len: self.len,
        }
    }

    /// Materializa una vista inspeccionable (claves ordenadas).
    ///
    /// Returns:
    ///     `Some(view)` o `None` si el arbol esta vacio.
    pub fn snapshot(&self) -> Option<CowBTreeNode> {
        let root = self.root.as_ref()?;
        let mut pairs = Vec::with_capacity(self.len);
        collect(root, &mut pairs);
        Some(CowBTreeNode {
            keys: pairs.iter().map(|(key, _)| *key).collect(),
            values: pairs.into_iter().map(|(_, value)| value).collect(),
            epoch: self.epoch,
        })
    }

    /// Epoch actual (numero de escrituras).
    ///
    /// Returns:
    ///     Contador de versiones.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Numero de claves.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Indica si el arbol esta vacio.
    ///
    /// Returns:
    ///     `true` sin claves.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Recorre en orden (in-order) acumulando pares.
fn collect(node: &Arc<Node>, out: &mut Vec<(i64, String)>) {
    if let Some(left) = &node.left {
        collect(left, out);
    }
    out.push((node.key, node.value.clone()));
    if let Some(right) = &node.right {
        collect(right, out);
    }
}

#[cfg(test)]
mod tests {
    use super::CowBTree;

    #[test] // @spec AC-0130-01
    fn test_ac_0130_01_cow_snapshot_isolation() {
        let mut tree = CowBTree::new();
        assert!(tree.is_empty());
        tree.insert(1, "a".to_string());
        assert_eq!(tree.get(1), Some("a".to_string()));
        assert_eq!(tree.epoch(), 1);

        // Snapshot O(1) con path copying: no ve escrituras posteriores.
        let snapshot = tree.fork();
        tree.insert(2, "b".to_string());
        assert_eq!(tree.len(), 2);
        assert_eq!(snapshot.get(2), None);
        assert_eq!(snapshot.get(1), Some("a".to_string()));
        assert_eq!(snapshot.epoch(), 1);

        let view = snapshot.snapshot().expect("vista");
        assert_eq!(view.keys, vec![1]);
        assert_eq!(view.values, vec!["a".to_string()]);
    }

    /// Muchas claves: busqueda correcta, orden y reemplazo.
    #[test]
    fn persistent_treap_is_correct_and_ordered() {
        let mut tree = CowBTree::new();
        for key in 0..1_000 {
            tree.insert(key * 3, format!("v{key}"));
        }
        assert_eq!(tree.len(), 1_000);
        for key in 0..1_000 {
            assert_eq!(tree.get(key * 3), Some(format!("v{key}")));
        }
        assert_eq!(tree.get(1), None);

        let view = tree.snapshot().expect("vista");
        assert!(view.keys.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(view.keys.len(), 1_000);

        // Reemplazo no cambia la cardinalidad.
        tree.insert(0, "nuevo".to_string());
        assert_eq!(tree.len(), 1_000);
        assert_eq!(tree.get(0), Some("nuevo".to_string()));

        // Los snapshots anteriores siguen viendo el valor viejo.
        let before = tree.fork();
        tree.insert(0, "otro".to_string());
        assert_eq!(before.get(0), Some("nuevo".to_string()));
        assert_eq!(tree.get(0), Some("otro".to_string()));
    }
}
