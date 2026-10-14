//! Tests de aceptación de SPEC-0090 (job MSRV en CI).
//!
//! Leen `ci.yml`, el `Cargo.toml` raíz y `scripts/check_ci_config.py` como
//! texto y verifican que el piso MSRV (fuente única: `rust-version`) está
//! cableado y validado. Solo `std`.

use std::path::{Path, PathBuf};

/// Raíz del workspace: dos niveles sobre `crates/xtask`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Lee un archivo relativo a la raíz del workspace.
fn read_root(relative: &str) -> String {
    std::fs::read_to_string(workspace_root().join(relative))
        .unwrap_or_else(|e| panic!("no se pudo leer {relative}: {e}"))
}

/// AC-0090-01: `ci.yml` declara el job `msrv` con el toolchain del piso (1.89)
/// y `cargo check --workspace --all-features --locked`.
#[test]
// @spec AC-0090-01
fn test_ac_0090_01_msrv_job_in_ci() {
    let ci = read_root(".github/workflows/ci.yml");
    assert!(ci.contains("msrv:"), "ci.yml debe declarar el job 'msrv'");
    assert!(
        ci.contains("dtolnay/rust-toolchain@1.89"),
        "el job msrv debe usar el toolchain MSRV 1.89"
    );
    assert!(
        ci.contains("cargo check --workspace --all-features --locked"),
        "el job msrv debe correr 'cargo check --workspace --all-features --locked'"
    );
}

/// AC-0090-02: la versión del job `msrv` coincide con `rust-version` del
/// workspace (fuente única) y `check_ci_config.py` lo valida sin duplicar el
/// número.
#[test]
// @spec AC-0090-02
fn test_ac_0090_02_msrv_version_single_source() {
    let version = read_root("Cargo.toml")
        .lines()
        .find_map(|line| {
            line.trim().strip_prefix("rust-version").map(|rest| {
                rest.trim_start_matches([' ', '='])
                    .trim()
                    .trim_matches('"')
                    .to_string()
            })
        })
        .expect("el Cargo.toml raíz debe declarar 'rust-version'");
    assert_eq!(version, "1.89", "el MSRV del workspace debe ser 1.89");

    let ci = read_root(".github/workflows/ci.yml");
    assert!(
        ci.contains(&format!("rust-toolchain@{version}")),
        "el job msrv debe usar rust-toolchain@{version} (coincide con rust-version)"
    );

    let guard = read_root("scripts/check_ci_config.py");
    assert!(
        guard.contains("check_msrv_job"),
        "check_ci_config.py no valida el job msrv"
    );
    assert!(
        guard.contains("rust-version"),
        "check_ci_config.py debe leer 'rust-version' (fuente única del MSRV)"
    );
}
