//! Sentencias preparadas (SPEC-0148): fachada minima del contrato de query.
//!
//! La implementacion completa (validacion de marcadores, tipos y escapado) vive
//! en [`crate::prepared_real::PreparedStmtReal`] (SPEC-0166). Este tipo se
//! mantiene por compatibilidad de la API publica.

/// Sentencia preparada (fachada de compatibilidad).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedStatement {
    query: String,
}

impl PreparedStatement {
    /// Crea una sentencia con el texto de la consulta.
    ///
    /// Args:
    ///     query: Texto RQL.
    ///
    /// Returns:
    ///     Sentencia lista para consultar su texto.
    pub fn prepare(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
        }
    }

    /// Texto de la consulta.
    ///
    /// Returns:
    ///     Consulta original.
    pub fn query(&self) -> &str {
        &self.query
    }
}

#[cfg(test)]
mod tests {
    use super::PreparedStatement;

    #[test] // @spec AC-0148-01
    fn test_ac_0148_01_prepared_select() {
        let statement = PreparedStatement::prepare("SELECT * FROM t");
        assert_eq!(statement.query(), "SELECT * FROM t");
        assert_eq!(statement.clone().query(), "SELECT * FROM t");
    }
}
