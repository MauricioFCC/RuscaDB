//! SQL estandar (SPEC-0141): funciones de ventana.
//!
//! Implementa `ROW_NUMBER` y `RANK` (con huecos) sobre valores ya ordenados.

/// Funciones de ventana SQL.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowFunc;

impl WindowFunc {
    /// `ROW_NUMBER()`: 1..=n.
    ///
    /// Args:
    ///     values: Filas de la particion.
    ///
    /// Returns:
    ///     Numeracion correlativa.
    pub fn row_number(values: &[i64]) -> Vec<usize> {
        values
            .iter()
            .enumerate()
            .map(|(index, _)| index + 1)
            .collect()
    }

    /// `RANK()`: empates comparten rango y dejan huecos.
    ///
    /// Args:
    ///     values: Valores ordenados.
    ///
    /// Returns:
    ///     Rango por fila (1, 1, 3, ...).
    pub fn rank(values: &[i64]) -> Vec<usize> {
        let mut result = vec![1usize; values.len()];
        for index in 1..values.len() {
            if values[index] != values[index - 1] {
                result[index] = index + 1;
            } else {
                result[index] = result[index - 1];
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::WindowFunc;

    #[test] // @spec AC-0141-01
    fn test_ac_0141_01_row_number() {
        let values = vec![10, 20, 30];
        assert_eq!(WindowFunc::row_number(&values), vec![1, 2, 3]);
        assert_eq!(WindowFunc::rank(&[10, 10, 20]), vec![1, 1, 3]);
    }
}
