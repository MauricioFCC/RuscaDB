//! Single-file VLE (SPEC-0137): stub sin dependencias externas.

/// Base de datos single-file (stub).
#[derive(Debug, Clone)]
pub struct VleDatabase {
    /// Ruta del fichero.
    pub path: String,
}

impl VleDatabase {
    /// Crea una base VLE.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    /// Ruta.
    pub fn path(&self) -> &str {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0137-01
    fn test_ac_0137_01_vle_create() {
        let db = VleDatabase::new("test.pvdb");
        assert_eq!(db.path(), "test.pvdb");
    }
}
