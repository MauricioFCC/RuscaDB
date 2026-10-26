#![allow(missing_docs)]
#![allow(dead_code)]
//! RQL CTE (SPEC-0163): stub.

#[derive(Debug, Clone, Default)]
pub struct RqlCte {
    pub name: String,
    pub recursive: bool,
}

impl RqlCte {
    pub fn new(name: impl Into<String>, recursive: bool) -> Self {
        Self {
            name: name.into(),
            recursive,
        }
    }
    pub fn is_recursive(&self) -> bool {
        self.recursive
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Punto fijo: itera hasta no haber nuevas filas (limite de profundidad).
    pub fn fixed_point(
        &self,
        seed: Vec<i64>,
        step: impl Fn(&[i64]) -> Vec<i64>,
        max_depth: usize,
    ) -> Vec<i64> {
        let mut acc = seed;
        for _ in 0..max_depth {
            let next = step(&acc);
            if next.is_empty() {
                break;
            }
            acc.extend(next);
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0163-01
    fn test_ac_0163_01_cte() {
        let c = RqlCte::new("t", false);
        assert_eq!(c.name(), "t");
        assert!(!c.is_recursive());
    }
    #[test] // @spec AC-0163-02
    fn test_ac_0163_02_recursive() {
        let c = RqlCte::new("g", true);
        let res = c.fixed_point(
            vec![1],
            |rows| {
                if rows.len() < 4 {
                    vec![rows.len() as i64 + 1]
                } else {
                    vec![]
                }
            },
            10,
        );
        assert_eq!(res, vec![1, 2, 3, 4]);
    }
}
