//! Puerta T2 v0: contrato de trazabilidad + juez determinista (SPEC-0061).
//!
//! Dos comandos:
//! - `contract`: trazabilidad bidireccional (K1: todo `test_ac_*` numérico con
//!   marcador `@spec` cercano; K2: todo marcador resuelve a un AC existente;
//!   K3: toda spec `implemented` tiene sus AC trazados).
//! - `judge`: rúbricas deterministas sobre un diff (J1 marcador ausente en
//!   tests nuevos → fail; J2 `unsafe` fuera de `ruscadb-ffi` → fail; J3
//!   `unwrap`/`expect` añadidos en `src/` → warn, nunca fail).
//!
//! Más validación del juez (κ + false-pass/false-fail) para calibrar el futuro
//! backend LLM (curso AI Evals W2). Solo `std`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Nivel de un hallazgo del juez.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Pasa (informativo).
    Pass,
    /// Aviso: no bloquea (heurística con posibles falsos positivos).
    Warn,
    /// Bloquea el veredicto.
    Fail,
}

/// Hallazgo de una rúbrica.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Identificador de la rúbrica (p. ej. `J1-missing-marker`).
    pub id: &'static str,
    /// Nivel del hallazgo.
    pub level: Level,
    /// Detalle accionable (fichero:línea + motivo).
    pub detail: String,
}

/// Veredicto agregado de las rúbricas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Hallazgos por rúbrica.
    pub findings: Vec<Finding>,
}

impl Verdict {
    /// `true` si ningún hallazgo es `Fail`.
    pub fn passes(&self) -> bool {
        self.findings.iter().all(|f| f.level != Level::Fail)
    }
}

/// Extrae los ids `AC-XXXX-NN` de una spec junto a su `status`.
///
/// Args:
///     text: Contenido de `specs/<feature>.md`.
///
/// Returns:
///     `(status, ids)`: estado declarado y ACs encontrados.
pub fn specacs(text: &str) -> (String, Vec<String>) {
    let mut status = String::new();
    let mut ids = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("status:") {
            status = rest.trim().to_string();
        }
        if let Some(pos) = trimmed.find("AC-") {
            let id: String = trimmed[pos..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            if id.len() >= 8 && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    (status, ids)
}

/// Verifica el contrato de trazabilidad sobre el workspace (K1/K2/K3).
///
/// Args:
///     root: Raíz del workspace.
///
/// Returns:
///     Lista de incumplimientos (vacía = contrato verde).
pub fn contract(root: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    let mut ac_defined: HashSet<String> = HashSet::new();
    let mut implemented_missing: Vec<(PathBuf, String)> = Vec::new();

    // K3: specs implemented totalmente trazadas (recolecta ACs a la vez).
    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_rs(&root.join("crates"), &mut rs_files);
    let mut test_fns: HashSet<String> = HashSet::new();
    for path in &rs_files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            if let Some(pos) = line.find("fn ") {
                let name: String = line[pos + 3..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.starts_with("test_ac_") {
                    test_fns.insert(name);
                }
            }
        }
    }
    let mut specs = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("specs")) else {
        return vec!["no se pudo leer specs/".to_string()];
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let (status, ids) = specacs(&text);
        for id in &ids {
            ac_defined.insert(id.clone());
        }
        if status == "implemented" {
            for id in &ids {
                if let Some(numeric) = ac_number(id) {
                    let prefix = format!("test_ac_{numeric}");
                    if !test_fns.iter().any(|t| t.starts_with(&prefix)) {
                        implemented_missing.push((path.clone(), id.clone()));
                    }
                }
            }
        }
        specs.push(path);
    }
    let _ = specs;

    // K1: cada test_ac numérico con marcador cercano; K2: marcadores válidos.
    for path in &rs_files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if let Some(pos) = line.find("fn test_ac_") {
                let name: String = line[pos + 3..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !is_numbered_ac_test(&name) {
                    continue;
                }
                let from = i.saturating_sub(4);
                let context = lines[from..=i].join("\n");
                if !context.contains("@spec") {
                    errors.push(format!(
                        "K1 sin marcador: {} ({}:{})",
                        name,
                        path.display(),
                        i + 1
                    ));
                }
            }
            if line.contains("@spec") {
                for marker in markers_in(line) {
                    if marker_is_numbered(&marker) && !ac_defined.contains(&marker) {
                        errors.push(format!(
                            "K2 marcador huérfano: {marker} ({}:{})",
                            path.display(),
                            i + 1
                        ));
                    }
                }
            }
        }
    }
    for (spec, id) in implemented_missing {
        errors.push(format!(
            "K3 implemented sin test: {id} ({})",
            spec.display()
        ));
    }
    errors
}

/// Número `XXXX_NN` de un AC `AC-XXXX-NN`, si es numérico.
fn ac_number(id: &str) -> Option<String> {
    let rest = id.strip_prefix("AC-")?;
    let mut parts = rest.split('-');
    let a = parts.next()?;
    let b = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if a.len() == 4
        && a.bytes().all(|c| c.is_ascii_digit())
        && b.bytes().all(|c| c.is_ascii_digit())
    {
        Some(format!("{a}_{b}"))
    } else {
        None
    }
}

/// `true` si el test es `test_ac_XXXX_NN_*` numérico (los `_bva_` están exentos).
fn is_numbered_ac_test(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("test_ac_") else {
        return false;
    };
    let mut parts = rest.splitn(3, '_');
    let (Some(a), Some(b), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    a.len() == 4 && a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit())
}

/// Marcadores `AC-...` mencionados en una línea.
fn markers_in(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search = line;
    while let Some(pos) = search.find("AC-") {
        let id: String = search[pos..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if id.len() > 3 {
            out.push(id.clone());
        }
        search = &search[pos + 3..];
    }
    out
}

/// `true` si el marcador es numérico (`AC-XXXX-NN...`).
fn marker_is_numbered(marker: &str) -> bool {
    ac_number(marker).is_some()
}

/// Recolecta los `.rs` bajo un directorio.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Evalúa las rúbricas J1/J2/J3 sobre un diff unificado.
///
/// Args:
///     diff: Texto del diff (`git diff`).
///
/// Returns:
///     Veredicto con un hallazgo por rúbrica disparada.
pub fn judge_diff(diff: &str) -> Verdict {
    let mut findings = Vec::new();
    let mut current_file = String::new();
    // Ventana de líneas añadidas recientes para el contexto del marcador.
    let mut recent_added: Vec<String> = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = path.to_string();
            recent_added.clear();
            continue;
        }
        if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") {
            continue;
        }
        let Some(added) = line.strip_prefix('+') else {
            continue;
        };
        // J1: test_ac numérico nuevo sin @spec en la ventana ±3.
        if added.contains("fn test_ac_") {
            let name: String = added[added.find("fn ").unwrap_or(0) + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if is_numbered_ac_test(&name) {
                let window = recent_added
                    .iter()
                    .rev()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>();
                if !window.iter().any(|l| l.contains("@spec")) && !added.contains("@spec") {
                    findings.push(Finding {
                        id: "J1-missing-marker",
                        level: Level::Fail,
                        detail: format!("{name} sin marcador @spec ({current_file})"),
                    });
                }
            }
        }
        // J2: unsafe fuera de ruscadb-ffi (patrones de código, no menciones).
        let is_ffi = current_file.starts_with("crates/ruscadb-ffi/");
        let code_unsafe = added.contains("unsafe {")
            || added.contains("unsafe{")
            || added.contains("unsafe fn")
            || added.contains("unsafe impl")
            || added.contains("unsafe extern");
        if code_unsafe && !added.contains("forbid(unsafe") && !is_ffi {
            findings.push(Finding {
                id: "J2-unsafe-outside-ffi",
                level: Level::Fail,
                detail: format!(
                    "unsafe añadido fuera de ruscadb-ffi ({current_file}): {}",
                    added.trim()
                ),
            });
        }
        // J3: unwrap/expect en src/ (warn: los tests unitarios viven en src/).
        let in_src = current_file.contains("/src/") && !current_file.contains("/tests/");
        if in_src && (added.contains(".unwrap()") || added.contains(".expect(")) {
            findings.push(Finding {
                id: "J3-unwrap-in-src",
                level: Level::Warn,
                detail: format!("unwrap/expect añadido en {current_file}: {}", added.trim()),
            });
        }
        recent_added.push(added.to_string());
    }
    if findings.is_empty() {
        findings.push(Finding {
            id: "J0-clean",
            level: Level::Pass,
            detail: "diff limpio según J1/J2/J3".to_string(),
        });
    }
    Verdict { findings }
}

/// Matriz de acuerdo juez-vs-humano para κ y tasas de error.
///
/// API de calibración del futuro backend LLM (curso AI Evals W2): hoy solo la
/// usan los tests; el wiring con juicios versionados llegará con el backend.
/// Se permite dead_code hasta entonces para no ensuciar el build normal.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Agreement {
    /// Juez pasa, humano pasa.
    pub tp: u64,
    /// Juez pasa, humano falla (false-pass).
    pub fp: u64,
    /// Juez falla, humano pasa (false-fail).
    pub fn_: u64,
    /// Ambos fallan.
    pub tn: u64,
}

#[allow(dead_code)]
impl Agreement {
    /// κ de Cohen: acuerdo observado menos azar, sobre 1 menos azar.
    ///
    /// Returns:
    ///     κ en [-1, 1]; 0.0 si el denominador es 0 (sin variabilidad).
    pub fn cohen_kappa(self) -> f64 {
        let total = (self.tp + self.fp + self.fn_ + self.tn) as f64;
        if total == 0.0 {
            return 0.0;
        }
        let observed = (self.tp + self.tn) as f64 / total;
        let judge_pass = (self.tp + self.fp) as f64 / total;
        let human_pass = (self.tp + self.fn_) as f64 / total;
        let judge_fail = 1.0 - judge_pass;
        let human_fail = 1.0 - human_pass;
        let chance = judge_pass * human_pass + judge_fail * human_fail;
        if (1.0 - chance).abs() < f64::EPSILON {
            return 0.0;
        }
        (observed - chance) / (1.0 - chance)
    }

    /// Tasa de false-pass: `FP / (FP + TN)` (lo peligroso: verde que miente).
    pub fn false_pass_rate(self) -> f64 {
        let denom = (self.fp + self.tn) as f64;
        if denom == 0.0 {
            return 0.0;
        }
        self.fp as f64 / denom
    }

    /// Tasa de false-fail: `FN / (FN + TP)` (ruido que bloquea).
    pub fn false_fail_rate(self) -> f64 {
        let denom = (self.fn_ + self.tp) as f64;
        if denom == 0.0 {
            return 0.0;
        }
        self.fn_ as f64 / denom
    }
}

/// Resume un veredicto en una línea máquina-legible.
pub fn verdict_line(verdict: &Verdict) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for finding in &verdict.findings {
        let key = match finding.level {
            Level::Pass => "pass",
            Level::Warn => "warn",
            Level::Fail => "fail",
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    format!(
        "verdict={} pass={} warn={} fail={}",
        if verdict.passes() { "PASS" } else { "FAIL" },
        counts.get("pass").copied().unwrap_or(0),
        counts.get("warn").copied().unwrap_or(0),
        counts.get("fail").copied().unwrap_or(0),
    )
}

#[cfg(test)]
mod judge_tests {
    //! Tests de aceptación de SPEC-0061 (juez determinista + κ).

    use super::{Agreement, Level, judge_diff, verdict_line};

    /// Parsers internos: `specacs` deduce estado e ids únicos (MutGen: mata
    /// `&& -> ||` en el filtro de ids cortos/duplicados).
    #[test]
    fn specacs_parses_status_and_dedups() {
        let (status, ids) = super::specacs(
            "status: accepted\nacceptance_criteria:\n  - id: AC-0001-01\n  - id: AC-0001-01\n  - id: AC-1\n",
        );
        assert_eq!(status, "accepted");
        assert_eq!(
            ids,
            vec!["AC-0001-01".to_string()],
            "únicos y >= 8: {ids:?}"
        );
    }

    /// Parsers internos: `ac_number`/`markers_in` estrictos (MutGen: mata los
    /// `&& -> ||`, el `> -> >=` y el `-> true` de la validación numérica).
    #[test]
    fn ac_number_and_markers_are_strict() {
        assert_eq!(super::ac_number("AC-0001-01"), Some("0001_01".to_string()));
        for malformed in ["AC-WIP", "AC-01", "AC-ABCD-01", "AC-0001", "AC-0001-01-x"] {
            assert_eq!(
                super::ac_number(malformed),
                None,
                "{malformed} no es numérico"
            );
        }
        assert!(super::marker_is_numbered("AC-0001-01"));
        assert!(!super::marker_is_numbered("AC-WIP"));
        assert_eq!(
            super::markers_in("ver AC- y AC-0001-01"),
            vec!["AC-0001-01".to_string()],
            "el bare AC- se ignora"
        );
    }

    /// AC-0061-03 — rúbricas sobre diffs fixture (sucio FAIL, limpio SUCCESS).
    #[test]
    // @spec AC-0061-03
    fn test_ac_0061_03_judge_rubrics_on_fixture_diff() {
        // Fixtures en ficheros .diff: invisibles a los escáneres K1/K2 que solo
        // leen *.rs (evita falsos positivos del propio harness).
        let dirty = include_str!("fixtures/dirty.diff");
        let verdict = judge_diff(dirty);
        assert!(!verdict.passes(), "el diff sucio debe fallar");
        // Conteos exactos (MutGen): cada mutante de un solo operador en las
        // cadenas ||/&& de J2/J3 cambia estos conteos y debe morir aquí.
        let count = |id: &str, level: Level| {
            verdict
                .findings
                .iter()
                .filter(|f| f.id == id && f.level == level)
                .count()
        };
        assert_eq!(count("J1-missing-marker", Level::Fail), 1, "J1 exacto");
        assert_eq!(count("J2-unsafe-outside-ffi", Level::Fail), 3, "J2 exacto");
        assert_eq!(count("J3-unwrap-in-src", Level::Warn), 1, "J3 exacto");
        assert_eq!(
            verdict.findings.len(),
            5,
            "sin hallazgos de más ni de menos"
        );

        let clean = include_str!("fixtures/clean.diff");
        let verdict = judge_diff(clean);
        assert!(verdict.passes(), "el diff limpio debe pasar");
        assert_eq!(verdict_line(&verdict), "verdict=PASS pass=1 warn=0 fail=0");
    }

    /// AC-0061-04 — κ y tasas sobre matriz 2x2 conocida.
    #[test]
    // @spec AC-0061-04
    fn test_ac_0061_04_kappa_fixture() {
        // TP=70 FP=10 FN=5 TN=115: Po=0.925, Pe=0.525, κ=0.4/0.475.
        let agreement = Agreement {
            tp: 70,
            fp: 10,
            fn_: 5,
            tn: 115,
        };
        let kappa = agreement.cohen_kappa();
        assert!(
            (kappa - 0.842_105_263_157_894_7).abs() < 1e-9,
            "κ esperado ≈0.8421, se obtuvo {kappa}"
        );
        assert!((agreement.false_pass_rate() - 0.08).abs() < 1e-12);
        assert!((agreement.false_fail_rate() - 5.0 / 75.0).abs() < 1e-12);

        // Sin variabilidad ni casos: κ = 0.0 sin panics (fronteras).
        let empty = Agreement {
            tp: 0,
            fp: 0,
            fn_: 0,
            tn: 0,
        };
        assert_eq!(empty.cohen_kappa(), 0.0);
        assert_eq!(empty.false_pass_rate(), 0.0);
        assert_eq!(empty.false_fail_rate(), 0.0);

        // Degenerado con chance == 1 (acuerdo total): κ = 0.0 por guarda, no
        // NaN (MutGen: mata las mutaciones del guarda anti-división-por-cero).
        let total = Agreement {
            tp: 5,
            fp: 0,
            fn_: 0,
            tn: 0,
        };
        assert_eq!(total.cohen_kappa(), 0.0);
    }

    /// El contrato detecta K1 (marcador ausente) y K2 (huérfano) sobre un
    /// árbol fixture (MutGen: mata `marker_is_numbered -> false`, que apaga
    /// K2 en silencio).
    #[test]
    fn contract_detects_orphan_and_missing_markers() {
        let root =
            std::env::temp_dir().join(format!("ruscadb-contract-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("specs")).expect("specs");
        std::fs::create_dir_all(root.join("crates/a/src")).expect("src");
        // Los tokens `fn test_ac_` / `@spec AC-` se concatenan para que K1/K2
        // no vean este fuente (auto-hospedaje: los escáneres leen *.rs).
        std::fs::write(
            root.join("specs/fix.md"),
            "status: accepted\nacceptance_criteria:\n  - id: AC-0001-01\n",
        )
        .expect("spec");
        let rs = "// @spec AC-".to_string()
            + "0001-01\nfn test_ac_"
            + "0001_01_ok() {}\nfn helper_a() {}\nfn helper_b() {}\nfn helper_c() {}\nfn helper_d() {}\nfn test_ac_"
            + "0001_02_missing() {}\n// @spec AC-"
            + "0001-99\nfn helper() {}\n// @spec AC-"
            + "WIP\nfn malformed_ignored() {}\n";
        std::fs::write(root.join("crates/a/src/lib.rs"), rs).expect("rs");
        let errors = super::contract(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            errors.iter().any(|e| e.starts_with("K1")),
            "debe flaggear K1 (02 sin marcador): {errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.starts_with("K2") && e.contains("AC-0001-99")),
            "debe flaggear K2 (huérfano 99): {errors:?}"
        );
        assert!(
            !errors.iter().any(|e| e.contains("AC-0001-01")),
            "01 está bien trazado: {errors:?}"
        );
        assert!(
            !errors.iter().any(|e| e.contains("WIP")),
            "el marcador malformado se ignora: {errors:?}"
        );
    }
}
