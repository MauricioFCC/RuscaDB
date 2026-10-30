//! Copy-on-write branching (SPEC-0139): ramas ligeras sobre snapshots.
//!
//! Cada rama es un [`CowBTree`] persistente: `fork_from` clona la raiz en
//! `O(1)` y las escrituras de una rama no afectan a las demas (path copying).
//! `merge_into` aplica una union con prioridad al destino (no pisa claves
//! existentes) y devuelve cuantas claves nuevas se anadieron.

use std::collections::BTreeMap;

use crate::cow_btree::{CowBTree, CowBTreeNode};

/// Rama de la base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    name: String,
    fork_tx: u64,
}

impl Branch {
    /// Crea una rama en `fork_tx`.
    ///
    /// Args:
    ///     name: Nombre de la rama.
    ///     fork_tx: Transaccion desde la que bifurca.
    ///
    /// Returns:
    ///     Rama lista.
    pub fn fork(name: impl Into<String>, fork_tx: u64) -> Self {
        Self {
            name: name.into(),
            fork_tx,
        }
    }

    /// Nombre de la rama.
    ///
    /// Returns:
    ///     Nombre declarado.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Transaccion de fork.
    ///
    /// Returns:
    ///     `tx_id` de bifurcacion.
    pub fn fork_tx(&self) -> u64 {
        self.fork_tx
    }
}

/// Estado interno de una rama.
#[derive(Debug, Clone)]
struct BranchState {
    meta: Branch,
    tree: CowBTree,
}

/// Gestor de ramas copy-on-write.
#[derive(Debug, Clone)]
pub struct BranchManager {
    branches: BTreeMap<String, BranchState>,
}

impl Default for BranchManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BranchManager {
    /// Crea el gestor con la rama `main` en `fork_tx = 0`.
    ///
    /// Returns:
    ///     Gestor con una rama vacia.
    pub fn new() -> Self {
        let mut branches = BTreeMap::new();
        branches.insert(
            "main".to_string(),
            BranchState {
                meta: Branch::fork("main", 0),
                tree: CowBTree::new(),
            },
        );
        Self { branches }
    }

    /// Crea una rama vacia.
    ///
    /// Args:
    ///     name: Nombre nuevo.
    ///     fork_tx: Transaccion de bifurcacion.
    ///
    /// Returns:
    ///     `Ok(())` si no existia.
    ///
    /// Errors:
    ///     Texto accionable si la rama ya existe o el nombre esta vacio.
    pub fn create(&mut self, name: impl Into<String>, fork_tx: u64) -> Result<(), String> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err("la rama necesita un nombre".to_string());
        }
        if self.branches.contains_key(&name) {
            return Err(format!("la rama '{name}' ya existe"));
        }
        self.branches.insert(
            name.clone(),
            BranchState {
                meta: Branch::fork(name, fork_tx),
                tree: CowBTree::new(),
            },
        );
        Ok(())
    }

    /// Bifurca una rama existente (`O(1)`, comparte el arbol).
    ///
    /// Args:
    ///     source: Rama origen.
    ///     name: Nombre de la rama nueva.
    ///     fork_tx: Transaccion de bifurcacion.
    ///
    /// Returns:
    ///     `Ok(())` si el origen existe y el destino no.
    ///
    /// Errors:
    ///     Texto accionable si falta el origen o el destino ya existe.
    pub fn fork_from(
        &mut self,
        source: &str,
        name: impl Into<String>,
        fork_tx: u64,
    ) -> Result<(), String> {
        let name = name.into();
        let Some(source_state) = self.branches.get(source) else {
            return Err(format!("la rama origen '{source}' no existe"));
        };
        if self.branches.contains_key(&name) {
            return Err(format!("la rama '{name}' ya existe"));
        }
        let tree = source_state.tree.fork();
        self.branches.insert(
            name.clone(),
            BranchState {
                meta: Branch::fork(name, fork_tx),
                tree,
            },
        );
        Ok(())
    }

    /// Escribe una clave en una rama.
    ///
    /// Args:
    ///     branch: Rama destino.
    ///     key: Clave.
    ///     value: Valor.
    ///
    /// Returns:
    ///     `Ok(())` si la rama existe.
    ///
    /// Errors:
    ///     Texto accionable si la rama no existe.
    pub fn insert(
        &mut self,
        branch: &str,
        key: i64,
        value: impl Into<String>,
    ) -> Result<(), String> {
        let Some(state) = self.branches.get_mut(branch) else {
            return Err(format!("la rama '{branch}' no existe"));
        };
        state.tree.insert(key, value.into());
        Ok(())
    }

    /// Lee una clave de una rama.
    ///
    /// Args:
    ///     branch: Rama.
    ///     key: Clave.
    ///
    /// Returns:
    ///     Valor si existe.
    pub fn get(&self, branch: &str, key: i64) -> Option<String> {
        self.branches
            .get(branch)
            .and_then(|state| state.tree.get(key))
    }

    /// Nombres de las ramas (orden alfabetico).
    ///
    /// Returns:
    ///     Lista de nombres.
    pub fn names(&self) -> Vec<String> {
        self.branches.keys().cloned().collect()
    }

    /// Numero de claves de una rama.
    ///
    /// Args:
    ///     branch: Rama.
    ///
    /// Returns:
    ///     Cardinalidad, o `None` si la rama no existe.
    pub fn len(&self, branch: &str) -> Option<usize> {
        self.branches.get(branch).map(|state| state.tree.len())
    }

    /// Vista materializada de una rama.
    ///
    /// Args:
    ///     branch: Rama.
    ///
    /// Returns:
    ///     Claves/valores ordenados, o `None`.
    pub fn snapshot(&self, branch: &str) -> Option<CowBTreeNode> {
        self.branches
            .get(branch)
            .and_then(|state| state.tree.snapshot())
    }

    /// Transaccion de fork de una rama.
    ///
    /// Args:
    ///     branch: Rama.
    ///
    /// Returns:
    ///     `tx_id` de bifurcacion, o `None` si la rama no existe.
    pub fn fork_tx(&self, branch: &str) -> Option<u64> {
        self.branches.get(branch).map(|state| state.meta.fork_tx())
    }

    /// Elimina una rama.
    ///
    /// Args:
    ///     branch: Rama.
    ///
    /// Returns:
    ///     `true` si existia.
    pub fn drop_branch(&mut self, branch: &str) -> bool {
        self.branches.remove(branch).is_some()
    }

    /// Fusiona `source` en `target` (union con prioridad al destino).
    ///
    /// Args:
    ///     source: Rama origen.
    ///     target: Rama destino.
    ///
    /// Returns:
    ///     Numero de claves nuevas anadidas al destino.
    ///
    /// Errors:
    ///     Texto accionable si falta alguna rama.
    pub fn merge_into(&mut self, source: &str, target: &str) -> Result<usize, String> {
        let Some(source_state) = self.branches.get(source) else {
            return Err(format!("la rama origen '{source}' no existe"));
        };
        let source_view = source_state.tree.snapshot();
        let Some(target_state) = self.branches.get_mut(target) else {
            return Err(format!("la rama destino '{target}' no existe"));
        };
        let Some(source_view) = source_view else {
            return Ok(0);
        };
        let mut added = 0usize;
        for (key, value) in source_view.keys.iter().zip(source_view.values.iter()) {
            if target_state.tree.get(*key).is_none() {
                target_state.tree.insert(*key, value.clone());
                added += 1;
            }
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::{Branch, BranchManager};

    #[test] // @spec AC-0139-01
    fn test_ac_0139_01_branch_fork() {
        let branch = Branch::fork("feature", 42);
        assert_eq!(branch.name(), "feature");
        assert_eq!(branch.fork_tx(), 42);
        assert_eq!(branch.clone().name(), "feature");
    }

    /// Fork O(1): la rama nueva ve el estado previo y queda aislada.
    #[test]
    fn forks_are_isolated_and_mergable() {
        let mut manager = BranchManager::new();
        manager.insert("main", 1, "base").expect("insert main");
        manager.fork_from("main", "feature", 7).expect("fork");
        assert_eq!(manager.get("feature", 1), Some("base".to_string()));
        assert_eq!(manager.fork_tx("feature"), Some(7));
        assert_eq!(manager.fork_tx("missing"), None);

        manager.insert("feature", 2, "nuevo").expect("insert");
        assert_eq!(manager.get("main", 2), None, "aislamiento COW");
        assert_eq!(manager.len("feature"), Some(2));
        assert_eq!(manager.len("main"), Some(1));

        // Merge: solo anade claves que faltan.
        assert_eq!(manager.merge_into("feature", "main").expect("merge"), 1);
        assert_eq!(manager.get("main", 2), Some("nuevo".to_string()));
        assert_eq!(manager.merge_into("feature", "main").expect("merge"), 0);

        // Errores accionables.
        assert!(manager.create("main", 0).is_err());
        assert!(manager.create("", 0).is_err());
        assert!(manager.fork_from("missing", "x", 0).is_err());
        assert!(manager.insert("missing", 1, "x").is_err());
        assert!(manager.merge_into("missing", "main").is_err());
        assert!(manager.drop_branch("feature"));
        assert!(!manager.drop_branch("feature"));
        assert_eq!(manager.names(), vec!["main".to_string()]);
        assert!(manager.snapshot("main").is_some());

        // Una rama vacia se fusiona sin cambios (no es un error).
        manager.create("empty", 0).expect("crear rama vacia");
        assert_eq!(manager.merge_into("empty", "main").expect("merge vacio"), 0);
    }
}
