//! Verificacion de backup (SPEC-0157): comparacion byte a byte origen/destino.
//!
//! Comprueba que cada fichero del origen exista en el destino con contenido
//! identico. Pensado para validar un backup antes de declararlo exitoso.

use std::fs;

/// Verificador de copias de seguridad.
#[derive(Debug, Clone)]
pub struct BackupVerifier {
    source: std::path::PathBuf,
    dest: std::path::PathBuf,
}

impl BackupVerifier {
    /// Crea el verificador.
    ///
    /// Args:
    ///     source: Directorio origen.
    ///     dest: Directorio de la copia.
    ///
    /// Returns:
    ///     Verificador listo.
    pub fn new(source: impl Into<std::path::PathBuf>, dest: impl Into<std::path::PathBuf>) -> Self {
        Self {
            source: source.into(),
            dest: dest.into(),
        }
    }

    /// Verifica que la copia coincide con el origen.
    ///
    /// Returns:
    ///     `Ok(true)` si todos los ficheros coinciden; `Ok(false)` si falta
    ///     alguno o difiere.
    ///
    /// Errors:
    ///     `Err(String)` si no se pueden leer los directorios/ficheros.
    pub fn verify(&self) -> Result<bool, String> {
        let source_entries: Vec<_> = fs::read_dir(&self.source)
            .map_err(|error| error.to_string())?
            .filter_map(|entry| entry.ok())
            .collect();
        for entry in &source_entries {
            let name = entry.file_name();
            let source_path = self.source.join(&name);
            let destination_path = self.dest.join(&name);
            if !destination_path.exists() {
                return Ok(false);
            }
            let source_bytes = fs::read(&source_path).map_err(|error| error.to_string())?;
            let destination_bytes =
                fs::read(&destination_path).map_err(|error| error.to_string())?;
            if source_bytes != destination_bytes {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::BackupVerifier;
    use std::fs;

    #[test] // @spec AC-0157-01
    fn test_ac_0157_01_backup_verify() {
        let dir = std::env::temp_dir().join(format!("bv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let source = dir.join("src");
        let dest = dir.join("dst");
        fs::create_dir_all(&source).expect("src");
        fs::create_dir_all(&dest).expect("dst");
        fs::write(source.join("a.db"), b"alpha").expect("write src");
        fs::write(dest.join("a.db"), b"alpha").expect("write dst");
        assert!(
            BackupVerifier::new(&source, &dest)
                .verify()
                .expect("verify")
        );

        fs::write(dest.join("a.db"), b"other").expect("corrupt");
        assert!(
            !BackupVerifier::new(&source, &dest)
                .verify()
                .expect("verify")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
