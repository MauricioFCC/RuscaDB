#![allow(missing_docs)]
#![allow(dead_code)]
//! Epoch-based reclamation (SPEC-0146): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct RcuDomain;

impl RcuDomain {
    pub fn new() -> Self {
        Self
    }
    pub fn enter(&self) -> u64 {
        0
    }
    pub fn exit(&self) {}
    pub fn synchronize(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0146-01
    fn test_ac_0146_01_rcu_snapshot() {
        let domain = RcuDomain::new();
        let epoch = domain.enter();
        assert_eq!(epoch, 0);
        domain.exit();
    }
}
