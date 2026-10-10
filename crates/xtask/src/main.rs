//! # xtask
//!
//! Tareas de desarrollo de RuscaDB ejecutadas con `cargo xtask <comando>`.
//!
//! Comandos:
//! - `trace`: verifica la trazabilidad SDD (cada AC en `specs/*.md` tiene un
//!   test existente en el workspace). Gate T1.
//! - `contract`: verifica el contrato bidireccional (K1: todo `test_ac_*`
//!   numérico con marcador `@spec`; K2: sin marcadores huérfanos; K3: toda
//!   spec `implemented` trazada). Gate T1 (SPEC-0061).
//! - `judge <diff>`: rúbricas deterministas sobre un diff con veredicto
//!   (puerta T2 v0, SPEC-0061; sin el path lee stdin).
//!
//! Sin dependencias externas (solo `std`) para mantener el bootstrap liviano.

mod judge;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "trace".to_string());
    match command.as_str() {
        "trace" => trace(),
        "contract" => contract_cmd(),
        "judge" => judge_cmd(args.next()),
        other => {
            eprintln!("comando desconocido: {other} (disponibles: trace, contract, judge)");
            ExitCode::FAILURE
        }
    }
}

/// Ejecuta el contrato de trazabilidad y reporta incumplimientos.
fn contract_cmd() -> ExitCode {
    let Some(root) = workspace_root() else {
        eprintln!("[FAIL] xtask contract: no se pudo determinar la raíz del workspace");
        return ExitCode::FAILURE;
    };
    let errors = judge::contract(&root);
    if errors.is_empty() {
        println!("[OK] xtask contract: marcadores bidireccionales y estados consistentes.");
        ExitCode::SUCCESS
    } else {
        eprintln!("[FAIL] xtask contract: {} incumplimientos:", errors.len());
        for error in &errors {
            eprintln!("  - {error}");
        }
        ExitCode::FAILURE
    }
}

/// Ejecuta el juez determinista sobre un diff (fichero o stdin).
///
/// Args:
///     source: Ruta del diff, o `None` para leer stdin.
fn judge_cmd(source: Option<String>) -> ExitCode {
    let diff = match source {
        Some(path) => match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("[FAIL] xtask judge: no se pudo leer {path}: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            use std::io::Read;
            let mut text = String::new();
            if std::io::stdin().read_to_string(&mut text).is_err() {
                eprintln!("[FAIL] xtask judge: no se pudo leer stdin");
                return ExitCode::FAILURE;
            }
            text
        }
    };
    let verdict = judge::judge_diff(&diff);
    println!(
        "[{}] xtask judge: {}",
        if verdict.passes() { "OK" } else { "FAIL" },
        judge::verdict_line(&verdict)
    );
    for finding in &verdict.findings {
        let level = match finding.level {
            judge::Level::Pass => "PASS",
            judge::Level::Warn => "WARN",
            judge::Level::Fail => "FAIL",
        };
        println!("  - [{level}] {}: {}", finding.id, finding.detail);
    }
    if verdict.passes() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
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
    //! chain), SPEC-0033 (CI cross-platform + sanitizers), SPEC-0042 (fuzzing
    //! continuo del parser RQL), SPEC-0046 (matriz rápida + macOS en nightly) y
    //! SPEC-0050 (documentación de estado y release: `CHANGELOG.md`, estado del
    //! roadmap y `docs/MVP.md`). Solo `std`: leen los artefactos de
    //! configuración/corpus/documentación como texto y verifican
    //! estructura/secciones (el gate canónico de CI es
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
    // @spec AC-0016-01
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
    // @spec AC-0016-02
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
    // @spec AC-0016-03
    fn test_ac_0016_03_deny_config_has_required_sections() {
        let toml = read_root_file("deny.toml");
        for section in ["[advisories]", "[licenses]", "[bans]", "[sources]"] {
            assert!(toml.contains(section), "deny.toml sin sección '{section}'");
        }
    }

    /// AC-0033-01: el job `test` de `ci.yml` declara la matriz OS rápida
    /// (ubuntu/windows) y corre en paralelo sin fail-fast. macOS se valida en
    /// `nightly.yml` (job `test-macos`, SPEC-0046).
    #[test]
    // @spec AC-0033-01
    fn test_ac_0033_01_ci_has_os_matrix() {
        let yaml = read_root_file(".github/workflows/ci.yml");
        assert!(
            yaml.contains("matrix:"),
            "ci.yml no declara strategy.matrix"
        );
        assert!(
            yaml.contains("${{ matrix.os }}"),
            "ci.yml no usa runs-on: ${{ matrix.os }}"
        );
        assert!(
            yaml.contains("fail-fast: false"),
            "ci.yml debe usar fail-fast: false para no cancelar evidencia"
        );
        for runner in ["ubuntu-latest", "windows-latest"] {
            assert!(
                yaml.contains(runner),
                "ci.yml no incluye el runner '{runner}' en la matriz OS"
            );
        }
        assert!(
            !yaml.contains("macos-latest"),
            "ci.yml no debe exigir macOS en cada push (SPEC-0046)"
        );
    }

    /// AC-0033-02: `nightly.yml` declara el job `sanitizers` (ASan sobre FFI,
    /// alert-only).
    #[test]
    // @spec AC-0033-02
    fn test_ac_0033_02_nightly_has_sanitizers() {
        let yaml = read_root_file(".github/workflows/nightly.yml");
        assert!(
            yaml.contains("sanitizers:"),
            "nightly.yml no declara el job 'sanitizers'"
        );
        assert!(
            yaml.contains("-Zsanitizer=address"),
            "nightly.yml no habilita AddressSanitizer (RUSTFLAGS)"
        );
        assert!(
            yaml.contains("ruscadb-ffi"),
            "nightly.yml no ejecuta ASan sobre ruscadb-ffi"
        );
        assert!(
            yaml.contains("components: rust-src"),
            "nightly.yml no instala rust-src para sanitizers"
        );
    }

    /// AC-0033-03: `check_ci_config.py` valida la matriz OS y el job
    /// `sanitizers` además de las validaciones previas.
    #[test]
    // @spec AC-0033-03
    fn test_ac_0033_03_check_ci_config_validates_matrix() {
        let script = read_root_file("scripts/check_ci_config.py");
        for needle in [
            "ci.yml",
            "matrix",
            "sanitizers",
            "ubuntu-latest",
            "windows-latest",
            "macos-latest",
        ] {
            assert!(
                script.contains(needle),
                "check_ci_config.py no valida '{needle}'"
            );
        }
    }

    /// AC-0042-01: el target `query_parse` cubre toda la gramática del lenguaje
    /// (referencia a SELECT/WHERE/AND/MATCH/KNN/TRAVERSE/EXPLAIN/ORDER BY/
    /// GROUP BY/LIMIT) e invoca las dos entradas públicas del parser.
    #[test]
    // @spec AC-0042-01
    fn test_ac_0042_01_fuzz_target_covers_grammar() {
        let target = read_root_file("fuzz/fuzz_targets/query_parse.rs");
        for clause in [
            "SELECT", "WHERE", "AND", "MATCH", "KNN", "TRAVERSE", "EXPLAIN", "ORDER BY",
            "GROUP BY", "LIMIT",
        ] {
            assert!(
                target.contains(clause),
                "el target del parser no menciona la cláusula '{clause}'"
            );
        }
        assert!(
            target.contains("parse_statement") && target.contains("parse("),
            "el target debe invocar ruscadb_query::parse_statement y ::parse"
        );
    }

    /// AC-0042-02: el corpus del parser está sembrado con al menos 5 ficheros
    /// versionables bajo `fuzz/corpus/query_parse/`.
    #[test]
    // @spec AC-0042-02
    fn test_ac_0042_02_fuzz_corpus_seeded() {
        let root = workspace_root().expect("raíz del workspace");
        let corpus = root.join("fuzz/corpus/query_parse");
        let count = fs::read_dir(&corpus)
            .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", corpus.display()))
            .flatten()
            .filter(|entry| entry.path().is_file())
            .count();
        assert!(
            count >= 5,
            "el corpus debe tener >= 5 semillas versionadas, tiene {count}"
        );
    }

    /// AC-0042-03: `nightly.yml` declara el job `fuzz` (alert-only) con la
    /// matriz de targets del parser y el dictionary de RQL para `query_parse`.
    #[test]
    // @spec AC-0042-03
    fn test_ac_0042_03_nightly_fuzz_job() {
        let yaml = read_root_file(".github/workflows/nightly.yml");
        assert!(yaml.contains("fuzz:"), "nightly.yml sin job 'fuzz'");
        assert!(
            yaml.contains("query_parse") && yaml.contains("wal_recover"),
            "nightly.yml: la matriz de fuzz debe incluir query_parse y wal_recover"
        );
        assert!(
            yaml.contains("continue-on-error: true"),
            "el job 'fuzz' debe ser alert-only (continue-on-error: true)"
        );
        assert!(
            yaml.contains("-max_total_time=300"),
            "el job 'fuzz' debe acotar la corrida a 300 s"
        );
        assert!(
            yaml.contains("-dict=query_parse.dict"),
            "el target query_parse debe usar el dictionary de tokens de RQL"
        );
    }

    /// AC-0042-04: el target del parser mantiene el contrato no-panic (sin
    /// `unwrap(`/`expect(` sobre el resultado del parser).
    #[test]
    // @spec AC-0042-04
    fn test_ac_0042_04_fuzz_target_is_panic_free() {
        let target = read_root_file("fuzz/fuzz_targets/query_parse.rs");
        for forbidden in ["unwrap(", "expect("] {
            assert!(
                !target.contains(forbidden),
                "el target del parser no debe contener '{forbidden}' \
                 (contrato no-panic)"
            );
        }
    }

    /// AC-0046-01: la matriz del job `test` de `ci.yml` es rápida
    /// (ubuntu+windows) y no exige macOS en cada push.
    #[test]
    // @spec AC-0046-01
    fn test_ac_0046_01_ci_matrix_fast() {
        let yaml = read_root_file(".github/workflows/ci.yml");
        assert!(
            yaml.contains("matrix:"),
            "ci.yml no declara strategy.matrix"
        );
        for runner in ["ubuntu-latest", "windows-latest"] {
            assert!(
                yaml.contains(runner),
                "ci.yml no incluye el runner '{runner}' en la matriz del job test"
            );
        }
        assert!(
            !yaml.contains("macos-latest"),
            "ci.yml no debe exigir macOS en cada push (SPEC-0046)"
        );
    }

    /// AC-0046-02: `nightly.yml` declara un job que corre los tests en macOS
    /// (schedule, alert-only).
    #[test]
    // @spec AC-0046-02
    fn test_ac_0046_02_nightly_has_macos() {
        let yaml = read_root_file(".github/workflows/nightly.yml");
        assert!(
            yaml.contains("test-macos:"),
            "nightly.yml no declara el job 'test-macos'"
        );
        assert!(
            yaml.contains("runs-on: macos-latest"),
            "nightly.yml no corre ningún job en 'macos-latest'"
        );
        assert!(
            yaml.contains("cargo test --workspace --all-features"),
            "el job de macOS debe ejecutar cargo test --workspace --all-features"
        );
        assert!(
            yaml.contains("continue-on-error: true"),
            "el job de macOS debe ser alert-only (continue-on-error: true)"
        );
    }

    /// AC-0046-03: `check_ci_config.py` valida la matriz rápida (ubuntu +
    /// windows) y el job macOS de nightly.
    #[test]
    // @spec AC-0046-03
    fn test_ac_0046_03_check_ci_config_validates_runners() {
        let script = read_root_file("scripts/check_ci_config.py");
        for needle in [
            "ci.yml",
            "ubuntu-latest",
            "windows-latest",
            "macos-latest",
            "nightly.yml",
        ] {
            assert!(
                script.contains(needle),
                "check_ci_config.py no valida '{needle}'"
            );
        }
    }

    /// AC-0050-01: `CHANGELOG.md` existe en la raíz, sigue el formato Keep a
    /// Changelog (`[Unreleased]` + Added/Changed/Fixed), lista las fases F0–F6
    /// y las specs, y no inventa releases publicadas (`[0.1.0] - no publicado`).
    #[test]
    // @spec AC-0050-01
    fn test_ac_0050_01_changelog_exists() {
        let changelog = read_root_file("CHANGELOG.md");
        assert!(
            changelog.contains("Keep a Changelog"),
            "CHANGELOG.md debe declarar el formato Keep a Changelog"
        );
        for section in ["## [Unreleased]", "### Added", "### Changed", "### Fixed"] {
            assert!(
                changelog.contains(section),
                "CHANGELOG.md sin la sección '{section}'"
            );
        }
        assert!(
            changelog.contains("[0.1.0] - no publicado"),
            "CHANGELOG.md debe declarar [0.1.0] - no publicado (sin inventar releases)"
        );
        for phase in ["F0", "F1", "F2", "F3", "F4", "F5", "F6"] {
            assert!(
                changelog.contains(phase),
                "CHANGELOG.md no menciona la fase '{phase}'"
            );
        }
        assert!(
            changelog.contains("SPEC-"),
            "CHANGELOG.md debe listar las specs implementadas"
        );
    }

    /// AC-0050-02: el roadmap incluye la sección 'Estado de implementación' con
    /// una tabla área/estado/evidencia y documenta el desvío de DataFusion
    /// (ADR-001) como pendiente.
    #[test]
    // @spec AC-0050-02
    fn test_ac_0050_02_roadmap_status() {
        let roadmap = read_root_file("docs/RuscaDB-roadmap.md");
        assert!(
            roadmap.contains("## Estado de implementación"),
            "el roadmap no incluye la sección 'Estado de implementación'"
        );
        assert!(
            roadmap.contains("| Área | Estado | Evidencia"),
            "la sección de estado debe ser una tabla área/estado/evidencia"
        );
        assert!(
            roadmap.contains("DataFusion") && roadmap.contains("ADR-001"),
            "la sección de estado debe documentar el desvío de DataFusion (ADR-001)"
        );
        assert!(
            roadmap.contains("⏳"),
            "la sección de estado debe marcar lo pendiente con ⏳"
        );
    }

    /// AC-0050-03: `docs/MVP.md` documenta las capacidades y límites
    /// actualizados: DML (`INSERT`/`UPDATE`/`DELETE`), `GROUP BY`, `ORDER BY` y
    /// blobs integrados.
    #[test]
    // @spec AC-0050-03
    fn test_ac_0050_03_mvp_updated() {
        let mvp = read_root_file("docs/MVP.md");
        for capability in ["INSERT", "UPDATE", "DELETE", "GROUP BY", "ORDER BY", "blob"] {
            assert!(
                mvp.contains(capability),
                "docs/MVP.md no menciona la capacidad '{capability}'"
            );
        }
    }

    /// AC-0061-01: el contrato exige marcador `@spec` en todo `test_ac_*`
    /// numérico y rechaza marcadores huérfanos (K1/K2 sobre el workspace real).
    #[test]
    // @spec AC-0061-01
    fn test_ac_0061_01_contract_markers_bidirectional() {
        let Some(root) = super::workspace_root() else {
            panic!("sin raíz del workspace");
        };
        let errors = super::judge::contract(&root);
        let marker_errors: Vec<&String> = errors
            .iter()
            .filter(|e| e.starts_with("K1") || e.starts_with("K2"))
            .collect();
        assert!(
            marker_errors.is_empty(),
            "incumplimientos K1/K2: {marker_errors:?}"
        );
    }

    /// AC-0061-02: toda spec `implemented` tiene sus AC trazados (K3 real).
    #[test]
    // @spec AC-0061-02
    fn test_ac_0061_02_implemented_specs_fully_traced() {
        let Some(root) = super::workspace_root() else {
            panic!("sin raíz del workspace");
        };
        let errors = super::judge::contract(&root);
        let state_errors: Vec<&String> = errors.iter().filter(|e| e.starts_with("K3")).collect();
        assert!(
            state_errors.is_empty(),
            "specs implemented sin trazar: {state_errors:?}"
        );
    }

    /// AC-0061-05: `ci.yml` declara el job `mutation-diff` (solo PR,
    /// `--in-diff`, gate con `--min-score 70`) y `check_ci_config.py` lo
    /// valida (FR-0061-05).
    #[test]
    // @spec AC-0061-05
    fn test_ac_0061_05_mutation_diff_job() {
        let ci = read_root_file(".github/workflows/ci.yml");
        for needle in [
            "mutation-diff:",
            "github.event_name == 'pull_request'",
            "--in-diff",
            "mutation_diff_gate.py",
            "--min-score 70",
        ] {
            assert!(
                ci.contains(needle),
                "ci.yml sin el cableado mutation-diff ('{needle}')"
            );
        }
        let guard = read_root_file("scripts/check_ci_config.py");
        assert!(
            guard.contains("check_mutation_diff_job"),
            "check_ci_config.py no valida el job mutation-diff"
        );
        let script = read_root_file("scripts/mutation_diff_gate.py");
        assert!(
            script.contains("min-score") && script.contains("min_score"),
            "mutation_diff_gate.py sin umbral configurable"
        );
    }

    /// AC-0061-06: `ci.yml` declara el job `coverage` (llvm-cov + lcov +
    /// gate 80/70 del core) y `check_ci_config.py` lo valida (FR-0061-06).
    #[test]
    // @spec AC-0061-06
    fn test_ac_0061_06_coverage_job() {
        let ci = read_root_file(".github/workflows/ci.yml");
        for needle in [
            "coverage:",
            "cargo llvm-cov",
            "--lcov",
            "coverage_gate.py",
            "--min-line 80 --min-branch 70",
        ] {
            assert!(
                ci.contains(needle),
                "ci.yml sin el cableado coverage ('{needle}')"
            );
        }
        let guard = read_root_file("scripts/check_ci_config.py");
        assert!(
            guard.contains("check_coverage_job"),
            "check_ci_config.py no valida el job coverage"
        );
        let script = read_root_file("scripts/coverage_gate.py");
        assert!(
            script.contains("min-line") && script.contains("min_line"),
            "coverage_gate.py sin umbrales configurables"
        );
    }
}
