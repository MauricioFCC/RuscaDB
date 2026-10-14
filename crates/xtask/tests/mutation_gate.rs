//! Tests de aceptación del gate de mutación sobre el diff (SPEC-0098).
//!
//! Invoca `scripts/mutation_diff_gate.py` sobre fixtures con los dos esquemas
//! de `outcomes.json` de cargo-mutants: cadena (v25+, `"CaughtMutant"`) y dict
//! legacy (`{"caught": N}`); además comprueba el fallo ruidoso ante un estado
//! desconocido (nunca verde silencioso). Solo `std`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Raíz del workspace: dos niveles sobre `crates/xtask`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Directorio de fixtures de mutación.
fn fixtures_dir() -> PathBuf {
    workspace_root()
        .join("crates")
        .join("xtask")
        .join("tests")
        .join("fixtures")
        .join("mutation")
}

/// Devuelve el intérprete de Python disponible (`python` o `python3`).
fn python_interpreter() -> String {
    for candidate in ["python", "python3"] {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return candidate.to_string();
        }
    }
    panic!(
        "WHAT: falta un intérprete de Python para los tests de SPEC-0098. \
         WHY: scripts/mutation_diff_gate.py es Python. WHERE: instala Python 3 \
         en PATH; test en crates/xtask/tests/mutation_gate.rs."
    );
}

/// Ejecuta el gate sobre un fixture con el umbral dado.
fn run_gate(fixture: &str, min_score: u32) -> Output {
    let script = workspace_root()
        .join("scripts")
        .join("mutation_diff_gate.py");
    Command::new(python_interpreter())
        .current_dir(workspace_root())
        .arg(script)
        .arg(fixtures_dir().join(fixture))
        .args(["--min-score", &min_score.to_string()])
        .output()
        .unwrap_or_else(|error| panic!("no se pudo ejecutar el gate de mutación: {error}"))
}

/// Concatena stdout y stderr de la salida.
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// AC-0098-01 — el esquema de cadena (cargo-mutants v25+) se puntúa.
#[test] // @spec AC-0098-01
fn test_ac_0098_01_string_summary_schema_is_scored() {
    // 2 caught + 1 timeout = 3 matados de 4 puntuables (1 missed) => 75 %.
    let output = run_gate("outcomes_v25.json", 70);
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "75 % >= 70 debe pasar; salida:\n{text}"
    );
    assert!(
        text.contains("MS_diff=75.0"),
        "el gate debe reportar el score real (75.0), no 'sin mutantes'; salida:\n{text}"
    );

    // Con umbral 80 el mismo fixture falla (75 < 80).
    let strict = run_gate("outcomes_v25.json", 80);
    assert_eq!(
        strict.status.code(),
        Some(1),
        "75 % < 80 debe fallar; salida:\n{}",
        combined(&strict)
    );
}

/// AC-0098-02 — el esquema dict legacy sigue puntuándose (compatibilidad).
#[test] // @spec AC-0098-02
fn test_ac_0098_02_legacy_dict_schema_still_supported() {
    let output = run_gate("outcomes_legacy.json", 70);
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "el esquema legacy (3 caught / 1 missed) debe pasar con 70; salida:\n{text}"
    );
    assert!(
        text.contains("MS_diff=75.0"),
        "el score legacy debe ser 75.0; salida:\n{text}"
    );
}

/// AC-0098-03 — un estado desconocido falla ruidoso (exit 2), nunca verde.
#[test] // @spec AC-0098-03
fn test_ac_0098_03_unknown_summary_fails_loudly() {
    let output = run_gate("outcomes_unknown.json", 70);
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(2),
        "un summary desconocido debe salir 2; salida:\n{text}"
    );
    assert!(
        text.to_lowercase().contains("no reconocidos"),
        "el fallo debe nombrar el estado no reconocido; salida:\n{text}"
    );
}
