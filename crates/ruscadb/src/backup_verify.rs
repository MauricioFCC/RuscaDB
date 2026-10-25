#![allow(missing_docs)]
#![allow(dead_code)]
//! Verificacion de backup (SPEC-0157): checksums.

use std::fs;

#[derive(Debug, Clone)]
pub struct BackupVerifier {
    source: std::path::PathBuf,
    dest: std::path::PathBuf,
}

impl BackupVerifier {
    pub fn new(source: impl Into<std::path::PathBuf>, dest: impl Into<std::path::PathBuf>) -> Self {
        Self {
            source: source.into(),
            dest: dest.into(),
        }
    }
    pub fn verify(&self) -> Result<bool, String> {
        let src_entries: Vec<_> = fs::read_dir(&self.source)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .collect();
        for entry in &src_entries {
            let name = entry.file_name();
            let src_path = self.source.join(&name);
            let dst_path = self.dest.join(&name);
            if !dst_path.exists() {
                return Ok(false);
            }
            let src_bytes = fs::read(&src_path).map_err(|e| e.to_string())?;
            let dst_bytes = fs::read(&dst_path).map_err(|e| e.to_string())?;
            if src_bytes != dst_bytes {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test] // @spec AC-0157-01
    fn test_ac_0157_01_backup_verify() {
        let dir = std::env::temp_dir().join(format!("bv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = dir.join("src");
        let dst = dir.join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("a.db"), b"hello").unwrap();
        fs::write(dst.join("a.db"), b"hello").unwrap();
        let verifier = BackupVerifier::new(&src, &dst);
        assert!(verifier.verify().unwrap());
        let _ = fs::remove_dir_all(&dir);
    }
}
