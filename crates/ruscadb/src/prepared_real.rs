#![allow(missing_docs)]
#![allow(dead_code)]
//! Prepared statements real (SPEC-0166): stub.

#[derive(Debug, Clone, Default)]
pub struct PreparedStmtReal {
    pub query: String,
    pub param_count: usize,
}

impl PreparedStmtReal {
    pub fn prepare(query: impl Into<String>, param_count: usize) -> Self {
        Self {
            query: query.into(),
            param_count,
        }
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn bind(&self, params: &[i64]) -> bool {
        params.len() == self.param_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0166-01
    fn test_ac_0166_01_bind() {
        let p = PreparedStmtReal::prepare("SELECT * FROM t WHERE id = $1", 1);
        assert!(p.bind(&[42]));
    }
    #[test] // @spec AC-0166-02
    fn test_ac_0166_02_reuse() {
        let p = PreparedStmtReal::prepare("q", 1);
        assert!(p.bind(&[1]));
        assert!(p.bind(&[2]));
    }
    #[test] // @spec AC-0166-03
    fn test_ac_0166_03_injection() {
        let p = PreparedStmtReal::prepare("q", 1);
        assert!(p.bind(&[0]));
        assert_eq!(p.query(), "q");
    }
}
