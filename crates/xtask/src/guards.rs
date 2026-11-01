//! Guards deterministas del workspace (portados desde `scripts/*.py`).
//!
//! Todos son std salvo `serde_json` (ya en el workspace) para leer
//! `cargo metadata` y `outcomes.json`. Ninguno escribe en la libreria: son
//! gates de desarrollo invocados por `cargo xtask <comando>`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Guard de arquitectura hexagonal (aristas, ciclos y contratos).
pub fn check_architecture(root: &Path) -> Result<(), String> {
    let declared = parse_architecture(root.join("architecture.toml").as_path())?;
    let graph = cargo_graph(root)?;
    let mut errors = Vec::new();

    for krate in graph.keys() {
        if !declared.contains_key(krate) {
            errors.push(format!(
                "crate desconocida '{krate}': declara sus dependencias en architecture.toml"
            ));
        }
    }
    for krate in declared.keys() {
        if !graph.contains_key(krate) {
            errors.push(format!(
                "contrato obsoleto: '{krate}' declarada en architecture.toml pero no existe"
            ));
        }
    }
    for (krate, deps) in &graph {
        let Some(allowed) = declared.get(krate) else {
            continue;
        };
        for dep in deps.difference(allowed) {
            errors.push(format!(
                "arista prohibida: '{krate}' -> '{dep}' (permitidas: {allowed:?})"
            ));
        }
    }
    for cycle in find_cycles(&graph) {
        errors.push(format!("ciclo de dependencias: {}", cycle.join(" -> ")));
    }

    if errors.is_empty() {
        println!(
            "[OK] check_architecture: {} crates, 0 ciclos, 0 aristas prohibidas (contratos: architecture.toml).",
            graph.len()
        );
        Ok(())
    } else {
        Err(errors.join("\n  - "))
    }
}

/// Guard de `unsafe` (core prohibido y presupuesto por crate).
pub fn check_unsafe(root: &Path) -> Result<(), String> {
    let budget = parse_budget(root.join("unsafe-allowlist.toml").as_path())?;
    let core = root.join("crates/ruscadb-core/src/lib.rs");
    if !fs::read_to_string(&core)
        .map_err(|e| e.to_string())?
        .contains("#![forbid(unsafe_code)]")
    {
        return Err("ruscadb-core/src/lib.rs no declara #![forbid(unsafe_code)]".to_string());
    }

    let mut errors = Vec::new();
    let crates_dir = root.join("crates");
    let mut entries: Vec<_> = fs::read_dir(&crates_dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let src = entry.path().join("src");
        if !src.is_dir() {
            continue;
        }
        let limit = budget.get(&name).copied().unwrap_or(0);
        let mut usages = Vec::new();
        collect_rs(&src, &mut usages)?;
        let mut hits = Vec::new();
        for file in usages {
            let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
            for line in unsafe_lines(&text) {
                hits.push((file.clone(), line));
            }
        }
        if hits.len() > limit {
            for (file, line) in hits {
                let relative = file.strip_prefix(root).unwrap_or(&file);
                errors.push(format!(
                    "{}:{line}: uso de unsafe (presupuesto de '{name}' = {limit})",
                    relative.display()
                ));
            }
        }
    }

    if errors.is_empty() {
        println!("[OK] check_core_unsafe: core sin unsafe; 0 usos sobre presupuesto.");
        Ok(())
    } else {
        Err(errors.join("\n  - "))
    }
}

/// Resultado agregado de una spec.
struct SpecRow {
    id: String,
    feature: String,
    status: String,
    maturity: String,
    file: String,
}

/// Guard de frontmatter/ACs de las specs.
pub fn check_specs(root: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut status_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut maturity_counts: BTreeMap<String, usize> = BTreeMap::new();

    for row in read_specs(root)? {
        let Some(header) = frontmatter(
            &fs::read_to_string(root.join("specs").join(&row.file)).map_err(|e| e.to_string())?,
        ) else {
            errors.push(format!("{}: falta frontmatter (--- ... ---)", row.file));
            continue;
        };
        let number = row.id.trim_start_matches("SPEC-").to_string();
        if row.feature == "-" || row.feature.is_empty() {
            errors.push(format!("{} ({}): falta 'feature'", row.id, row.file));
        }
        if row.status == "-" || row.status.is_empty() {
            errors.push(format!("{} ({}): falta 'status'", row.id, row.file));
        } else {
            *status_counts.entry(row.status.clone()).or_default() += 1;
            if !matches!(
                row.status.as_str(),
                "proposed" | "accepted" | "implemented" | "superseded" | "deferred"
            ) {
                errors.push(format!(
                    "{} ({}): status '{}' no permitido",
                    row.id, row.file, row.status
                ));
            }
        }
        if row.maturity != "-" && !row.maturity.is_empty() {
            *maturity_counts.entry(row.maturity.clone()).or_default() += 1;
            if !matches!(row.maturity.as_str(), "real" | "stub" | "placeholder") {
                errors.push(format!(
                    "{} ({}): maturity '{}' no permitido",
                    row.id, row.file, row.maturity
                ));
            }
        }
        if let Some(previous) = seen.insert(row.id.clone(), row.file.clone()) {
            errors.push(format!(
                "{}: id duplicado en {previous} y {}",
                row.id, row.file
            ));
        }

        let declared = declared_acs(&header);
        if declared
            .iter()
            .any(|ac| ac.split('-').nth(1) != Some(number.as_str()))
        {
            errors.push(format!(
                "{} ({}): AC declarado no pertenece a la spec",
                row.id, row.file
            ));
        }
        let mut counts: BTreeMap<&String, usize> = BTreeMap::new();
        for ac in &declared {
            *counts.entry(ac).or_default() += 1;
        }
        for (ac, count) in counts {
            if count > 1 {
                errors.push(format!(
                    "{} ({}): {ac} declarado dos veces",
                    row.id, row.file
                ));
            }
        }
        if row.status == "implemented" && declared.is_empty() {
            errors.push(format!(
                "{} ({}): implemented sin ACs declarados",
                row.id, row.file
            ));
        }
    }

    if errors.is_empty() {
        println!(
            "[OK] check_specs: {} specs (status: {status_counts:?}; maturity: {maturity_counts:?}).",
            seen.len()
        );
        Ok(())
    } else {
        Err(errors.join("\n  - "))
    }
}

/// Genera `docs/specs-index.md` a partir de `specs/*.md`.
pub fn gen_specs_index(root: &Path) -> Result<(), String> {
    let rows = read_specs(root)?;
    let mut lines = vec![
        "# Indice de specs (generado)".to_string(),
        String::new(),
        "> Generado desde el frontmatter de `specs/*.md` por `cargo xtask gen-specs-index`."
            .to_string(),
        "> No editar a mano: regenerar tras cambiar una spec.".to_string(),
        String::new(),
        "| Spec | Feature | Status | Madurez | Archivo |".to_string(),
        "|---|---|---|---|---|".to_string(),
    ];
    for row in &rows {
        lines.push(format!(
            "| {} | `{}` | {} | {} | `{}` |",
            row.id, row.feature, row.status, row.maturity, row.file
        ));
    }
    lines.push(String::new());
    fs::write(root.join("docs/specs-index.md"), lines.join("\n")).map_err(|e| e.to_string())?;
    println!("index: {} specs -> docs/specs-index.md", rows.len());
    Ok(())
}

/// Gate agregado de mutacion (lee `outcomes.json`, umbral en %).
pub fn mutation_score(paths: &[PathBuf], threshold: f64, allow_empty: bool) -> Result<(), String> {
    let mut caught = 0usize;
    let mut missed = 0usize;
    let mut timeout = 0usize;
    let mut unviable = 0usize;
    for path in paths {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let outcomes = value
            .get("outcomes")
            .and_then(serde_json::Value::as_array)
            .or_else(|| value.as_array())
            .ok_or_else(|| format!("{}: falta 'outcomes'", path.display()))?;
        let mut file_caught = 0;
        let mut file_missed = 0;
        let mut file_timeout = 0;
        for outcome in outcomes {
            match outcome.get("summary").and_then(serde_json::Value::as_str) {
                Some("CaughtMutant") => {
                    caught += 1;
                    file_caught += 1;
                }
                Some("MissedMutant") => {
                    missed += 1;
                    file_missed += 1;
                }
                Some("Timeout") => {
                    timeout += 1;
                    file_timeout += 1;
                }
                Some("Unviable") => unviable += 1,
                _ => {}
            }
        }
        let evaluated = file_caught + file_missed + file_timeout;
        if evaluated > 0 {
            println!(
                "[shard] {}: MS={:.2}% (caught={file_caught}, missed={file_missed}, timeout={file_timeout})",
                path.display(),
                100.0 * (file_caught + file_timeout) as f64 / evaluated as f64
            );
        }
    }
    let evaluated = caught + missed + timeout;
    if evaluated == 0 {
        if allow_empty {
            println!("[OK] mutation score: sin mutantes evaluables (--allow-empty)");
            return Ok(());
        }
        return Err("sin mutantes evaluables; el gate no puede dar verde".to_string());
    }
    let percent = 100.0 * (caught + timeout) as f64 / evaluated as f64;
    println!(
        "[{}] mutation score global = {percent:.2}% (objetivo >= {threshold:.2}%; caught={caught}, missed={missed}, timeout={timeout}, unviable={unviable})",
        if percent >= threshold { "OK" } else { "FALLA" }
    );
    if percent >= threshold {
        Ok(())
    } else {
        Err(format!("MS {percent:.2}% < {threshold:.2}%"))
    }
}

/// Valida mensajes Conventional Commits (SPEC-0089).
///
/// Acepta `--range A..B`, `--file ruta` o `--message texto`.
pub fn check_commits(args: &[String]) -> Result<(), String> {
    let mut subjects = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--range" => {
                index += 1;
                let Some(range) = args.get(index) else {
                    return Err("uso: --range A..B".to_string());
                };
                let output = Command::new("git")
                    .args(["log", "--no-merges", "--format=%s", range])
                    .output()
                    .map_err(|e| format!("git log: {e}"))?;
                if !output.status.success() {
                    return Err(format!(
                        "no se pudo leer el rango '{range}': {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                subjects.extend(
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .map(str::to_string),
                );
            }
            "--file" => {
                index += 1;
                let Some(file) = args.get(index) else {
                    return Err("uso: --file ruta".to_string());
                };
                let text = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
                subjects.extend(
                    text.lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .map(str::to_string),
                );
            }
            "--message" => {
                index += 1;
                let Some(message) = args.get(index) else {
                    return Err("uso: --message texto".to_string());
                };
                subjects.push(message.trim().to_string());
            }
            other => return Err(format!("argumento desconocido: {other}")),
        }
        index += 1;
    }
    if subjects.is_empty() {
        return Err("sin mensajes que revisar (rango/fichero vacio)".to_string());
    }

    let allowed = [
        "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style",
        "test",
    ];
    let mut offenders = Vec::new();
    for subject in &subjects {
        if subject.starts_with("Merge ") || subject.starts_with("Revert \"") {
            continue;
        }
        let Some((head, body)) = subject.split_once(": ") else {
            offenders.push(format!("{subject:?}: no sigue 'type(scope): descripcion'"));
            continue;
        };
        if body.trim().is_empty() {
            offenders.push(format!("{subject:?}: descripcion vacia"));
            continue;
        }
        let kind = head.split(['(', '!']).next().unwrap_or(head);
        if !allowed.contains(&kind) {
            offenders.push(format!("{subject:?}: tipo '{kind}' no permitido"));
        }
    }
    if offenders.is_empty() {
        println!(
            "check_commits: OK ({} mensaje(s) Conventional Commits)",
            subjects.len()
        );
        Ok(())
    } else {
        Err(offenders.join("\n  - "))
    }
}

/// Valida el CHANGELOG (Keep a Changelog) y opcionalmente extrae notas.
pub fn check_changelog(
    path: &Path,
    tag: Option<&str>,
    notes_out: Option<&Path>,
) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut problems = Vec::new();
    match section_body(&text, "## [Unreleased]") {
        None => problems.push("falta la seccion '## [Unreleased]'".to_string()),
        Some(body) if !has_content(&body) => {
            problems.push("la seccion '## [Unreleased]' esta vacia".to_string());
        }
        Some(_) => {}
    }
    if let Some(tag) = tag {
        let version = tag.trim().trim_start_matches(['v', 'V']);
        let header = format!("## [{version}]");
        match section_body(&text, &header) {
            None => problems.push(format!("falta la seccion '{header}' para el tag '{tag}'")),
            Some(body) if !has_content(&body) => {
                problems.push(format!("la seccion '{header}' esta vacia"));
            }
            Some(body) => {
                if let Some(notes_out) = notes_out {
                    fs::write(notes_out, format!("{}\n", body.trim()))
                        .map_err(|e| format!("{}: {e}", notes_out.display()))?;
                    println!(
                        "check_changelog: notas escritas en '{}'",
                        notes_out.display()
                    );
                }
            }
        }
    }
    if problems.is_empty() {
        println!("check_changelog: OK ('{}')", path.display());
        Ok(())
    } else {
        Err(problems.join("\n  - "))
    }
}

fn section_body(text: &str, header_prefix: &str) -> Option<String> {
    let mut body = String::new();
    let mut collecting = false;
    for line in text.lines() {
        if line.starts_with("## ") {
            if collecting {
                break;
            }
            if line.starts_with(header_prefix) {
                collecting = true;
            }
            continue;
        }
        if collecting {
            body.push_str(line);
            body.push('\n');
        }
    }
    collecting.then_some(body)
}

fn has_content(body: &str) -> bool {
    body.lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
}

/// Auditoria reproducible de specs vs codigo (`cargo xtask audit`).
///
/// Comprueba: ACs declarados con test existente, specs implementadas con ACs,
/// madurez declarada y marcadores reales de `todo!`/`unimplemented!`.
pub fn audit(root: &Path) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut ac_total = 0usize;
    let mut status: BTreeMap<String, usize> = BTreeMap::new();
    let mut maturity: BTreeMap<String, usize> = BTreeMap::new();

    let mut rust = String::new();
    let mut files = Vec::new();
    collect_rs(&root.join("crates"), &mut files)?;
    for file in files {
        rust.push_str(&fs::read_to_string(&file).unwrap_or_default());
        rust.push('\n');
    }

    let mut entries: Vec<_> = fs::read_dir(root.join("specs"))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file = entry.file_name().to_string_lossy().into_owned();
        if !file.ends_with(".md") || file.starts_with('_') {
            continue;
        }
        let text = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
        let Some(header) = frontmatter(&text) else {
            continue;
        };
        let spec_status = field(&header, "status");
        *status.entry(spec_status.clone()).or_default() += 1;
        *maturity.entry(field(&header, "maturity")).or_default() += 1;
        let acs = declared_acs(&header);
        ac_total += acs.len();
        for test in declared_tests(&header) {
            if !rust.contains(&format!("fn {test}")) {
                missing.push(format!("{file}: test '{test}' no existe"));
            }
        }
        if spec_status == "implemented" && acs.is_empty() {
            missing.push(format!("{file}: implemented sin ACs"));
        }
    }

    let real_markers = ["todo!(", "unimplemented!("];
    for (index, line) in rust.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || line.contains('"') {
            continue;
        }
        if real_markers.iter().any(|marker| line.contains(marker)) {
            missing.push(format!(
                "codigo: marcador sin implementar en linea {}",
                index + 1
            ));
        }
    }

    println!("[audit] {ac_total} ACs; status={status:?}; maturity={maturity:?}");
    if missing.is_empty() {
        println!("[OK] audit: 0 ACs sin test, 0 todo!/unimplemented!");
        Ok(())
    } else {
        Err(missing.join("\n  - "))
    }
}

fn declared_tests(header: &str) -> Vec<String> {
    header
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("test:")?;
            Some(rest.trim().to_string())
        })
        .collect()
}

fn parse_architecture(path: &Path) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut declared = BTreeMap::new();
    let mut in_crates = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[") {
            in_crates = trimmed == "[crates]";
            continue;
        }
        if !in_crates || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = trimmed.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"').to_string();
        let deps = rest
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|dep| dep.trim().trim_matches('"').to_string())
            .filter(|dep| !dep.is_empty())
            .collect();
        declared.insert(name, deps);
    }
    if declared.is_empty() {
        return Err("architecture.toml: falta la tabla [crates]".to_string());
    }
    Ok(declared)
}

fn parse_budget(path: &Path) -> Result<BTreeMap<String, usize>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut budget = BTreeMap::new();
    let mut in_budget = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_budget = trimmed == "[budget]";
            continue;
        }
        if !in_budget || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = trimmed.split_once('=') {
            let value = value.trim().parse().map_err(|_| {
                format!(
                    "unsafe-allowlist.toml: presupuesto invalido '{}'",
                    value.trim()
                )
            })?;
            budget.insert(name.trim().to_string(), value);
        }
    }
    Ok(budget)
}

fn cargo_graph(root: &Path) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("metadata JSON: {e}"))?;
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "metadata sin packages".to_string())?;
    let names: BTreeSet<&str> = packages
        .iter()
        .filter_map(|package| package.get("name").and_then(serde_json::Value::as_str))
        .collect();
    let mut graph = BTreeMap::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let mut internal = BTreeSet::new();
        if let Some(deps) = package
            .get("dependencies")
            .and_then(serde_json::Value::as_array)
        {
            for dep in deps {
                let Some(dep_name) = dep.get("name").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let kind = dep.get("kind").and_then(serde_json::Value::as_str);
                if names.contains(dep_name) && matches!(kind, None | Some("build")) {
                    internal.insert(dep_name.to_string());
                }
            }
        }
        graph.insert(name.to_string(), internal);
    }
    Ok(graph)
}

fn find_cycles(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    let mut color: HashMap<&str, u8> = HashMap::new();
    let mut stack = Vec::new();
    let mut cycles = Vec::new();
    fn visit<'a>(
        node: &'a str,
        graph: &'a BTreeMap<String, BTreeSet<String>>,
        color: &mut HashMap<&'a str, u8>,
        stack: &mut Vec<&'a str>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(node, 1);
        stack.push(node);
        if let Some(nexts) = graph.get(node) {
            for next in nexts {
                match color.get(next.as_str()).copied().unwrap_or(0) {
                    1 => {
                        let start = stack
                            .iter()
                            .position(|item| *item == next.as_str())
                            .unwrap_or(0);
                        let mut cycle: Vec<String> = stack[start..]
                            .iter()
                            .map(|item| (*item).to_string())
                            .collect();
                        cycle.push(next.clone());
                        cycles.push(cycle);
                    }
                    0 => visit(next, graph, color, stack, cycles),
                    _ => {}
                }
            }
        }
        stack.pop();
        color.insert(node, 2);
    }
    for node in graph.keys() {
        if color.get(node.as_str()).copied().unwrap_or(0) == 0 {
            visit(node, graph, &mut color, &mut stack, &mut cycles);
        }
    }
    cycles
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out.sort();
    Ok(())
}

fn unsafe_lines(source: &str) -> Vec<usize> {
    let mut clean = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_block = false;
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if in_block {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            clean.push(' ');
            continue;
        }
        if in_string {
            if ch == '\\' {
                chars.next();
            } else if ch == '"' {
                in_string = false;
            }
            clean.push(' ');
            continue;
        }
        if ch == '"' {
            in_string = true;
            clean.push(' ');
        } else if ch == '/' && chars.peek() == Some(&'/') {
            while chars.next().is_some_and(|next| next != '\n') {}
            clean.push('\n');
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_block = true;
            clean.push(' ');
        } else {
            clean.push(ch);
        }
    }
    clean
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            line.match_indices("unsafe").any(|(index, _)| {
                let previous = line[..index].chars().next_back();
                let next = line[index + "unsafe".len()..].chars().next();
                let boundary = previous
                    .map(|ch| !ch.is_alphanumeric() && ch != '_')
                    .unwrap_or(true);
                boundary && next != Some('_')
            })
        })
        .map(|(index, _)| index + 1)
        .collect()
}

fn read_specs(root: &Path) -> Result<Vec<SpecRow>, String> {
    let mut rows = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(root.join("specs"))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file = entry.file_name().to_string_lossy().into_owned();
        if !file.ends_with(".md") || file.starts_with('_') {
            continue;
        }
        let text = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
        let Some(header) = frontmatter(&text) else {
            continue;
        };
        rows.push(SpecRow {
            id: field(&header, "id"),
            feature: field(&header, "feature"),
            status: field(&header, "status"),
            maturity: field(&header, "maturity"),
            file,
        });
    }
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(rows)
}

fn frontmatter(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut header = String::new();
    for line in lines {
        if line.trim() == "---" {
            return Some(header);
        }
        header.push_str(line);
        header.push('\n');
    }
    None
}

fn field(header: &str, name: &str) -> String {
    for line in header.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&format!("{name}:")) {
            return rest.trim().to_string();
        }
    }
    "-".to_string()
}

fn declared_acs(header: &str) -> Vec<String> {
    header
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("- id:")?;
            let id = rest.trim();
            id.starts_with("AC-").then(|| id.to_string())
        })
        .collect()
}
