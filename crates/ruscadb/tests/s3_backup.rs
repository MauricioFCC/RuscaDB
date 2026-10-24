//! Tests de aceptación de la feature `s3-backup` (SPEC-0128).

#![allow(clippy::expect_used)]

use ruscadb::S3Backup;

#[test] // @spec AC-0128-01
fn test_ac_0128_01_s3_manifest() {
    let dir = std::env::temp_dir().join(format!("ruscadb-s3-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(src.join("data.db"), b"hello").expect("write");

    let backup = S3Backup::new(&src, "my-bucket");
    let manifest = backup.manifest().expect("manifest");
    assert!(manifest.contains("my-bucket"));
    assert!(manifest.contains("data.db"));

    let _ = std::fs::remove_dir_all(&dir);
}
