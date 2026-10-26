#![allow(missing_docs)]
#![cfg(feature = "blockmax-wand")]
#![allow(clippy::expect_used)]
use ruscadb::BlockMaxWand;
#[test] // @spec AC-0177-01
fn test_ac_0177_01_topk() {
    let w = BlockMaxWand::new();
    let r = w.top_k(&[(1, 0.5), (2, 0.9), (3, 0.1)], 2);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].0, 2);
}
#[test] // @spec AC-0177-02
fn test_ac_0177_02_prune() {
    let w = BlockMaxWand::new();
    let r = w.top_k(&[(1, 0.5)], 1);
    assert_eq!(r.len(), 1);
}
