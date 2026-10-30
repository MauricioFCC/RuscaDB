//! Single-file VLE (SPEC-0137): identificador de base en un unico fichero.

/// Base VLE (single-file) identificada por su ruta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VleDatabase {
    /// Ruta del fichero unico.
    pub path: String,
}

impl VleDatabase {
    /// Crea la referencia a una base VLE.
    ///
    /// Args:
    ///     path: Ruta del fichero.
    ///
    /// Returns:
    ///     Referencia lista.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    /// Ruta del fichero.
    ///
    /// Returns:
    ///     Ruta declarada.
    pub fn path(&self) -> &str {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::VleDatabase;

    #[test] // @spec AC-0137-01
    fn test_ac_0137_01_vle_create() {
        let database = VleDatabase::new("test.pvdb");
        assert_eq!(database.path(), "test.pvdb");
    }
}
