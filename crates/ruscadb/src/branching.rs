//! Copy-on-write branching (SPEC-0139): stub.

/// Branch de base de datos.
#[derive(Debug, Clone)]
pub struct Branch {
    /// Nombre del branch.
    pub name: String,
    /// TxId del punto de fork.
    pub fork_tx: u64,
}

impl Branch {
    /// Crea un branch (fork).
    pub fn fork(name: impl Into<String>, fork_tx: u64) -> Self {
        Self {
            name: name.into(),
            fork_tx,
        }
    }

    /// Nombre.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// TxId del fork.
    pub fn fork_tx(&self) -> u64 {
        self.fork_tx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0139-01
    fn test_ac_0139_01_branch_fork() {
        let branch = Branch::fork("feature", 42);
        assert_eq!(branch.name(), "feature");
        assert_eq!(branch.fork_tx(), 42);
    }
}
