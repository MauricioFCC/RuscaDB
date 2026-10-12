//! Smoke de los benches criterion (SPEC-0059, AC-0059-01).
//!
//! El test es `#[ignore]` para no penalizar el T1 (NF-0059-02: cero impacto
//! en builds normales). Nightly lo ejecuta explícitamente:
//! `cargo test -p ruscadb --test bench_smoke -- --ignored --exact`.

use std::process::Command;

/// Ejecuta los 3 benches criterion en modo smoke (una pasada, sin medición).
#[test]
#[ignore = "T3: se ejecuta en nightly (SPEC-0059)"]
// @spec AC-0059-01
fn test_ac_0059_01_benches_compile_and_run() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .expect("crates/")
        .parent()
        .expect("raíz del workspace");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    // Benches explícitos (no `--workspace`): evita que el harness de libtest
    // re-ejecute los unittests de otros crates en modo bench.
    let status = Command::new(cargo)
        .args([
            "bench",
            "-p",
            "ruscadb",
            "--bench",
            "facade_smoke",
            "-p",
            "ruscadb-wal",
            "--bench",
            "wal_commit",
            "-p",
            "ruscadb-vector",
            "--bench",
            "knn_latency",
            "--",
            "--test",
        ])
        .current_dir(workspace_root)
        .status()
        .expect("no se pudo lanzar `cargo bench`");
    assert!(status.success(), "smoke de los benches criterion falló");
}
