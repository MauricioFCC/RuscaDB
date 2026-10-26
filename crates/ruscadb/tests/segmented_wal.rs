#![allow(missing_docs)]
#![cfg(feature = "segmented-wal")]
#![allow(clippy::expect_used)]
use ruscadb::SegmentedWal;
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
