//! Aceptación de SPEC-0191 (`migrate --check`).

#![allow(clippy::expect_used)]

use std::process::Command;

#[test] // @spec AC-0191-01
fn test_ac_0191_01_migrate_check() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("migrate.db");
    {
        let _database = ruscadb::Database::open(ruscadb::DbConfig::new(&path, 32)).expect("open");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ruscadb"))
        .args(["migrate", "--db", path.to_str().expect("ruta"), "check"])
        .output()
        .expect("run");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("schema_version=1"), "{text}");
    assert!(text.contains("ok=true"), "{text}");
}
