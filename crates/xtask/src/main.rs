//! # xtask
//!
//! Tareas de desarrollo de RuscaDB ejecutadas con `cargo xtask <comando>`.
//!
//! Comandos:
//! - `trace`: verifica la trazabilidad SDD (cada AC en `specs/*.md` tiene un
//!   test existente en el workspace). Gate T1.
//!
//! Sin dependencias externas (solo `std`) para mantener el bootstrap liviano.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "trace".to_string());
    match command.as_str() {
        "trace" => trace(),
        other => {
            eprintln!("comando desconocido: {other} (disponible: trace)");
            ExitCode::FAILURE
        }
    }
}

/// Raíz del workspace (dos niveles sobre `crates/xtask`).
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

/// Verifica que cada AC de `specs/*.md` referencia un test existente.
fn trace() -> ExitCode {
    let Some(root) = workspace_root() else {
        eprintln!("[FAIL] xtask trace: no se pudo determinar la raíz del workspace");
        return ExitCode::FAILURE;
    };
    let known_tests = collect_test_names(&root.join("crates"));
    let specs = read_markdown_files(&root.join("specs"));

    let mut total = 0usize;
    let mut missing: Vec<(PathBuf, String)> = Vec::new();
    for spec in &specs {
        let Ok(text) = fs::read_to_string(spec) else {
            continue;
        };
        for name in extract_test_names(&text) {
            total += 1;
            if !known_tests.contains(&name) {
                missing.push((spec.clone(), name));
            }
        }
    }

    if missing.is_empty() {
        println!("[OK] xtask trace: {total} AC trazados a tests existentes.");
        ExitCode::SUCCESS
    } else {
        eprintln!("[FAIL] xtask trace: {} AC sin test:", missing.len());
        for (spec, name) in &missing {
            eprintln!("  - {} -> {name}", spec.display());
        }
        ExitCode::FAILURE
    }
}

/// Extrae los nombres de test declarados en `test: <nombre>` de una spec.
fn extract_test_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let index = line.find("test:")?;
            let rest = line[index + "test:".len()..].trim();
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

/// Lista los `*.md` de un directorio (no recursivo).
fn read_markdown_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect()
}

/// Recolecta todos los nombres de función (`fn <name>`) del código Rust.
fn collect_test_names(root: &Path) -> HashSet<String> {
    let mut names = HashSet::new();
    walk(root, &mut |path| {
        if path.extension().is_none_or(|ext| ext != "rs") {
            return;
        }
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        for line in text.lines() {
            let Some(position) = line.find("fn ") else {
                continue;
            };
            let name: String = line[position + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                names.insert(name);
            }
        }
    });
    names
}

/// Recorre recursivamente un directorio invocando `visit` por archivo.
fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests de aceptación de SPEC-0016 (endurecimiento de CI T3 y supply
    //! chain). Solo `std`: leen los artefactos de configuración como texto y
    //! verifican estructura/secciones (el gate canónico es
    //! `scripts/check_ci_config.py`).

    use super::workspace_root;
    use std::fs;

    /// Lee un archivo relativo a la raíz del workspace.
    fn read_root_file(relative: &str) -> String {
        let root = workspace_root().expect("raíz del workspace");
        let path = root.join(relative);
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("no se pudo leer {relative}: {e}"))
    }

    /// AC-0016-01: el workflow nightly es válido y declara mutation/fuzz/miri.
    #[test]
    fn test_ac_0016_01_nightly_workflow_is_valid() {
        let yaml = read_root_file(".github/workflows/nightly.yml");
        for trigger in ["schedule:", "workflow_dispatch:"] {
            assert!(
                yaml.contains(trigger),
                "nightly.yml sin trigger '{trigger}'"
            );
        }
        for job in ["mutation:", "fuzz:", "miri:"] {
            assert!(yaml.contains(job), "nightly.yml sin job '{job}'");
        }
        assert!(
            yaml.contains("continue-on-error: true"),
            "nightly.yml debe ser alert-only (continue-on-error)"
        );
        assert!(
            yaml.contains("--shard="),
            "nightly.yml debe ejecutar mutation sharded"
        );
        assert!(
            yaml.contains("-max_total_time=300"),
            "nightly.yml debe acotar el fuzzing a 300 s"
        );
        assert!(
            yaml.contains("dtolnay/rust-toolchain@nightly") && yaml.contains("miri"),
            "nightly.yml debe correr miri con el toolchain nightly"
        );
    }

    /// AC-0016-02: la config de cargo-mutants define exclude y toolchain (vía
    /// pin en `rust-toolchain.toml`) sin romper T1 (`deny_unknown_fields`).
    #[test]
    fn test_ac_0016_02_mutants_config_is_valid() {
        let toml = read_root_file(".cargo/mutants.toml");
        for key in ["exclude_globs", "test_tool", "additional_cargo_test_args"] {
            assert!(toml.contains(key), "mutants.toml sin clave '{key}'");
        }
        assert!(
            toml.contains("toolchain"),
            "mutants.toml debe documentar la política de toolchain"
        );
        // cargo-mutants usa `deny_unknown_fields`: una clave `toolchain` real
        // rompería el parseo. Solo se admite como documentación (comentario).
        for line in toml.lines() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("toolchain") || trimmed.starts_with('#'),
                "mutants.toml declara la clave no soportada 'toolchain'"
            );
        }
    }

    /// AC-0016-03: `deny.toml` declara las 4 secciones requeridas.
    #[test]
    fn test_ac_0016_03_deny_config_has_required_sections() {
        let toml = read_root_file("deny.toml");
        for section in ["[advisories]", "[licenses]", "[bans]", "[sources]"] {
            assert!(toml.contains(section), "deny.toml sin sección '{section}'");
        }
    }
}
