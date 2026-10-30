//! Copy-on-write branching (SPEC-0139): ramas ligeras sobre un `tx_id`.
//!
//! Una rama es una vista nombrada que arranca en un punto de fork; el
//! aislamiento lo aporta el snapshot MVCC de ese `tx_id`.

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

#[cfg(test)]
mod tests {
    use super::Branch;

    #[test] // @spec AC-0139-01
    fn test_ac_0139_01_branch_fork() {
        let branch = Branch::fork("feature", 42);
        assert_eq!(branch.name(), "feature");
        assert_eq!(branch.fork_tx(), 42);
        assert_eq!(branch.clone().name(), "feature");
    }
}
