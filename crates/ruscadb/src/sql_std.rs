//! SQL estándar (SPEC-0141): window functions, CTEs, MERGE (stub).

/// Función de ventana (stub).
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowFunc;

impl WindowFunc {
    /// Calcula row_number por partición.
    pub fn row_number(values: &[i64]) -> Vec<usize> {
        values.iter().enumerate().map(|(i, _)| i + 1).collect()
    }

    /// Calcula rank por partición.
    pub fn rank(values: &[i64]) -> Vec<usize> {
        let mut result = vec![1usize; values.len()];
        for i in 1..values.len() {
            if values[i] != values[i - 1] {
                result[i] = i + 1;
            } else {
                result[i] = result[i - 1];
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0141-01
    fn test_ac_0141_01_row_number() {
        let values = vec![10, 20, 30];
        let rn = WindowFunc::row_number(&values);
        assert_eq!(rn, vec![1, 2, 3]);
    }
}
