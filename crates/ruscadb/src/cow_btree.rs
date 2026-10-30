//! Copy-on-write B+tree (SPEC-0130): nodo inmutable versionado por epoch.
//!
//! Modelo inicial: un unico nodo hoja que se copia en cada escritura
//! (copy-on-write) y publica un `epoch` nuevo; los lectores conservan el nodo
//! anterior. La estructura de arbol completa queda como trabajo futuro.

/// Nodo hoja copy-on-write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CowBTreeNode {
    /// Claves ordenadas por insercion.
    pub keys: Vec<i64>,
    /// Valores alineados con `keys`.
    pub values: Vec<String>,
    /// Epoch en que se creo el nodo.
    pub epoch: u64,
}

/// Arbol copy-on-write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CowBTree {
    root: Option<CowBTreeNode>,
    epoch: u64,
}

impl CowBTree {
    /// Crea el arbol vacio.
    ///
    /// Returns:
    ///     Arbol sin nodos.
    pub fn new() -> Self {
        Self {
            root: None,
            epoch: 0,
        }
    }

    /// Inserta una clave (copia el nodo y publica un epoch nuevo).
    ///
    /// Args:
    ///     key: Clave.
    ///     value: Valor asociado.
    pub fn insert(&mut self, key: i64, value: String) {
        self.epoch += 1;
        let mut node = self.root.take().unwrap_or_default();
        node.keys.push(key);
        node.values.push(value);
        node.epoch = self.epoch;
        self.root = Some(node);
    }

    /// Busca una clave.
    ///
    /// Args:
    ///     key: Clave.
    ///
    /// Returns:
    ///     Valor asociado, si existe.
    pub fn get(&self, key: i64) -> Option<String> {
        self.root.as_ref().and_then(|node| {
            node.keys
                .iter()
                .position(|candidate| *candidate == key)
                .map(|index| node.values[index].clone())
        })
    }

    /// Snapshot inmutable del nodo actual (para lectores concurrentes).
    ///
    /// Returns:
    ///     Copia del nodo raiz, o `None` si el arbol esta vacio.
    pub fn snapshot(&self) -> Option<CowBTreeNode> {
        self.root.clone()
    }

    /// Epoch actual.
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
        self.root.as_ref().map_or(0, |node| node.keys.len())
    }

    /// Indica si el arbol esta vacio.
    ///
    /// Returns:
    ///     `true` sin claves.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

        let snapshot = tree.snapshot().expect("snapshot");
        tree.insert(2, "b".to_string());
        assert_eq!(tree.len(), 2);
        assert_eq!(
            snapshot.keys,
            vec![1],
            "el snapshot no ve la escritura nueva"
        );
        assert_eq!(snapshot.epoch, 1);
        assert_eq!(tree.epoch(), 2);
    }
}
