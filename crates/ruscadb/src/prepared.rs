#![allow(missing_docs)]
#![allow(dead_code)]
//! Sentencias preparadas (SPEC-0148): stub.

#[derive(Debug, Clone)]
pub struct PreparedStatement {
    pub query: String,
}

impl PreparedStatement {
    pub fn prepare(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
        }
    }
    pub fn query(&self) -> &str {
        &self.query
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0148-01
    fn test_ac_0148_01_prepared_select() {
        let stmt = PreparedStatement::prepare("SELECT * FROM t");
        assert_eq!(stmt.query(), "SELECT * FROM t");
    }
}
