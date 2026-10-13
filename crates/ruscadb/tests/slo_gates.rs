//! Tests de aceptación de SPEC-0082 (gates estadísticos SLO).
//!
//! Invocan `scripts/slo_gate.py` sobre los fixtures JSON versionados en
//! `benches/baselines/slo/` y asientan los códigos de salida 0/1/2 del
//! contrato SPEC-0079 (regresión significativa / pasa / fallo ruidoso).
//!
//! Intérprete: `python` con fallback a `python3`; si ninguno existe el test
//! falla ruidosamente con un mensaje WHAT+WHY+WHERE (nunca verde silencioso).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Raíz del workspace: dos niveles sobre `crates/ruscadb`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Directorio de fixtures JSON del gate estadístico.
fn fixtures_dir() -> PathBuf {
    workspace_root()
        .join("benches")
        .join("baselines")
        .join("slo")
}

/// Devuelve el intérprete de Python disponible (`python` o `python3`).
///
/// Returns:
///     Nombre del ejecutable de Python.
///
/// Panics:
///     Si ningún intérprete está en PATH: sin Python no se puede verificar la
///     spec AC-0082 (el gate es Python), así que el fallo debe ser ruidoso.
fn python_interpreter() -> String {
    for candidate in ["python", "python3"] {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return candidate.to_string();
        }
    }
    panic!(
        "WHAT: falta un intérprete de Python para los tests de SPEC-0082. \
         WHY: scripts/slo_gate.py es Python y no se puede verificar sin él. \
         WHERE: instala Python 3 y asegúralo en PATH ('python' o 'python3'); \
         test en crates/ruscadb/tests/slo_gates.rs."
    );
}

/// Ejecuta `scripts/slo_gate.py` sobre un fixture dado.
///
/// Args:
///     fixture: Nombre del fixture JSON dentro de `benches/baselines/slo/`.
///
/// Returns:
///     La salida del proceso (stdout/stderr/exit status).
fn run_gate(fixture: &str) -> Output {
    let script = workspace_root().join("scripts").join("slo_gate.py");
    let path = fixtures_dir().join(fixture);
    Command::new(python_interpreter())
        .arg(script)
        .arg(&path)
        .output()
        .unwrap_or_else(|e| panic!("no se pudo ejecutar slo_gate.py: {e}"))
}

/// Concatena stdout y stderr de la salida del gate (para asertar el mensaje).
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// AC-0082-01: Mann-Whitney señala una regresión de latencia con
/// p < 0.002 (exit 1 nombrando la métrica) y aprueba muestras equivalentes
/// (exit 0).
#[test]
// @spec AC-0082-01
fn test_ac_0082_01_mann_whitney_flags_significant_regression() {
    let regresion = run_gate("latency_regression.json");
    let texto = combined(&regresion);
    assert_eq!(
        regresion.status.code(),
        Some(1),
        "latencia claramente peor debe salir con exit 1; salida:\n{texto}"
    );
    assert!(
        texto.contains("wal_commit_p50_ms"),
        "la salida debe nombrar la métrica regresionada; salida:\n{texto}"
    );

    let equivalente = run_gate("latency_same.json");
    let texto_ok = combined(&equivalente);
    assert_eq!(
        equivalente.status.code(),
        Some(0),
        "muestras equivalentes no deben fallar (falso positivo); salida:\n{texto_ok}"
    );
}

/// AC-0082-02: el bootstrap pareado gatea el delta de recall por
/// significancia estadística: delta real => exit 1; solo ruido => exit 0.
#[test]
// @spec AC-0082-02
fn test_ac_0082_02_bootstrap_paired_recall_gates_delta() {
    let delta = run_gate("recall_delta.json");
    let texto = combined(&delta);
    assert_eq!(
        delta.status.code(),
        Some(1),
        "delta de recall significativo debe salir con exit 1; salida:\n{texto}"
    );
    assert!(
        texto.contains("recall_at_10"),
        "la salida debe nombrar la métrica de recall; salida:\n{texto}"
    );

    let ruido = run_gate("recall_noise.json");
    let texto_ok = combined(&ruido);
    assert_eq!(
        ruido.status.code(),
        Some(0),
        "delta dentro del ruido no debe fallar (noise floor); salida:\n{texto_ok}"
    );
}

/// AC-0082-03: la calibración A/A pasa sin falsos positivos y un fixture
/// malformado sale con exit 2 (fallo ruidoso, nunca verde silencioso).
#[test]
// @spec AC-0082-03
fn test_ac_0082_03_aa_calibration_and_loud_failure() {
    let aa = run_gate("aa.json");
    let texto = combined(&aa);
    assert_eq!(
        aa.status.code(),
        Some(0),
        "A/A (misma distribución) debe pasar el gate; salida:\n{texto}"
    );

    let malformado = run_gate("malformed.json");
    let texto_err = combined(&malformado);
    assert_eq!(
        malformado.status.code(),
        Some(2),
        "fixture malformado debe salir con exit 2; salida:\n{texto_err}"
    );
    assert!(
        texto_err.contains("malformed.json"),
        "el fallo ruidoso debe nombrar el fixture; salida:\n{texto_err}"
    );
}

/// Cableado del gate: `slo.yml` invoca el script en PRs y
/// `docs/verification.md` documenta cómo correrlo local (FR-0082-04,
/// NF-0082-03). Sin marcador AC: complementa a los tres AC anteriores.
#[test]
fn test_gate_wired_in_workflow_and_docs() {
    let workflow = std::fs::read_to_string(workspace_root().join(".github/workflows/slo.yml"))
        .expect("no se pudo leer slo.yml (FR-0082-04: falta el workflow SLO)");
    assert!(
        workflow.contains("slo_gate.py"),
        "slo.yml debe invocar scripts/slo_gate.py"
    );
    assert!(
        workflow.contains("pull_request"),
        "slo.yml debe correr en pull_request (FR-0082-04)"
    );

    let docs = std::fs::read_to_string(workspace_root().join("docs/verification.md"))
        .expect("no se pudo leer docs/verification.md");
    assert!(
        docs.contains("Gates estadísticos SLO"),
        "docs/verification.md debe documentar la sección 'Gates estadísticos SLO'"
    );
}
