#![allow(missing_docs)]
#![allow(dead_code)]
//! CRDT sync real (SPEC-0179): stub.

#[derive(Debug, Clone, Default)]
pub struct CrdtSyncReal {
    pub clock: u64,
}

impl CrdtSyncReal {
    pub fn new() -> Self {
        Self { clock: 0 }
    }
    pub fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }
    pub fn merge(&self, a: i64, b: i64) -> i64 {
        a.max(b)
    }
    pub fn delta(&self) -> u64 {
        self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0179-01
    fn test_ac_0179_01_merge() {
        let c = CrdtSyncReal::new();
        assert_eq!(c.merge(3, 5), 5);
    }
    #[test] // @spec AC-0179-02
    fn test_ac_0179_02_delta() {
        let mut c = CrdtSyncReal::new();
        c.tick();
        assert_eq!(c.delta(), 1);
    }
}
