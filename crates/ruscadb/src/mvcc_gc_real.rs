#![allow(missing_docs)]
#![allow(dead_code)]
//! MVCC GC real (SPEC-0169): stub.

#[derive(Debug, Clone, Copy, Default)]
pub struct MvccGcReal {
    pub low_watermark: u64,
}

impl MvccGcReal {
    pub fn new() -> Self {
        Self { low_watermark: 0 }
    }
    pub fn set_watermark(&mut self, tx: u64) {
        self.low_watermark = tx;
    }
    pub fn is_obsolete(&self, created_tx: u64, deleted_tx: Option<u64>) -> bool {
        if let Some(dt) = deleted_tx {
            dt <= self.low_watermark
        } else {
            created_tx < self.low_watermark
        }
    }
    pub fn reclaim_count(&self, versions: &[u64]) -> usize {
        versions.iter().filter(|&&v| v < self.low_watermark).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0169-01
    fn test_ac_0169_01_gc() {
        let mut g = MvccGcReal::new();
        g.set_watermark(5);
        assert!(g.is_obsolete(2, Some(3)));
    }
    #[test] // @spec AC-0169-02
    fn test_ac_0169_02_reader_blocks() {
        let g = MvccGcReal::new();
        assert!(!g.is_obsolete(10, None));
    }
    #[test] // @spec AC-0169-03
    fn test_ac_0169_03_metrics() {
        let mut g = MvccGcReal::new();
        g.set_watermark(5);
        assert_eq!(g.reclaim_count(&[1, 2, 6]), 2);
    }
}
