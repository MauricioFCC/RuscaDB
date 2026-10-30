//! Backup online opcional (SPEC-0116): copia de ficheros de la base.

use std::fs;
use std::path::{Path, PathBuf};

/// Backup de la base RuscaDB.
#[derive(Debug, Clone)]
pub struct Backup {
    /// Ruta origen de la base.
    source: PathBuf,
}

impl Backup {
    /// Crea un backup a partir de la ruta de la base.
    pub fn new(source: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
        }
    }

    /// Copia los ficheros de la base al directorio destino.
    pub fn snapshot(&self, dest: &Path) -> Result<usize, String> {
        fs::create_dir_all(dest)
            .map_err(|e| format!("no se pudo crear el directorio destino: {e}"))?;
        let mut copied = 0;
        for entry in
            fs::read_dir(&self.source).map_err(|e| format!("no se pudo leer el origen: {e}"))?
        {
            let entry = entry.map_err(|e| format!("error de directorio: {e}"))?;
            let path = entry.path();
            if path.is_file() {
                let name = path
                    .file_name()
                    .ok_or_else(|| "nombre invalido".to_string())?;
                let dest_path = dest.join(name);
                fs::copy(&path, &dest_path)
                    .map_err(|e| format!("no se pudo copiar '{}': {e}", path.display()))?;
                copied += 1;
            }
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

    #[test] // @spec AC-0116-01
    fn test_ac_0116_01_backup_snapshot() {
        let dir = std::env::temp_dir().join(format!("ruscadb-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = dir.join("src");
        let dst = dir.join("dst");
        fs::create_dir_all(&src).expect("create src");
        fs::write(src.join("data.db"), b"hello").expect("write data");
        fs::write(src.join("data.wal"), b"world").expect("write wal");

        let backup = Backup::new(&src);
        let copied = backup.snapshot(&dst).expect("snapshot");
        assert_eq!(copied, 2);
        assert!(dst.join("data.db").exists());
        assert!(dst.join("data.wal").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
