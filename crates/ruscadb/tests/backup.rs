//! Tests de aceptación de la feature `backup` (SPEC-0116).

#![allow(clippy::expect_used)]

use ruscadb::Backup;
use std::fs;

#[test] // @spec AC-0116-01
fn test_ac_0116_01_backup_snapshot() {
    let dir = std::env::temp_dir().join(format!("ruscadb-backup-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let src = dir.join("src");
    let dst = dir.join("dst");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("data.db"), b"hello").expect("write data");
    fs::write(src.join("data.wal"), b"world").expect("write wal");

    let backup = Backup::new(&src);
    let copied = backup.snapshot(&dst).expect("snapshot");
    assert_eq!(copied, 2);
    assert!(dst.join("data.db").exists());

    let _ = fs::remove_dir_all(&dir);
}
