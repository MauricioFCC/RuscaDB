//! Tests de aceptación de SPEC-0079 (política de cobertura multi-scope).
//!
//! Invocan `scripts/coverage_gate.py` sobre fixtures lcov versionados y
//! asientan los códigos de salida 0/1/2, además de comprobar que la política
//! quedó cableada en `ci.yml` y documentada en `docs/verification.md`.
//!
//! Intérprete: `python` con fallback a `python3`; si ninguno existe el test
//! falla ruidosamente con un mensaje WHAT+WHY+WHERE (nunca verde silencioso).

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
///
/// Returns:
///     Nombre del ejecutable de Python.
///
/// Panics:
///     Si ningún intérprete está en PATH: sin Python no se puede verificar la
///     spec AC-0079 (el gate es Python), así que el fallo debe ser ruidoso.
fn python_interpreter() -> String {
    for candidate in ["python", "python3"] {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return candidate.to_string();
        }
    }
    panic!(
        "WHAT: falta un intérprete de Python para los tests de SPEC-0079. \
         WHY: scripts/coverage_gate.py es Python y no se puede verificar sin él. \
         WHERE: instala Python 3 y asegúralo en PATH ('python' o 'python3'); \
         test en crates/xtask/tests/coverage_gate.rs."
    );
}

/// Ejecuta `scripts/coverage_gate.py` con el lcov y los flags dados.
///
/// Args:
///     lcov: Ruta del fixture lcov a analizar.
///     extra: Argumentos adicionales (`--scope`/`--min-line`/`--min-branch`).
///
/// Returns:
///     La salida del proceso (stdout/stderr/exit status).
fn run_gate(lcov: &Path, extra: &[&str]) -> Output {
    let script = workspace_root().join("scripts").join("coverage_gate.py");
    let mut command = Command::new(python_interpreter());
    command.arg(script).arg(lcov);
    for arg in extra {
        command.arg(arg);
    }
    command
        .output()
        .unwrap_or_else(|e| panic!("no se pudo ejecutar el gate de cobertura: {e}"))
}

/// Concatena stdout y stderr de la salida del gate (para asertar el mensaje).
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// AC-0079-01: con dos scopes, uno bajo su umbral, el gate sale 1 y nombra el
/// scope infractor; con ambos por encima de su umbral sale 0.
#[test]
// @spec AC-0079-01
fn test_ac_0079_01_multi_scope_gate_enforces_each_threshold() {
    let mixed = fixtures_dir().join("mixed_scopes.lcov");

    // beta (25% línea) incumple 90/90; alpha (100%) cumple 10/10 -> exit 1.
    // El scope infractor va PRIMERO: así el gate debe respetar el umbral de
    // *cada* scope (no basta con el último).
    let failing = run_gate(
        &mixed,
        &[
            "--scope",
            "crates/beta/",
            "--min-line",
            "90",
            "--min-branch",
            "90",
            "--scope",
            "crates/alpha/",
            "--min-line",
            "10",
            "--min-branch",
            "10",
        ],
    );
    let failing_text = combined(&failing);
    assert_eq!(
        failing.status.code(),
        Some(1),
        "se esperaba exit 1 con un scope bajo umbral; salida:\n{failing_text}"
    );
    assert!(
        failing_text.contains("crates/beta/"),
        "la salida debe nombrar el scope infractor 'crates/beta/'; salida:\n{failing_text}"
    );

    // Con el umbral de beta relajado (10/10) ambos cumplen -> exit 0.
    let passing = run_gate(
        &mixed,
        &[
            "--scope",
            "crates/beta/",
            "--min-line",
            "10",
            "--min-branch",
            "10",
            "--scope",
            "crates/alpha/",
            "--min-line",
            "90",
            "--min-branch",
            "90",
        ],
    );
    let passing_text = combined(&passing);
    assert_eq!(
        passing.status.code(),
        Some(0),
        "se esperaba exit 0 cuando todos los scopes cumplen; salida:\n{passing_text}"
    );
}

/// AC-0079-02: un lcov sin registros del scope pedido hace fallar el gate con
/// exit 2 (fallo ruidoso, contrato SPEC-0061), nunca verde silencioso.
#[test]
// @spec AC-0079-02
fn test_ac_0079_02_empty_scope_fails_loudly() {
    let lcov = fixtures_dir().join("all_pass.lcov");
    let output = run_gate(
        &lcov,
        &[
            "--scope",
            "crates/inexistente/",
            "--min-line",
            "80",
            "--min-branch",
            "70",
        ],
    );
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(2),
        "un scope sin datos debe salir con exit 2; salida:\n{text}"
    );
    assert!(
        text.contains("crates/inexistente/"),
        "el fallo ruidoso debe nombrar el scope sin datos; salida:\n{text}"
    );
}

/// AC-0079-03: el job `coverage` de `ci.yml` invoca el gate multi-scope
/// (core + txn + wal) y `docs/verification.md` documenta la política.
#[test]
// @spec AC-0079-03
fn test_ac_0079_03_policy_wired_in_ci_and_docs() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("no se pudo leer ci.yml");
    assert!(
        ci.contains("coverage_gate.py"),
        "ci.yml debe invocar scripts/coverage_gate.py"
    );
    for scope in [
        "--scope ruscadb-core/",
        "--scope ruscadb-txn/",
        "--scope ruscadb-wal/",
    ] {
        assert!(
            ci.contains(scope),
            "ci.yml debe incluir el scope '{scope}' en el job coverage"
        );
    }

    let docs = std::fs::read_to_string(workspace_root().join("docs/verification.md"))
        .expect("no se pudo leer docs/verification.md");
    assert!(
        docs.contains("Política de cobertura"),
        "docs/verification.md debe documentar la sección 'Política de cobertura'"
    );
    for scope in ["ruscadb-txn/", "ruscadb-wal/"] {
        assert!(
            docs.contains(scope),
            "docs/verification.md debe listar el scope '{scope}'"
        );
    }
}
