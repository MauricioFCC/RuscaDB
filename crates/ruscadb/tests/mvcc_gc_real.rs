#![allow(missing_docs)]
#![cfg(feature = "mvcc-gc-real")]
#![allow(clippy::expect_used)]
use ruscadb::MvccGcReal;
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
