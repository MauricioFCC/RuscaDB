//! Checkpoint a S3 (SPEC-0128): manifiesto de backup.

use std::fs;
use std::path::PathBuf;

/// Backup S3 (stub: genera manifiesto JSON).
#[derive(Debug, Clone)]
pub struct S3Backup {
    /// Ruta origen de la base.
    source: PathBuf,
    /// Bucket S3 destino (simulado).
    bucket: String,
}

impl S3Backup {
    /// Crea un backup S3.
    pub fn new(source: impl Into<PathBuf>, bucket: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            bucket: bucket.into(),
        }
    }

    /// Genera un manifiesto JSON de los ficheros de la base.
    pub fn manifest(&self) -> Result<String, String> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.source).map_err(|e| format!("read_dir: {e}"))? {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                files.push(format!(r#"{{"name":"{}","size":{}}}"#, name, size));
            }
        }
        Ok(format!(
            r#"{{"bucket":"{}","files":[{}]}}"#,
            self.bucket,
            files.join(",")
        ))
    }

    /// Bucket destino.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test] // @spec AC-0128-01
    fn test_ac_0128_01_s3_manifest() {
        let dir = std::env::temp_dir().join(format!("ruscadb-s3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src");
        fs::write(src.join("data.db"), b"hello").expect("write");

        let backup = S3Backup::new(&src, "my-bucket");
        let manifest = backup.manifest().expect("manifest");
        assert!(manifest.contains("my-bucket"));
        assert!(manifest.contains("data.db"));

        let _ = fs::remove_dir_all(&dir);
    }
}
