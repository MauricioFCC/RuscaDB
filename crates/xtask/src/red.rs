//! Gate RED-first (SPEC-0094): evidencia de que los tests de un spec **en
//! curso** (`status: planned|draft`) fallan antes de la implementación.
//!
//! El comando `cargo xtask red <spec>` ejecuta los tests citados por los ACs
//! del spec y exige que **fallen** (RED). Un spec `implemented` no es objeto de
//! este gate (sus tests deben pasar). La clasificación de la salida de `cargo
//! test` es pura y testeable ([`evaluate`]); el proceso se lanza desde
//! `main.rs`.
//!
//! Límite del gate: solo aplica a specs en curso; una vez `implemented` la
//! evidencia RED ya no es reproducible (el harness no reescribe la historia).

/// Estado RED-first de un spec en curso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedVerdict {
    /// Los tests existen y fallan (o el target no compila): evidencia RED.
    Red,
    /// Los tests existen y pasan: no hay evidencia RED.
    AlreadyPassing,
    /// Ninguno de los tests citados se ejecutó: el spec no está cableado.
    Missing,
}

/// Datos de un spec relevantes para el gate RED-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedSpec {
    /// Estado declarado en el frontmatter (`planned`/`draft`/`implemented`...).
    pub status: String,
    /// Nombres de test citados en los ACs (`test:`).
    pub tests: Vec<String>,
    /// Crates declarados en `boundaries.crates` (para acotar `cargo test -p`).
    pub crates: Vec<String>,
}

/// Indica si un estado es objeto del gate RED-first.
///
/// Args:
///     status: Estado declarado por el spec.
///
/// Returns:
///     `true` si el spec está en curso (`planned`/`draft`).
pub fn is_in_progress(status: &str) -> bool {
    matches!(status.trim(), "planned" | "draft")
}

/// Extrae estado, tests y crates de un `specs/<feature>.md`.
///
/// Args:
///     text: Contenido completo del spec.
///
/// Returns:
///     El [`RedSpec`] con los campos encontrados (vacíos si faltan).
pub fn parse_spec(text: &str) -> RedSpec {
    let mut spec = RedSpec {
        status: String::new(),
        tests: Vec::new(),
        crates: Vec::new(),
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("status:") {
            spec.status = rest.trim().to_string();
        }
        if let Some(rest) = trimmed.strip_prefix("test:") {
            let name = rest.trim().to_string();
            if !name.is_empty() && !spec.tests.contains(&name) {
                spec.tests.push(name);
            }
        }
        if let Some(rest) = trimmed.strip_prefix("crates:") {
            for name in parse_crate_list(rest) {
                if !spec.crates.contains(&name) {
                    spec.crates.push(name);
                }
            }
        }
    }
    spec
}

/// Parsea `[a, b, c]` (lista inline de `boundaries.crates`).
///
/// Args:
///     rest: Texto tras `crates:` (p. ej. `[ruscadb, ruscadb-mcp]`).
///
/// Returns:
///     Los nombres de crate, saneados y sin duplicados.
fn parse_crate_list(rest: &str) -> Vec<String> {
    rest.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|name| name.trim().trim_matches('"').trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Clasifica la salida de `cargo test` en un veredicto RED-first.
///
/// Reglas deterministas:
/// - un error de compilación (`error[E` / `error:`) es RED (la API aún no
///   existe);
/// - `failed > 0` es RED;
/// - `passed == 0` sin fallos ni error es [`RedVerdict::Missing`] (ningún test
///   del spec se ejecutó: filtro sin coincidencias);
/// - cualquier otro caso con tests ejecutados y sin fallos es
///   [`RedVerdict::AlreadyPassing`].
///
/// Args:
///     output: stdout+stderr de `cargo test` (filtrado por los tests del spec).
///
/// Returns:
///     El veredicto [`RedVerdict`].
pub fn evaluate(output: &str) -> RedVerdict {
    let mut passed = 0_u64;
    let mut failed = 0_u64;
    let mut saw_result = false;
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("error[E") || line.starts_with("error:") {
            return RedVerdict::Red;
        }
        if let Some(rest) = line.strip_prefix("test result:") {
            saw_result = true;
            for part in rest.split(';') {
                let tokens: Vec<&str> = part.split_whitespace().collect();
                if tokens.len() < 2 {
                    continue;
                }
                let value = tokens[tokens.len() - 2].parse::<u64>().unwrap_or(0);
                match tokens[tokens.len() - 1] {
                    "passed" => passed += value,
                    "failed" => failed += value,
                    _ => {}
                }
            }
        }
    }
    if failed > 0 {
        return RedVerdict::Red;
    }
    if !saw_result || passed == 0 {
        return RedVerdict::Missing;
    }
    RedVerdict::AlreadyPassing
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// AC-0094-01 — `evaluate` clasifica RED/pass/missing de forma determinista.
    #[test] // @spec AC-0094-01
    fn test_ac_0094_01_evaluate_classifies_red_pass_and_missing() {
        // (a) El target no compila: RED (la API del spec aún no existe).
        let compile = "error[E0599]: no function or associated item named `read_write`";
        assert_eq!(evaluate(compile), RedVerdict::Red);

        // (b) Un test falla: RED.
        let failing = "running 2 tests\n\
            test foo ... FAILED\n\
            test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out";
        assert_eq!(evaluate(failing), RedVerdict::Red);

        // (c) Todos pasan: no hay evidencia RED.
        let passing = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
        assert_eq!(evaluate(passing), RedVerdict::AlreadyPassing);

        // (d) Filtro sin coincidencias: el spec no está cableado.
        let missing = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
        assert_eq!(evaluate(missing), RedVerdict::Missing);
        assert_eq!(evaluate(""), RedVerdict::Missing);
    }

    /// El parser extrae estado, tests y crates (incluida la lista inline).
    #[test]
    fn parse_spec_reads_status_tests_and_crates() {
        let text = "---\nstatus: planned\nboundaries:\n  crates: [ruscadb, ruscadb-mcp]\n\
            acceptance_criteria:\n  - id: AC-1\n    test: test_ac_0001_01_x\n";
        let spec = parse_spec(text);
        assert_eq!(spec.status, "planned");
        assert_eq!(spec.tests, ["test_ac_0001_01_x"]);
        assert_eq!(spec.crates, ["ruscadb", "ruscadb-mcp"]);
        assert!(is_in_progress(&spec.status));
        assert!(!is_in_progress("implemented"));
    }
}
