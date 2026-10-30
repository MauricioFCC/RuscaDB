//! Sentencias preparadas (SPEC-0148): fachada de [`PreparedStmtReal`].
//!
//! Mantiene el nombre historico `PreparedStatement` y delega toda la logica en
//! la implementacion real (SPEC-0166): validacion de marcadores, tipos y
//! escapado de literales.

use crate::prepared_real::PreparedStmtReal;

/// Sentencia preparada (fachada de compatibilidad).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedStatement {
    inner: PreparedStmtReal,
}

impl PreparedStatement {
    /// Prepara una consulta contando sus marcadores (`$n` / `?`).
    ///
    /// Args:
    ///     query: Texto RQL.
    ///
    /// Returns:
    ///     Sentencia lista para `bind`.
    pub fn prepare(query: impl Into<String>) -> Self {
        Self {
            inner: PreparedStmtReal::prepare(query, 0),
        }
    }

    /// Texto de la consulta (inmutable).
    ///
    /// Returns:
    ///     Consulta original.
    pub fn query(&self) -> &str {
        self.inner.query()
    }

    /// Numero de parametros esperados.
    ///
    /// Returns:
    ///     Aridad declarada.
    pub fn param_count(&self) -> usize {
        self.inner.param_count()
    }

    /// Comprueba la aridad de los parametros.
    ///
    /// Args:
    ///     params: Valores enteros.
    ///
    /// Returns:
    ///     `true` si la aridad coincide.
    pub fn bind(&self, params: &[i64]) -> bool {
        self.inner.bind(params)
    }

    /// Acceso a la implementacion real (tipos, bind_values, cache key).
    ///
    /// Returns:
    ///     Referencia a [`PreparedStmtReal`].
    pub fn inner(&self) -> &PreparedStmtReal {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::PreparedStatement;

    #[test] // @spec AC-0148-01
    fn test_ac_0148_01_prepared_select() {
        let statement = PreparedStatement::prepare("SELECT * FROM t WHERE id = $1");
        assert_eq!(statement.query(), "SELECT * FROM t WHERE id = $1");
        assert_eq!(statement.param_count(), 1);
        assert!(statement.bind(&[42]));
        assert!(!statement.bind(&[]));
        assert_eq!(statement.inner().query(), statement.query());
    }
}
