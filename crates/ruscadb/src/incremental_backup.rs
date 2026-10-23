//! Backup incremental (SPEC-0120): copia solo ficheros modificados.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Backup incremental basado en timestamp de modificación.
#[derive(Debug, Clone)]
pub struct IncrementalBackup {
    /// Ruta origen de la base.
    source: PathBuf,
    /// Timestamp del último backup (epoch secs).
    last_backup: Option<u64>,
}

impl IncrementalBackup {
    /// Crea un backup incremental.
    pub fn new(source: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            last_backup: None,
        }
    }

    /// Establece el timestamp del último backup.
    pub fn set_last_backup(&mut self, ts: u64) {
        self.last_backup = Some(ts);
    }

    /// Copia solo los ficheros modificados desde el último backup.
    pub fn snapshot(&self, dest: &Path) -> Result<usize, String> {
        fs::create_dir_all(dest)
            .map_err(|e| format!("no se pudo crear el directorio destino: {e}"))?;
        let mut copied = 0;
        for entry in
            fs::read_dir(&self.source).map_err(|e| format!("no se pudo leer el origen: {e}"))?
        {
            let entry = entry.map_err(|e| format!("error de directorio: {e}"))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Some(last) = self.last_backup
                && modified <= last
            {
                continue;
            }
            let name = path
                .file_name()
                .ok_or_else(|| "nombre invalido".to_string())?;
            let dest_path = dest.join(name);
            fs::copy(&path, &dest_path)
                .map_err(|e| format!("no se pudo copiar '{}': {e}", path.display()))?;
            copied += 1;
        }
        Ok(copied)
    }

    /// Ruta origen.
    pub fn source(&self) -> &Path {
        &self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test] // @spec AC-0120-01
    fn test_ac_0120_01_incremental_backup() {
        let dir = std::env::temp_dir().join(format!("ruscadb-incr-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = dir.join("src");
        let dst = dir.join("dst");
        fs::create_dir_all(&src).expect("create src");
        fs::write(src.join("a.db"), b"hello").expect("write a");

        // Primer backup: copia todo.
        let backup = IncrementalBackup::new(&src);
        let copied = backup.snapshot(&dst).expect("snapshot 1");
        assert_eq!(copied, 1);

        // Segundo backup sin cambios: copia 0.
        let mut backup2 = IncrementalBackup::new(&src);
        backup2.set_last_backup(u64::MAX);
        let copied2 = backup2.snapshot(&dst).expect("snapshot 2");
        assert_eq!(copied2, 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
