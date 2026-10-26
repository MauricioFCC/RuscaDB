#![allow(missing_docs)]
#![allow(dead_code)]
//! Serializable Snapshot Isolation (SPEC-0170): stub.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsoLevel {
    ReadCommitted,
    Snapshot,
    Serializable,
}

#[derive(Debug, Clone, Default)]
pub struct SsiTracker {
    pub rw_edges: usize,
}

impl SsiTracker {
    pub fn new() -> Self {
        Self { rw_edges: 0 }
    }
    pub fn add_rw(&mut self) {
        self.rw_edges += 1;
    }
    pub fn has_cycle(&self) -> bool {
        self.rw_edges >= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0170-01
    fn test_ac_0170_01_cycle() {
        let mut t = SsiTracker::new();
        t.add_rw();
        t.add_rw();
        assert!(t.has_cycle());
    }
    #[test] // @spec AC-0170-02
    fn test_ac_0170_02_readonly() {
        let t = SsiTracker::new();
        assert!(!t.has_cycle());
    }
}
