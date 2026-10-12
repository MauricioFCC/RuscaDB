//! Veredicto ADR-013 y aislamiento de la feature (SPEC-0060).
//!
//! Cubre AC-0060-01 (el veredicto vive en la SSOT pusheada del roadmap,
//! §4.6 — `docs/adr/` es anexo local-only *gitignored* por ADR_POLICY) y
//! AC-0060-03 (DataFusion fuera del árbol de dependencias por defecto).
#![allow(clippy::expect_used)]

use std::path::Path;
use std::process::Command;

/// Raíz del workspace (crates/ruscadb-query -> ../..).
fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("raíz del workspace")
}

/// AC-0060-01 — el ADR-013 de revalidación está registrado en el roadmap.
///
/// Given: la tabla de ADRs del roadmap (§4.6).
/// When: se lee la fila ADR-013.
/// Then: tiene veredicto único, tradeoff (+/-) y evidencia numérica.
#[test]
// @spec AC-0060-01
fn test_ac_0060_01_adr_recorded() {
    let roadmap = workspace_root().join("docs").join("RuscaDB-roadmap.md");
    let text = std::fs::read_to_string(&roadmap)
        .unwrap_or_else(|error| panic!("no se pudo leer {}: {error}", roadmap.display()));

    let row = text
        .lines()
        .find(|line| line.contains("| ADR-013 |"))
        .expect("la tabla §4.6 debe registrar ADR-013");
    assert!(
        row.contains("DataFusion"),
        "ADR-013 debe tratar de DataFusion: {row}"
    );
    // Tradeoff con pros (+) y contras (-) en la misma fila.
    assert!(
        row.contains('+') && row.contains('-'),
        "ADR-013 debe listar tradeoff con +/-: {row}"
    );
    // Evidencia numérica (cualquier cifra con unidad en la fila).
    let has_number = row.chars().any(|character| character.is_ascii_digit());
    assert!(
        has_number,
        "ADR-013 debe citar evidencia numérica (coste/árbol/paridad): {row}"
    );
}

/// AC-0060-03 — por defecto DataFusion no está en el árbol de dependencias.
///
/// Given: build por defecto (features default).
/// When: se inspecciona `cargo tree -e normal -p ruscadb-query`.
/// Then: la salida no menciona `datafusion`.
#[test]
// @spec AC-0060-03
fn test_ac_0060_03_feature_off_by_default() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["tree", "-p", "ruscadb-query", "-e", "normal"])
        .current_dir(workspace_root())
        .output()
        .expect("no se pudo lanzar `cargo tree`");
    let tree = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "`cargo tree` falló: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !tree.contains("datafusion"),
        "el árbol por defecto NO debe incluir DataFusion (NF-0060-01):\n{tree}"
    );
}
