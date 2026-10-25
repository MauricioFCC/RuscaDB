#![allow(missing_docs)]
#![allow(dead_code)]
//! Lance format (SPEC-0136): stub.

#[derive(Debug, Clone, Default)]
pub struct LanceTable {
    pub name: String,
    pub row_count: usize,
}

impl LanceTable {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            row_count: 0,
        }
    }
    pub fn append(&mut self) {
        self.row_count += 1;
    }
    pub fn len(&self) -> usize {
        self.row_count
    }
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0136-01
    fn test_ac_0136_01_lance_append_scan() {
        let mut table = LanceTable::new("test");
        table.append();
        table.append();
        assert_eq!(table.len(), 2);
    }
}
