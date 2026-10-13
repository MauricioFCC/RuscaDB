//! Suite de aceptación de la estabilidad de API y semver (SPEC-0078).
//!
//! Verifica, como aserción sobre los artefactos de configuración/fuente (no
//! sobre comportamiento dinámico):
//!
//! - AC-0078-01: `RuscaError` es `#[non_exhaustive]` — añadir variantes futuras
//!   no rompe la compilación de un consumidor externo (el compilador le exige
//!   el brazo `_`).
//! - AC-0078-03: `.github/workflows/semver.yml` existe, corre en `pull_request`
//!   y usa `cargo-semver-checks` con **baseline git** (no contra crates.io).

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

/// Ruta absoluta a un fichero relativo a la raíz del workspace.
///
/// Args:
///     relative: Ruta relativa a la raíz del repositorio (separador `/`).
///
/// Returns:
///     La ruta resuelta desde `CARGO_MANIFEST_DIR/../..` (raíz del workspace).
fn workspace_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Lee un fichero del workspace como texto UTF-8.
///
/// Args:
///     relative: Ruta relativa a la raíz del workspace.
///
/// Returns:
///     El contenido del fichero.
///
/// Raises:
///     panic: si el fichero no existe o no es UTF-8 válido (el test debe fallar).
fn read_workspace_file(relative: &str) -> String {
    let path = workspace_path(relative);
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => panic!("no se pudo leer {}: {error}", path.display()),
    }
}

/// AC-0078-01 — `RuscaError` es `#[non_exhaustive]` (aserción sobre el fuente).
///
/// Lee `crates/ruscadb-core/src/error.rs` y exige el atributo
/// `#[non_exhaustive]` justo encima de `pub enum RuscaError`: sin él, un
/// consumidor externo haría un `match` exhaustivo y añadir una variante sería
/// un cambio incompatible (WHAT: falta el atributo; WHY: rompe semver; WHERE:
/// `ruscadb-core::error`).
#[test]
// @spec AC-0078-01
fn test_ac_0078_01_error_enum_is_non_exhaustive() {
    let source = read_workspace_file("crates/ruscadb-core/src/error.rs");
    let lines: Vec<&str> = source.lines().collect();
    let declaration = lines
        .iter()
        .position(|line| line.contains("pub enum RuscaError"));
    let Some(index) = declaration else {
        panic!("no se encontró 'pub enum RuscaError' en crates/ruscadb-core/src/error.rs");
    };
    let attribute_above = index.checked_sub(1).map(|previous| lines[previous].trim());
    assert_eq!(
        attribute_above,
        Some("#[non_exhaustive]"),
        "el atributo #[non_exhaustive] debe estar justo encima de 'pub enum RuscaError' \
         (línea {})",
        index + 1
    );
}

/// AC-0078-03 — `semver.yml` cableado con baseline git en PRs.
///
/// El workflow debe dispararse en `pull_request`, usar `cargo-semver-checks` y
/// tomar la baseline de git (`baseline-rev`), nunca de crates.io
/// (`baseline-version`), porque el workspace es `publish = false`.
#[test]
// @spec AC-0078-03
fn test_ac_0078_03_semver_workflow_wired() {
    let yaml = read_workspace_file(".github/workflows/semver.yml");
    assert!(
        yaml.contains("pull_request"),
        "semver.yml debe correr en pull_request"
    );
    assert!(
        yaml.contains("semver-checks"),
        "semver.yml debe usar cargo-semver-checks"
    );
    assert!(
        yaml.contains("baseline-rev"),
        "semver.yml debe comparar contra una baseline git (baseline-rev)"
    );
    assert!(
        !yaml.contains("baseline-version"),
        "semver.yml no debe usar la baseline de crates.io (baseline-version)"
    );
}
