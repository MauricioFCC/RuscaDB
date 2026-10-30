//! Vistas materializadas (SPEC-0149): resultado precomputado de una consulta.
//!
//! La vista guarda las filas del ultimo `refresh`; el `query` asociado permite
//! recalcularlas con el executor. No se refresca sola (el disparador es del
//! consumidor), lo que evita coste oculto.

/// Vista materializada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedView {
    /// Nombre de la vista.
    pub name: String,
    /// Consulta que la alimenta.
    pub query: String,
    /// Filas precomputadas.
    pub rows: Vec<Vec<String>>,
}

impl MaterializedView {
    /// Crea la vista (sin filas).
    ///
    /// Args:
    ///     name: Nombre.
    ///     query: Consulta RQL.
    ///
    /// Returns:
    ///     Vista vacia.
    pub fn new(name: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            query: query.into(),
            rows: Vec::new(),
        }
    }

    /// Carga el resultado inicial.
    ///
    /// Args:
    ///     rows: Filas precomputadas.
    pub fn create(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
    }

    /// Reemplaza el resultado (refresh).
    ///
    /// Args:
    ///     rows: Filas nuevas.
    pub fn refresh(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
    }

    /// Filas precomputadas.
    ///
    /// Returns:
    ///     Slice de filas.
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// Nombre de la vista.
    ///
    /// Returns:
    ///     Nombre declarado.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Consulta asociada.
    ///
    /// Returns:
    ///     Texto RQL.
    pub fn query(&self) -> &str {
        &self.query
    }
}

#[cfg(test)]
mod tests {
    use super::MaterializedView;

    #[test] // @spec AC-0149-01
    fn test_ac_0149_01_materialized_view_create() {
        let mut view = MaterializedView::new("v1", "SELECT * FROM t");
        assert!(view.rows().is_empty());
        view.create(vec![vec!["1".to_string(), "a".to_string()]]);
        assert_eq!(view.rows().len(), 1);
        assert_eq!(view.name(), "v1");
        assert_eq!(view.query(), "SELECT * FROM t");
        view.refresh(vec![vec!["2".to_string()]]);
        assert_eq!(view.rows()[0][0], "2");
    }
}
