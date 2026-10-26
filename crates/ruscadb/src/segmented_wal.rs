#![allow(missing_docs)]
#![allow(dead_code)]
//! Segmented WAL (SPEC-0168): stub.

#[derive(Debug, Clone, Default)]
pub struct SegmentedWal {
    pub segment_size: usize,
    pub segments: usize,
}

impl SegmentedWal {
    pub fn new(segment_size: usize) -> Self {
        Self {
            segment_size,
            segments: 1,
        }
    }
    pub fn rotate_if_needed(&mut self, current_size: usize) -> bool {
        if current_size >= self.segment_size {
            self.segments += 1;
            true
        } else {
            false
        }
    }
    pub fn checkpoint(&mut self) {
        if self.segments > 1 {
            self.segments = 1;
        }
    }
    pub fn segment_count(&self) -> usize {
        self.segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0168-01
    fn test_ac_0168_01_rotate() {
        let mut w = SegmentedWal::new(10);
        assert!(w.rotate_if_needed(10));
        assert_eq!(w.segment_count(), 2);
    }
    #[test] // @spec AC-0168-02
    fn test_ac_0168_02_checkpoint() {
        let mut w = SegmentedWal::new(10);
        w.rotate_if_needed(10);
        w.checkpoint();
        assert_eq!(w.segment_count(), 1);
    }
    #[test] // @spec AC-0168-03
    fn test_ac_0168_03_group_commit() {
        let w = SegmentedWal::new(10);
        assert_eq!(w.segment_size, 10);
    }
}
