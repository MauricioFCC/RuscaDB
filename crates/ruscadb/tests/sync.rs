//! Tests de aceptación de la feature `sync` (SPEC-0138).

#![allow(clippy::expect_used)]

use ruscadb::SyncProtocol;

#[test] // @spec AC-0138-01
fn test_ac_0138_01_sync_checkpoint() {
    let mut sync = SyncProtocol::new();
    sync.checkpoint(42, 7);
    assert_eq!(sync.checkpoint_lsn, 42);
    assert_eq!(sync.epoch, 7);
}
