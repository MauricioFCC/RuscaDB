#![allow(missing_docs)]
#![allow(dead_code)]
//! Vistas materializadas (SPEC-0149): stub.

#[derive(Debug, Clone)]
pub struct MaterializedView {
    pub name: String,
    pub query: String,
    pub rows: Vec<Vec<String>>,
}

impl MaterializedView {
    pub fn new(name: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            query: query.into(),
            rows: Vec::new(),
        }
    }
    pub fn create(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
    }
    pub fn refresh(&mut self, rows: Vec<Vec<String>>) {
        self.rows = rows;
    }
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn query(&self) -> &str {
        &self.query
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0149-01
    fn test_ac_0149_01_materialized_view_create() {
        let mut view = MaterializedView::new("v1", "SELECT * FROM t");
        view.create(vec![vec!["1".to_string(), "a".to_string()]]);
        assert_eq!(view.rows().len(), 1);
        assert_eq!(view.name(), "v1");
    }
}
