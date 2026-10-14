//! Tests de aceptación del gate RED-first (SPEC-0094).
//!
//! Cubre AC-0094-02 (job `red-first` cableado y validado por
//! `check_ci_config.py`), AC-0094-03 (`cargo xtask red` rechaza un spec
//! `implemented` y un path inexistente sin lanzar cargo) y AC-0094-04 (el
//! script lista solo los specs en curso). La clasificación RED/pass/missing de
//! `cargo test` se cubre en las pruebas unitarias de `src/red.rs`
//! (AC-0094-01).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Raíz del workspace: dos niveles sobre `crates/xtask`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
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
        "WHAT: falta un intérprete de Python para los tests de SPEC-0094. \
         WHY: scripts/red_first_gate.py es Python. WHERE: instala Python 3 en \
         PATH; test en crates/xtask/tests/red_first.rs."
    );
}

/// Concatena stdout y stderr de la salida.
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// AC-0094-02 — el job `red-first` está cableado (PR) y el guard lo valida.
#[test] // @spec AC-0094-02
fn test_ac_0094_02_red_first_job_wired_in_ci_and_guard() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("no se pudo leer ci.yml");
    assert!(
        ci.contains("red-first:"),
        "ci.yml debe declarar el job 'red-first' (SPEC-0094)"
    );
    assert!(
        ci.contains("red_first_gate.py"),
        "el job 'red-first' debe invocar scripts/red_first_gate.py"
    );
    assert!(
        ci.contains("--diff"),
        "el gate RED-first debe analizar el diff del PR (--diff)"
    );

    let guard = std::fs::read_to_string(workspace_root().join("scripts/check_ci_config.py"))
        .expect("no se pudo leer check_ci_config.py");
    assert!(
        guard.contains("check_red_first_job"),
        "check_ci_config.py debe validar el job 'red-first'"
    );
}

/// AC-0094-03 — `cargo xtask red` rechaza un spec `implemented` (exit 2) y un
/// path inexistente (exit 2), sin lanzar `cargo test`.
#[test] // @spec AC-0094-03
fn test_ac_0094_03_red_rejects_implemented_and_missing() {
    let xtask = env!("CARGO_BIN_EXE_xtask");

    let implemented = Command::new(xtask)
        .current_dir(workspace_root())
        .args(["red", "specs/semantic_cache_persistence.md"])
        .output()
        .expect("ejecutar xtask red (implemented)");
    let text = combined(&implemented);
    assert_eq!(
        implemented.status.code(),
        Some(2),
        "un spec implemented no aplica al gate RED-first (exit 2); salida:\n{text}"
    );
    assert!(
        text.contains("planned") || text.contains("draft") || text.contains("status"),
        "el rechazo debe explicar que solo aplica a specs en curso; salida:\n{text}"
    );

    let missing = Command::new(xtask)
        .current_dir(workspace_root())
        .args(["red", "specs/definitely-missing-spec.md"])
        .output()
        .expect("ejecutar xtask red (missing)");
    assert_eq!(
        missing.status.code(),
        Some(2),
        "un spec inexistente debe fallar ruidoso (exit 2):\n{}",
        combined(&missing)
    );
}

/// AC-0094-04 — el script lista solo los specs en curso (planned), no los
/// implemented.
#[test] // @spec AC-0094-04
fn test_ac_0094_04_gate_lists_only_in_progress_specs() {
    let dir = std::env::temp_dir().join(format!("ruscadb_red_first_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("directorio temporal");
    let planned = dir.join("planned_spec.md");
    let implemented = dir.join("implemented_spec.md");
    std::fs::write(
        &planned,
        "---\nid: SPEC-9999\nfeature: fixture\nstatus: planned\nacceptance_criteria:\n  - id: AC-9999-01\n    test: test_ac_9999_01_fixture\n---\n\n# fixture\n",
    )
    .expect("escribir spec planned");
    std::fs::write(
        &implemented,
        "---\nid: SPEC-9998\nfeature: fixture\nstatus: implemented\n---\n\n# fixture\n",
    )
    .expect("escribir spec implemented");

    let script = workspace_root().join("scripts").join("red_first_gate.py");
    let output = Command::new(python_interpreter())
        .current_dir(workspace_root())
        .arg(script)
        .args(["--list", "--specs"])
        .arg(&planned)
        .arg(&implemented)
        .output()
        .expect("ejecutar red_first_gate.py --list");
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "el listado debe salir 0; salida:\n{text}"
    );
    assert!(
        text.contains("planned_spec.md"),
        "el listado debe incluir el spec en curso; salida:\n{text}"
    );
    assert!(
        !text.contains("implemented_spec.md"),
        "el listado NO debe incluir specs implemented; salida:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
