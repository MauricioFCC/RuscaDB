//! Tests de aceptación de la feature `incremental-backup` (SPEC-0120).

#![allow(clippy::expect_used)]

use ruscadb::IncrementalBackup;
use std::fs;

#[test] // @spec AC-0120-01
fn test_ac_0120_01_incremental_backup() {
    let dir = std::env::temp_dir().join(format!("ruscadb-incr-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("a.db"), b"hello").expect("write a");

    let backup = IncrementalBackup::new(&src);
    let copied = backup.snapshot(&dst).expect("snapshot");
    assert_eq!(copied, 1);

    let _ = fs::remove_dir_all(&dir);
}
