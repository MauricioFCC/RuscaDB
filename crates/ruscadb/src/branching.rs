#![allow(missing_docs)]
#![allow(dead_code)]
//! Copy-on-write branching (SPEC-0139): stub.

#[derive(Debug, Clone)]
pub struct Branch {
    name: String,
    fork_tx: u64,
}

impl Branch {
    pub fn fork(name: impl Into<String>, fork_tx: u64) -> Self {
        Self {
            name: name.into(),
            fork_tx,
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
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
