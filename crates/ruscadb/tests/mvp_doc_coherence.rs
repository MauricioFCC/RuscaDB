//! Guard de coherencia de `docs/MVP.md` (SPEC-0095).
//!
//! El MVP documenta el producto para terceros; este test fija el contrato de
//! contenido: el doc debe citar las capacidades clave cerradas tras el MVP
//! inicial (AC-0095-01) y no puede reintroducir límites ya superados
//! (AC-0095-02). Solo lectura de fichero, sin dependencias.

#![forbid(unsafe_code)]
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

/// Raíz del workspace: dos niveles sobre `crates/ruscadb`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Lee `docs/MVP.md` del workspace.
fn read_mvp_doc() -> String {
    let path = workspace_root().join("docs").join("MVP.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("no se pudo leer {}: {error}", path.display()))
}

/// AC-0095-01 — el doc cita las capacidades vigentes clave.
#[test] // @spec AC-0095-01
fn test_ac_0095_01_mvp_doc_lists_current_capabilities() {
    let doc = read_mvp_doc().to_lowercase();
    for needle in [
        "cross-proceso",
        "fsyncpolicy",
        "update(table, id, scalars)",
        "overflow",
        "free-list",
        "cache.json",
        "rotación de claves",
    ] {
        assert!(
            doc.contains(needle),
            "docs/MVP.md debe documentar la capacidad '{needle}' (SPEC-0095/AC-0095-01)"
        );
    }
}

/// AC-0095-02 — el doc no afirma límites ya superados.
#[test] // @spec AC-0095-02
fn test_ac_0095_02_mvp_doc_drops_stale_limits() {
    let doc = read_mvp_doc();
    for stale in [
        "16 páginas = 64 KiB",
        "Sin rotación de claves ni KMS/HSM",
        "Sin GC en grafo/FTS",
        "Rollback no soportado en cifrado",
    ] {
        assert!(
            !doc.contains(stale),
            "docs/MVP.md no debe reintroducir el límite superado '{stale}' (SPEC-0095/AC-0095-02)"
        );
    }
}
