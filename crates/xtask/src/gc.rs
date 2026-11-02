//! Recolector de artefactos regenerables del workspace (SPEC-0192).
//!
//! El recolector elimina **solo** rutas de una allowlist explícita (`target/`,
//! `mutants.out*`, `fuzz/target`, `coverage`, `crates/*/target` y perfiles de
//! cobertura en la raíz). Por defecto es un *dry-run* y solo borra artefactos
//! más antiguos que `max_age`; nunca toca fuente, documentación, specs, `.git`
//! ni el propio workspace.
//!
//! Complejidad: `O(N)` llamadas a `stat` sobre los artefactos candidatos
//! (`N` = ficheros de esos árboles) y `O(k)` memoria auxiliar (`k` = candidatos),
//! frente al recorrido recursivo del árbol completo del script histórico.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

/// Política de recolección.
#[derive(Debug, Clone)]
pub struct GcOptions {
    /// Si es `false`, solo informa (dry-run).
    pub apply: bool,
    /// Si es `true`, ignora la antigüedad y borra todos los candidatos.
    pub all: bool,
    /// Antigüedad mínima para borrar (por defecto 24 h).
    pub max_age: Duration,
    /// Emite el reporte como JSON.
    pub json: bool,
}

impl Default for GcOptions {
    fn default() -> Self {
        Self {
            apply: false,
            all: false,
            max_age: Duration::from_secs(24 * 60 * 60),
            json: false,
        }
    }
}

/// Acción aplicada (o prevista) a un candidato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcAction {
    /// Borrado porque cumple la política.
    Removed,
    /// Se borraría (dry-run) porque cumple la política.
    WouldRemove,
    /// Se conserva por ser más reciente que `max_age`.
    KeptFresh,
    /// Se omite porque contiene el ejecutable de `xtask` en uso (Windows).
    SkippedRunning,
    /// Error al borrar (se reporta y no aborta el resto).
    Error,
}

impl GcAction {
    /// Texto canónico de la acción.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::WouldRemove => "would-remove",
            Self::KeptFresh => "kept-fresh",
            Self::SkippedRunning => "skipped-running",
            Self::Error => "error",
        }
    }
}

/// Candidato evaluado por el recolector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcCandidate {
    /// Ruta absoluta del artefacto.
    pub path: PathBuf,
    /// Tamaño recursivo en bytes.
    pub bytes: u64,
    /// Antigüedad del fichero/directorio más reciente.
    pub age: Duration,
    /// Acción aplicada o prevista.
    pub action: GcAction,
}

/// Reporte agregado.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcReport {
    /// Raíz del workspace.
    pub root: PathBuf,
    /// `true` si no se borró nada.
    pub dry_run: bool,
    /// Candidatos en orden determinista.
    pub candidates: Vec<GcCandidate>,
    /// Errores de borrado (no abortan el resto).
    pub errors: Vec<String>,
    /// Omisiones seguras (p. ej. el ejecutable de `xtask` en uso).
    pub warnings: Vec<String>,
    /// Bytes liberados realmente.
    pub removed_bytes: u64,
    /// Bytes totales de los candidatos (antes de borrar).
    pub total_bytes: u64,
}

/// Indica si una ruta relativa es un artefacto regenerable permitido.
///
/// Args:
///     relative: Ruta relativa a la raíz del workspace.
///
/// Returns:
///     `true` solo para las rutas de la allowlist.
pub fn is_allowed_artifact(relative: &Path) -> bool {
    if relative.is_absolute() {
        return false;
    }
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => match value.to_str() {
                Some(text) => parts.push(text),
                None => return false,
            },
            _ => return false,
        }
    }
    match parts.as_slice() {
        ["target"] | ["mutants.out"] | ["mutants.out.old"] | ["fuzz", "target"] | ["coverage"] => {
            true
        }
        ["crates", _, "target"] => true,
        [file] => matches!(
            Path::new(file)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("profraw" | "profdata" | "lcov")
        ),
        _ => false,
    }
}

/// Recolecta (y opcionalmente borra) los artefactos regenerables.
///
/// Args:
///     root: Raíz del workspace.
///     options: Política de recolección.
///
/// Returns:
///     Reporte con candidatos, bytes y errores.
///
/// Errors:
///     Error de E/S al enumerar la raíz o los candidatos.
pub fn collect(root: &Path, options: &GcOptions) -> Result<GcReport, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("no se pudo resolver la raíz '{}': {error}", root.display()))?;
    let now = SystemTime::now();
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut removed_bytes = 0u64;
    let mut total_bytes = 0u64;
    for path in candidate_paths(&root) {
        let Ok(relative) = path.strip_prefix(&root) else {
            continue;
        };
        if !is_allowed_artifact(relative) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        // Nunca seguir un symlink que apunte fuera del workspace.
        let Ok(resolved) = path.canonicalize() else {
            continue;
        };
        if !resolved.starts_with(&root) || resolved == root {
            continue;
        }
        let bytes = size_of(&path).unwrap_or(metadata.len());
        let newest = newest_mtime(&path).unwrap_or_else(|_| metadata.modified().unwrap_or(now));
        let age = now.duration_since(newest).unwrap_or_default();
        total_bytes = total_bytes.saturating_add(bytes);
        if !options.all && age < options.max_age {
            candidates.push(GcCandidate {
                path,
                bytes,
                age,
                action: GcAction::KeptFresh,
            });
            continue;
        }
        // En Windows no se puede borrar el ejecutable en uso: se omite el
        // árbol completo en vez de dejar una eliminación parcial.
        if options.apply && running_exe_inside(&resolved) {
            warnings.push(format!(
                "'{}' contiene el ejecutable de xtask en uso; se omite (usa `cargo clean` o un CARGO_TARGET_DIR externo)",
                path.display()
            ));
            candidates.push(GcCandidate {
                path,
                bytes,
                age,
                action: GcAction::SkippedRunning,
            });
            continue;
        }
        if !options.apply {
            candidates.push(GcCandidate {
                path,
                bytes,
                age,
                action: GcAction::WouldRemove,
            });
            continue;
        }
        match remove_path(&path) {
            Ok(()) => {
                removed_bytes = removed_bytes.saturating_add(bytes);
                candidates.push(GcCandidate {
                    path,
                    bytes,
                    age,
                    action: GcAction::Removed,
                });
            }
            Err(error) => {
                errors.push(format!("no se pudo borrar '{}': {error}", path.display()));
                candidates.push(GcCandidate {
                    path,
                    bytes,
                    age,
                    action: GcAction::Error,
                });
            }
        }
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(GcReport {
        root,
        dry_run: !options.apply,
        candidates,
        errors,
        warnings,
        removed_bytes,
        total_bytes,
    })
}

/// Serializa el reporte a JSON determinista.
///
/// Args:
///     report: Reporte del recolector.
///
/// Returns:
///     JSON con rutas, bytes, antigüedad y acción por candidato.
pub fn report_json(report: &GcReport) -> String {
    let candidates: Vec<Value> = report
        .candidates
        .iter()
        .map(|candidate| {
            json!({
                "path": candidate.path.display().to_string(),
                "bytes": candidate.bytes,
                "age_seconds": candidate.age.as_secs(),
                "action": candidate.action.as_str(),
            })
        })
        .collect();
    let value = json!({
        "root": report.root.display().to_string(),
        "dry_run": report.dry_run,
        "candidates": candidates,
        "errors": report.errors,
        "warnings": report.warnings,
        "removed_bytes": report.removed_bytes,
        "total_bytes": report.total_bytes,
    });
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

/// Indica si el ejecutable actual vive dentro del candidato.
fn running_exe_inside(candidate: &Path) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Ok(exe) = exe.canonicalize() else {
        return false;
    };
    let Ok(candidate) = candidate.canonicalize() else {
        return false;
    };
    exe.starts_with(candidate)
}

/// Enumera las rutas candidatas de la allowlist.
fn candidate_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        root.join("target"),
        root.join("mutants.out"),
        root.join("mutants.out.old"),
        root.join("fuzz").join("target"),
        root.join("coverage"),
    ];
    if let Ok(entries) = fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            paths.push(entry.path().join("target"));
        }
    }
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let allowed = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension, "profraw" | "profdata" | "lcov"));
            if allowed {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Tamaño recursivo de un fichero o directorio (sin seguir symlinks).
fn size_of(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        total = total.saturating_add(size_of(&entry.path()).unwrap_or(0));
    }
    Ok(total)
}

/// Fecha de modificación más reciente del árbol (sin seguir symlinks).
fn newest_mtime(path: &Path) -> io::Result<SystemTime> {
    let metadata = fs::symlink_metadata(path)?;
    let own = metadata.modified()?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return Ok(own);
    }
    let mut newest = own;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if let Ok(child) = newest_mtime(&entry.path())
            && child > newest
        {
            newest = child;
        }
    }
    Ok(newest)
}

/// Borra un fichero o directorio sin seguir symlinks.
fn remove_path(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Crea un fichero con contenido (y sus directorios padre).
    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("crear directorio");
        }
        fs::write(path, bytes).expect("escribir fichero");
    }

    /// AC-0192-01 — el dry-run reporta pero no borra.
    #[test] // @spec AC-0192-01
    fn test_ac_0192_01_dry_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "target/debug/app.exe", b"0123456789");
        let report = collect(
            dir.path(),
            &GcOptions {
                apply: false,
                all: true,
                ..GcOptions::default()
            },
        )
        .expect("collect");
        assert!(report.dry_run);
        assert_eq!(report.removed_bytes, 0);
        assert!(report.total_bytes >= 10);
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.candidates[0].action, GcAction::WouldRemove);
        assert!(dir.path().join("target/debug/app.exe").exists());
    }

    /// AC-0192-02 — `--apply` borra solo artefactos y conserva la fuente.
    #[test] // @spec AC-0192-02
    fn test_ac_0192_02_apply_removes_only_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "target/debug/app.exe", b"build");
        write(dir.path(), "mutants.out/outcomes.json", b"{}");
        write(dir.path(), "specs/keep.md", b"spec");
        write(dir.path(), "docs/keep.md", b"docs");
        write(dir.path(), ".git/config", b"git");
        write(dir.path(), "crates/ruscadb/src/lib.rs", b"fn main() {}");
        let report = collect(
            dir.path(),
            &GcOptions {
                apply: true,
                all: true,
                ..GcOptions::default()
            },
        )
        .expect("collect");
        assert!(report.errors.is_empty(), "errores: {:?}", report.errors);
        assert!(report.removed_bytes > 0);
        assert!(!dir.path().join("target").exists());
        assert!(!dir.path().join("mutants.out").exists());
        assert!(dir.path().join("specs/keep.md").exists());
        assert!(dir.path().join("docs/keep.md").exists());
        assert!(dir.path().join(".git/config").exists());
        assert!(dir.path().join("crates/ruscadb/src/lib.rs").exists());
    }

    /// AC-0192-03 — sin `--all`, un artefacto reciente se conserva.
    #[test] // @spec AC-0192-03
    fn test_ac_0192_03_age_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "target/debug/app.exe", b"build");
        let fresh = collect(
            dir.path(),
            &GcOptions {
                apply: true,
                all: false,
                max_age: Duration::from_secs(24 * 60 * 60),
                ..GcOptions::default()
            },
        )
        .expect("collect fresh");
        assert_eq!(fresh.candidates[0].action, GcAction::KeptFresh);
        assert!(dir.path().join("target/debug/app.exe").exists());
        let all = collect(
            dir.path(),
            &GcOptions {
                apply: true,
                all: true,
                ..GcOptions::default()
            },
        )
        .expect("collect all");
        assert_eq!(all.candidates[0].action, GcAction::Removed);
        assert!(!dir.path().join("target").exists());
    }

    /// AC-0192-04 — segunda pasada idempotente y JSON determinista.
    #[test] // @spec AC-0192-04
    fn test_ac_0192_04_idempotent_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "target/a", b"a");
        write(dir.path(), "mutants.out/b", b"b");
        let options = GcOptions {
            apply: true,
            all: true,
            ..GcOptions::default()
        };
        let first = collect(dir.path(), &options).expect("primera");
        assert!(first.removed_bytes > 0);
        let second = collect(dir.path(), &options).expect("segunda");
        assert_eq!(second.removed_bytes, 0);
        assert!(second.candidates.is_empty());
        let json = report_json(&second);
        assert!(json.contains("\"removed_bytes\": 0"));
        assert!(json.contains("\"dry_run\": false"));
        assert!(json.contains("\"candidates\": []"));
        assert_eq!(GcAction::Removed.as_str(), "removed");
        assert_eq!(GcAction::WouldRemove.as_str(), "would-remove");
        assert_eq!(GcAction::KeptFresh.as_str(), "kept-fresh");
        assert_eq!(GcAction::Error.as_str(), "error");
    }

    /// Detecta el ejecutable en uso para no borrarlo en Windows.
    #[test]
    fn running_exe_detection() {
        let exe = std::env::current_exe().expect("exe");
        let parent = exe.parent().expect("parent");
        assert!(running_exe_inside(parent));
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!running_exe_inside(dir.path()));
    }

    /// El mtime del árbol es el del hijo más reciente, no el del directorio.
    #[test]
    fn newest_mtime_tracks_newest_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("target/debug");
        fs::create_dir_all(&nested).expect("dirs");
        fs::write(nested.join("old"), b"old").expect("old");
        std::thread::sleep(Duration::from_millis(20));
        fs::write(nested.join("new"), b"new").expect("new");
        let newest_file = fs::metadata(nested.join("new"))
            .expect("meta")
            .modified()
            .expect("mtime");
        let newest_tree = newest_mtime(&dir.path().join("target")).expect("tree mtime");
        assert!(
            newest_tree >= newest_file,
            "el árbol debe reflejar el hijo más reciente"
        );
    }

    /// AC-0192-05 — la allowlist rechaza fuente, docs, specs y `.git`.
    #[test] // @spec AC-0192-05
    fn test_ac_0192_05_protected_paths() {
        assert!(is_allowed_artifact(Path::new("target")));
        assert!(is_allowed_artifact(Path::new("crates/ruscadb/target")));
        assert!(is_allowed_artifact(Path::new("mutants.out")));
        assert!(is_allowed_artifact(Path::new("coverage")));
        assert!(is_allowed_artifact(Path::new("trace.profraw")));
        assert!(!is_allowed_artifact(Path::new("specs/artifact_gc.md")));
        assert!(!is_allowed_artifact(Path::new("docs/RuscaDB-roadmap.md")));
        assert!(!is_allowed_artifact(Path::new(".git/config")));
        assert!(!is_allowed_artifact(Path::new("crates/ruscadb/src/lib.rs")));
        assert!(!is_allowed_artifact(Path::new("../target")));
    }

    proptest! {
        /// Invariante: si una ruta es artefacto permitido, ningún componente
        /// puede ser de fuente/docs/specs/.git ni traversal.
        #[test]
        fn prop_allowed_never_protected(parts in prop::collection::vec("[a-z.]{1,6}", 1..4)) {
            let joined = parts.join("/");
            let path = Path::new(&joined);
            if is_allowed_artifact(path) {
                for part in &parts {
                    prop_assert!(!matches!(
                        part.as_str(),
                        ".." | ".git" | "specs" | "docs" | "src" | "scripts"
                    ));
                }
            }
        }
    }
}
