//! Tests de aceptación de SPEC-0092 (reporte explícito de ramas BRDA).
//!
//! Invocan `scripts/branch_coverage_report.py` sobre el fixture lcov versionado
//! y asientan el gate (exit 0/1) y el volcado JSON por crate; además comprueban
//! que el reporte quedó cableado en el job `coverage` de `ci.yml` y validado por
//! `scripts/check_ci_config.py`. Solo `std`.
//!
//! Intérprete: `python` con fallback a `python3`; si ninguno existe el test
//! falla ruidosamente (nunca verde silencioso).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Raíz del workspace: dos niveles sobre `crates/xtask`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Directorio de fixtures lcov de este test.
fn fixtures_dir() -> PathBuf {
    workspace_root()
        .join("crates")
        .join("xtask")
        .join("tests")
        .join("fixtures")
        .join("coverage")
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
        "WHAT: falta un intérprete de Python para los tests de SPEC-0092. \
         WHY: scripts/branch_coverage_report.py es Python y no se puede \
         verificar sin él. WHERE: instala Python 3 en PATH ('python' o \
         'python3'); test en crates/xtask/tests/branch_coverage.rs."
    );
}

/// Ejecuta `scripts/branch_coverage_report.py` con el lcov y los flags dados.
fn run_report(lcov: &Path, extra: &[&str]) -> Output {
    let script = workspace_root()
        .join("scripts")
        .join("branch_coverage_report.py");
    let mut command = Command::new(python_interpreter());
    command.arg(script).arg(lcov);
    for arg in extra {
        command.arg(arg);
    }
    command
        .output()
        .unwrap_or_else(|e| panic!("no se pudo ejecutar el reporte de ramas: {e}"))
}

/// Concatena stdout y stderr de la salida (para asertar mensajes).
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// Ruta temporal única para el artefacto JSON del test.
fn temp_json(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ruscadb_branch_coverage_{tag}_{}.json",
        std::process::id()
    ))
}

/// AC-0092-01: el reporte cita la rama por scope/crate, respeta `--min-branch`
/// (exit 1 bajo umbral, exit 0 al relajarlo) y vuelca el detalle a JSON.
#[test]
// @spec AC-0092-01
fn test_ac_0092_01_branch_report_gates_and_emits_json() {
    let mixed = fixtures_dir().join("mixed_scopes.lcov");

    let failing = run_report(
        &mixed,
        &[
            "--scope",
            "crates/beta/",
            "--min-branch",
            "70",
            "--scope",
            "crates/alpha/",
            "--min-branch",
            "10",
        ],
    );
    let failing_text = combined(&failing);
    assert_eq!(
        failing.status.code(),
        Some(1),
        "beta (rama 50%) bajo el umbral 70 debe salir 1; salida:\n{failing_text}"
    );
    assert!(
        failing_text.contains("crates/beta/") && failing_text.contains("50.0%"),
        "el reporte debe citar el scope infractor y su % de rama; salida:\n{failing_text}"
    );

    let json_path = temp_json("gate");
    let passing = run_report(
        &mixed,
        &[
            "--scope",
            "crates/beta/",
            "--min-branch",
            "10",
            "--scope",
            "crates/alpha/",
            "--min-branch",
            "10",
            "--json-out",
            json_path.to_str().expect("ruta utf-8"),
        ],
    );
    let passing_text = combined(&passing);
    assert_eq!(
        passing.status.code(),
        Some(0),
        "con ambos scopes sobre su umbral debe salir 0; salida:\n{passing_text}"
    );

    let raw = std::fs::read_to_string(&json_path).expect("el reporte JSON debe existir");
    assert!(
        raw.contains("\"ok\": true"),
        "el JSON debe reflejar el veredicto global; contenido:\n{raw}"
    );
    assert!(
        raw.contains("\"scope\": \"crates/beta/\"") && raw.contains("\"branch_percent\": 50.0"),
        "el JSON debe incluir beta con 1/2 ramas (50.0%); contenido:\n{raw}"
    );
    assert!(
        raw.contains("\"crate\": \"beta\"") && raw.contains("\"branch_hit\": 1"),
        "el JSON debe desglosar el crate beta con sus ramas cubiertas; contenido:\n{raw}"
    );
    let _ = std::fs::remove_file(&json_path);
}

/// AC-0092-02: el job `coverage` de `ci.yml` publica el reporte y el guard lo
/// valida (`check_branch_report_job`).
#[test]
// @spec AC-0092-02
fn test_ac_0092_02_report_wired_in_ci_and_guard() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("no se pudo leer ci.yml");
    assert!(
        ci.contains("branch_coverage_report.py"),
        "ci.yml debe invocar scripts/branch_coverage_report.py en el job coverage"
    );
    assert!(
        ci.contains("--json-out"),
        "el reporte de ramas debe volcarse a JSON (--json-out)"
    );
    assert!(
        ci.contains("branch-coverage-report"),
        "ci.yml debe subir el artefacto 'branch-coverage-report'"
    );

    let guard = std::fs::read_to_string(workspace_root().join("scripts/check_ci_config.py"))
        .expect("no se pudo leer check_ci_config.py");
    assert!(
        guard.contains("check_branch_report_job"),
        "check_ci_config.py debe validar el reporte de ramas"
    );
}
