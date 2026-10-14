//! Tests de aceptación de SPEC-0089 (automatización de releases/tags y
//! validación de CHANGELOG/commits).
//!
//! Invocan `scripts/check_commits.py` y `scripts/check_changelog.py` con
//! entradas temporales y asientan los códigos de salida 0/1/2, además de
//! comprobar el cableado en `.github/workflows/{ci,release}.yml` y en
//! `scripts/check_ci_config.py`.
//!
//! Intérprete: `python` con fallback a `python3`; sin intérprete el test falla
//! ruidosamente (nunca verde silencioso).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Raíz del workspace: dos niveles sobre `crates/xtask`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Devuelve el intérprete de Python disponible (`python` o `python3`).
///
/// Panics:
///     Si ningún intérprete está en PATH: sin Python no se puede verificar la
///     spec AC-0089 (los validadores son Python), así que el fallo es ruidoso.
fn python_interpreter() -> String {
    for candidate in ["python", "python3"] {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return candidate.to_string();
        }
    }
    panic!(
        "WHAT: falta un intérprete de Python para los tests de SPEC-0089. \
         WHY: scripts/check_commits.py y check_changelog.py son Python. \
         WHERE: instala Python 3 y asegúralo en PATH ('python' o 'python3'); \
         test en crates/xtask/tests/release_automation.rs."
    );
}

/// Ejecuta un script de `scripts/` con los argumentos dados.
fn run_script(script: &str, args: &[&str]) -> Output {
    let path = workspace_root().join("scripts").join(script);
    let mut command = Command::new(python_interpreter());
    command.arg(path);
    for arg in args {
        command.arg(arg);
    }
    command
        .output()
        .unwrap_or_else(|e| panic!("no se pudo ejecutar {script}: {e}"))
}

/// Concatena stdout y stderr de una salida (para asertar el mensaje).
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// Crea (limpio) un directorio temporal único para el test.
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ruscadb-release-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("no se pudo crear {}: {error}", dir.display()));
    dir
}

/// AC-0089-01: el linter de commits acepta Conventional Commits válidos (exit
/// 0), rechaza inválidos nombrando el infractor (exit 1) y falla ruidoso sin
/// entradas (exit 2).
#[test]
// @spec AC-0089-01
fn test_ac_0089_01_commit_lint_accepts_and_rejects() {
    let dir = temp_dir("commits");

    let valid = dir.join("valid.txt");
    std::fs::write(
        &valid,
        "feat(db): soporte KNN\ndocs: aclarar la guía de API\nMerge branch 'main'\n",
    )
    .expect("escribir valid.txt");
    let output = run_script("check_commits.py", &["--file", valid.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "un rango válido debe salir con exit 0; salida:\n{}",
        combined(&output)
    );

    let invalid = dir.join("invalid.txt");
    std::fs::write(&invalid, "wip: no permitido\nfeat(db): esto sí\n")
        .expect("escribir invalid.txt");
    let output = run_script("check_commits.py", &["--file", invalid.to_str().unwrap()]);
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(1),
        "un mensaje inválido debe salir con exit 1; salida:\n{text}"
    );
    assert!(
        text.contains("wip"),
        "la salida debe nombrar el commit infractor; salida:\n{text}"
    );

    let empty = dir.join("empty.txt");
    std::fs::write(&empty, "").expect("escribir empty.txt");
    let output = run_script("check_commits.py", &["--file", empty.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "sin mensajes que revisar el gate debe salir con exit 2; salida:\n{}",
        combined(&output)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// AC-0089-02: el gate de CHANGELOG exige la sección de la versión del tag
/// (exit 0/1) y falla ruidoso si el fichero no existe (exit 2).
#[test]
// @spec AC-0089-02
fn test_ac_0089_02_changelog_gate_enforces_version() {
    let changelog = workspace_root().join("CHANGELOG.md");
    let changelog_arg = changelog.to_str().unwrap();

    let output = run_script("check_changelog.py", &[changelog_arg]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "el CHANGELOG real debe pasar; salida:\n{}",
        combined(&output)
    );

    let output = run_script("check_changelog.py", &[changelog_arg, "--tag", "v0.1.0"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "el tag documentado [0.1.0] debe pasar; salida:\n{}",
        combined(&output)
    );

    let output = run_script("check_changelog.py", &[changelog_arg, "--tag", "v9.9.9"]);
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(1),
        "un tag sin sección debe salir con exit 1; salida:\n{text}"
    );
    assert!(
        text.contains("9.9.9"),
        "la salida debe nombrar la versión que falta; salida:\n{text}"
    );

    let missing = workspace_root().join("no-existe-changelog.md");
    let output = run_script("check_changelog.py", &[missing.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "un CHANGELOG inexistente debe salir con exit 2; salida:\n{}",
        combined(&output)
    );
}

/// AC-0089-03: cableado real — job `commits` (solo PR) en `ci.yml`,
/// `release.yml` en tags `v*` con `contents: write` y el SBOM, y el guard
/// `check_ci_config.py` validándolo.
#[test]
// @spec AC-0089-03
fn test_ac_0089_03_release_wiring_in_ci_and_workflow() {
    let ci = std::fs::read_to_string(workspace_root().join(".github/workflows/ci.yml"))
        .expect("no se pudo leer ci.yml");
    assert!(
        ci.contains("commits:"),
        "ci.yml debe declarar el job 'commits'"
    );
    for needle in ["check_commits.py", "check_changelog.py", "pull_request"] {
        assert!(
            ci.contains(needle),
            "ci.yml: el job 'commits' debe cablear '{needle}'"
        );
    }

    let release = std::fs::read_to_string(workspace_root().join(".github/workflows/release.yml"))
        .expect("no se pudo leer release.yml");
    for needle in ["tags:", "contents: write", "check_changelog.py", "v*"] {
        assert!(release.contains(needle), "release.yml sin '{needle}'");
    }

    let guard = std::fs::read_to_string(workspace_root().join("scripts/check_ci_config.py"))
        .expect("no se pudo leer check_ci_config.py");
    assert!(
        guard.contains("check_release_config"),
        "check_ci_config.py no valida el cableado de release"
    );
}
