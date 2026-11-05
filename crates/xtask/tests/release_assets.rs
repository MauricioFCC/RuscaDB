//! SPEC-0208 — matriz de assets del release e instalación con fallback.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(workspace_root().join(relative))
        .unwrap_or_else(|error| panic!("no se pudo leer {relative}: {error}"))
}

fn python_interpreter() -> String {
    for candidate in ["python", "python3"] {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return candidate.to_string();
        }
    }
    panic!("falta Python para verificar SPEC-0208");
}

fn run_validator() -> Output {
    let script = workspace_root()
        .join("scripts")
        .join("check_release_assets.py");
    Command::new(python_interpreter())
        .arg(script)
        .arg("--dry-run")
        .output()
        .unwrap_or_else(|error| panic!("no se pudo ejecutar el validador: {error}"))
}

/// AC-0208-01 — la matriz offline valida 8 targets y sus assets.
#[test]
// @spec AC-0208-01
fn test_ac_0208_01_release_assets_matrix() {
    let output = run_validator();
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("8 targets"), "{text}");
    assert!(text.contains("8 assets"), "{text}");
}

/// AC-0208-02 — los instaladores fallan con fallback git si no hay release.
#[test]
// @spec AC-0208-02
fn test_ac_0208_02_install_fallback() {
    for file in ["scripts/install.sh", "scripts/install.ps1"] {
        let text = read(file);
        for needle in [
            "releases/latest",
            "releases/download",
            "cargo install --git",
            "--package ruscadb-cli --locked",
            "tag v*",
        ] {
            assert!(text.contains(needle), "{file} sin '{needle}'");
        }
    }
}

/// AC-0208-03 — release.yml publica los assets y checksums de la matriz.
#[test]
// @spec AC-0208-03
fn test_ac_0208_03_release_workflow() {
    let yaml = read(".github/workflows/release.yml");
    for target in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        assert!(yaml.contains(target), "release.yml sin target {target}");
    }
    assert!(
        yaml.contains("ruscadb-${{ matrix.target }}.tar.gz"),
        "release.yml no empaqueta el asset"
    );
    assert!(
        yaml.contains("ruscadb-${{ matrix.target }}.sha256"),
        "release.yml no publica checksum"
    );
}

/// AC-0208-04 — README e instalación documentan fallback y `--dry-run`.
#[test]
// @spec AC-0208-04
fn test_ac_0208_04_install_dry_run_docs() {
    for file in ["README.md", "docs/instalacion.md"] {
        let text = read(file);
        for needle in ["cargo install --git", "--dry-run", "404"] {
            assert!(text.contains(needle), "{file} sin '{needle}'");
        }
    }
}
