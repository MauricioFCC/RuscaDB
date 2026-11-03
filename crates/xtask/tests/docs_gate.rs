//! Tests de aceptación de SPEC-0199 (calidad de documentación).
//!
//! Invocan `scripts/check_docs.py` sobre el repo real y sobre fixtures
//! temporales para asentar los códigos de salida y los mensajes accionables.

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
    panic!("WHAT: falta Python para verificar SPEC-0199. WHERE: crates/xtask/tests/docs_gate.rs.");
}

/// Ejecuta el gate de documentación sobre `root`.
fn run_gate(root: &Path) -> Output {
    let script = workspace_root().join("scripts").join("check_docs.py");
    Command::new(python_interpreter())
        .arg(script)
        .arg("--root")
        .arg(root)
        .output()
        .unwrap_or_else(|error| panic!("no se pudo ejecutar check_docs.py: {error}"))
}

/// Concatena stdout y stderr.
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// AC-0199-01: la documentación real pasa el gate (enlaces, rutas, fences e
/// índice completos).
#[test]
// @spec AC-0199-01
fn test_ac_0199_01_docs_gate_repo_clean() {
    let output = run_gate(&workspace_root());
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "la documentación del repo debe pasar el gate; salida:\n{text}"
    );
    assert!(text.contains("[OK] docs:"), "{text}");
}

/// AC-0199-02: un enlace roto, una ruta personal o un fence desbalanceado
/// fallan con exit 1 y mensaje accionable (nunca verde silencioso).
#[test]
// @spec AC-0199-02
fn test_ac_0199_02_docs_gate_rejects_issues() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for (path, content) in [
        ("README.md", "# README\n"),
        ("AGENTS.md", "# AGENTS\n"),
        ("CHANGELOG.md", "# CHANGELOG\n"),
        (
            "docs/README.md",
            "# Índice\n\n[good](good.md)\n[bad](bad.md)\n[verification](verification.md)\n[specs](specs-index.md)\n",
        ),
        ("docs/verification.md", "# Verificación\n"),
        ("docs/specs-index.md", "# Specs\n"),
        ("docs/good.md", "# Bueno\n\n[ok](verification.md)\n"),
        (
            "docs/bad.md",
            "# Malo\n\n[roto](missing.md)\n[ancla](good.md#no-existe)\nC:\\Users\\alice\\db.rs\n```text\n",
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }

    let output = run_gate(root);
    let text = combined(&output);
    assert_eq!(output.status.code(), Some(1), "se esperaba exit 1:\n{text}");
    for needle in [
        "enlace roto",
        "ancla rota",
        "ruta Windows",
        "desbalanceados",
    ] {
        assert!(text.contains(needle), "falta '{needle}' en:\n{text}");
    }
}

/// AC-0199-03: el gate está cableado en CI y en el Gate T1 de AGENTS.md.
#[test]
// @spec AC-0199-03
fn test_ac_0199_03_docs_gate_wired() {
    let ci =
        std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml")).expect("ci.yml");
    assert!(
        ci.contains("scripts/check_docs.py"),
        "ci.yml debe ejecutar scripts/check_docs.py"
    );
    let agents = std::fs::read_to_string(workspace_root().join("AGENTS.md")).expect("AGENTS.md");
    assert!(
        agents.contains("scripts/check_docs.py"),
        "AGENTS.md debe incluir check_docs.py en el Gate T1"
    );
}
