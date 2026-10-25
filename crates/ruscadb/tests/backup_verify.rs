//! Tests de aceptación de la feature `backup-verify` (SPEC-0157).

#![allow(clippy::expect_used)]

use ruscadb::BackupVerifier;
use std::fs;

#[test] // @spec AC-0157-01
fn test_ac_0157_01_backup_verify() {
    let dir = std::env::temp_dir().join(format!("bv-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir_all(&src).expect("create src");
    fs::create_dir_all(&dst).expect("create dst");
    fs::write(src.join("a.db"), b"hello").expect("write");
    fs::write(dst.join("a.db"), b"hello").expect("write");
    let verifier = BackupVerifier::new(&src, &dst);
    assert!(verifier.verify().unwrap());
    let _ = fs::remove_dir_all(&dir);
}
