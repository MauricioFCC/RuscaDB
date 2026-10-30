//! Reordenamiento de joins (SPEC-0153): heuristica cost-based.
//!
//! En un nested-loop conviene que la tabla externa sea la mas pequena; el
//! reordenador decide si hay que intercambiar el orden.

/// Reordenador de joins.
#[derive(Debug, Clone, Copy, Default)]
pub struct JoinReorderer;

impl JoinReorderer {
    /// Decide si hay que intercambiar las entradas.
    ///
    /// Args:
    ///     left_rows: Filas de la entrada izquierda.
    ///     right_rows: Filas de la entrada derecha.
    ///
    /// Returns:
    ///     `true` si la izquierda debe pasar a la derecha (es mas grande).
    pub fn reorder(left_rows: usize, right_rows: usize) -> bool {
        left_rows > right_rows
    }
}

#[cfg(test)]
mod tests {
    use super::JoinReorderer;

    #[test] // @spec AC-0153-01
    fn test_ac_0153_01_join_reorder() {
        assert!(JoinReorderer::reorder(100, 10));
        assert!(!JoinReorderer::reorder(10, 100));
        assert!(!JoinReorderer::reorder(10, 10));
    }
}
