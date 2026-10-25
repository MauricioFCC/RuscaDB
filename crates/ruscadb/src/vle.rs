#![allow(missing_docs)]
#![allow(dead_code)]
//! Single-file VLE (SPEC-0137): stub.

#[derive(Debug, Clone)]
pub struct VleDatabase {
    pub path: String,
}

impl VleDatabase {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
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
