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
