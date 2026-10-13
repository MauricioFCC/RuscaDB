//! Documentación de operación y `SECURITY.md` alineado (SPEC-0084).
//!
//! Lee los tres documentos **desde disco** (no strings inline) para que el gate
//! detecte *drift* entre lo documentado y el estado real del repositorio: si un
//! fichero cambia y rompe el contrato, estos tests fallan.

// El oráculo de integración puede fallar ruidosamente (equivalente a
// `allow-expect-in-tests` del workspace, que no alcanza a targets de
// integración). Ver `crates/ruscadb/tests/process_lock.rs`.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

/// Raíz del workspace, dos niveles por encima de `CARGO_MANIFEST_DIR`
/// (`crates/ruscadb` -> `crates` -> raíz).
///
/// Returns:
///     Ruta absoluta y canónica de la raíz del repositorio.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("la raíz del workspace debe existir")
}

/// Lee un fichero del workspace por ruta relativa.
///
/// Args:
///     relative: Ruta relativa a la raíz del workspace (p. ej. `SECURITY.md`).
///
/// Returns:
///     El contenido del fichero como UTF-8.
///
/// Raises:
///     Panic: si el fichero no existe o no es UTF-8 válido (con ruta legible).
fn read_workspace_file(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("no se pudo leer {}: {error}", path.display()))
}

/// AC-0084-01 — el runbook existe y contiene las cinco secciones mínimas.
#[test]
// @spec AC-0084-01
fn test_ac_0084_01_ops_runbook_sections() {
    // Arrange: el runbook debe existir en disco.
    let doc = read_workspace_file("docs/operacion.md");

    // Act + Assert: las cinco secciones exactas exigidas por SPEC-0084.
    for section in [
        "## Abrir una base (lock y DatabaseInUse)",
        "## Migración de esquema (forward-only)",
        "## Backup y restore",
        "## Rotación de claves",
        "## Troubleshooting",
    ] {
        assert!(
            doc.contains(section),
            "docs/operacion.md debe contener la sección `{section}`"
        );
    }

    // Act + Assert: enlaza los documentos de seguridad y verificación (NF-0084-03).
    assert!(
        doc.contains("SECURITY.md"),
        "el runbook debe enlazar SECURITY.md"
    );
    assert!(
        doc.contains("verification.md"),
        "el runbook debe enlazar docs/verification.md"
    );
}

/// AC-0084-02 — `SECURITY.md` no afirma mecanismos ausentes del repo.
#[test]
// @spec AC-0084-02
fn test_ac_0084_02_security_no_unfounded_claims() {
    // Arrange: el texto real de la política de seguridad.
    let security = read_workspace_file("SECURITY.md");

    // Assert: afirmaciones eliminadas en la auditoría por no existir en el repo.
    for absent in [
        "BLAKE3",
        "blake3",
        "cargo-vet",
        "cargo-geiger",
        "AES-256-GCM",
        "secrecy",
    ] {
        assert!(
            !security.contains(absent),
            "SECURITY.md no debe afirmar `{absent}`: no existe en el workspace (SPEC-0084)"
        );
    }

    // Assert: los mecanismos que SÍ existen siguen documentados (estado real).
    for present in [
        "CRC32C",
        "cargo-deny",
        "cargo-audit",
        "gitleaks",
        "SBOM",
        "XChaCha20-Poly1305",
    ] {
        assert!(
            security.contains(present),
            "SECURITY.md debe mencionar `{present}`: es un mecanismo real del repo"
        );
    }
}

/// AC-0084-03 — el README enlaza el runbook y `SECURITY.md` con rutas reales.
#[test]
// @spec AC-0084-03
fn test_ac_0084_03_readme_links_ops_docs() {
    // Arrange: el README público (inglés, mantenido a mano).
    let readme = read_workspace_file("README.md");

    // Assert: menciona los dos documentos.
    assert!(
        readme.contains("docs/operacion.md"),
        "README.md debe enlazar docs/operacion.md"
    );
    assert!(
        readme.contains("SECURITY.md"),
        "README.md debe enlazar SECURITY.md"
    );

    // Assert: las rutas resuelven a ficheros existentes (NF-0084-03).
    let root = workspace_root();
    assert!(
        root.join("docs/operacion.md").is_file(),
        "docs/operacion.md debe existir en disco"
    );
    assert!(
        root.join("SECURITY.md").is_file(),
        "SECURITY.md debe existir en disco"
    );
}
