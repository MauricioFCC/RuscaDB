//! Operaciones de conjuntos RQL con semantica de bolsa SQL (SPEC-0164).
//!
//! SQL no trabaja con conjuntos matematicos sino con **bolsas** (multiconjuntos):
//! `INTERSECT` conserva `min(n_a, n_b)` copias de cada valor y `EXCEPT`
//! conserva `max(0, n_a - n_b)`. `UNION` deduplica y `UNION ALL` conserva
//! todas las copias. Se preserva el orden de la entrada izquierda.

use std::collections::BTreeMap;

/// Operaciones de conjuntos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RqlSetOps;

impl RqlSetOps {
    /// Crea el operador.
    ///
    /// Returns:
    ///     Instancia sin estado.
    pub const fn new() -> Self {
        Self
    }

    /// `UNION`: deduplica preservando el orden de aparicion.
    ///
    /// Args:
    ///     a: Entrada izquierda.
    ///     b: Entrada derecha.
    ///
    /// Returns:
    ///     Valores unicos en orden de primera aparicion.
    pub fn union(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        let mut seen = BTreeMap::new();
        let mut out = Vec::with_capacity(a.len() + b.len());
        for &value in a.iter().chain(b) {
            if seen.insert(value, ()).is_none() {
                out.push(value);
            }
        }
        out
    }

    /// `UNION ALL`: concatena sin deduplicar.
    ///
    /// Args:
    ///     a: Entrada izquierda.
    ///     b: Entrada derecha.
    ///
    /// Returns:
    ///     Concatenacion.
    pub fn union_all(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        let mut out = a.to_vec();
        out.extend_from_slice(b);
        out
    }

    /// `INTERSECT`: conserva `min(n_a, n_b)` copias de cada valor.
    ///
    /// Args:
    ///     a: Entrada izquierda.
    ///     b: Entrada derecha.
    ///
    /// Returns:
    ///     Interseccion de bolsa en orden de `a`.
    pub fn intersect(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        let mut available = Self::counts(b);
        let mut out = Vec::new();
        for &value in a {
            if let Some(count) = available.get_mut(&value)
                && *count > 0
            {
                *count -= 1;
                out.push(value);
            }
        }
        out
    }

    /// `EXCEPT`: conserva `max(0, n_a - n_b)` copias de cada valor.
    ///
    /// Args:
    ///     a: Entrada izquierda.
    ///     b: Entrada derecha.
    ///
    /// Returns:
    ///     Diferencia de bolsa en orden de `a`.
    pub fn except(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        let mut removable = Self::counts(b);
        let mut out = Vec::new();
        for &value in a {
            match removable.get_mut(&value) {
                Some(count) if *count > 0 => *count -= 1,
                _ => out.push(value),
            }
        }
        out
    }

    /// Alias explicito de [`Self::intersect`] (semantica de bolsa).
    ///
    /// Args:
    ///     a: Entrada izquierda.
    ///     b: Entrada derecha.
    ///
    /// Returns:
    ///     Interseccion de bolsa.
    pub fn intersect_all(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        self.intersect(a, b)
    }

    /// Alias explicito de [`Self::except`] (semantica de bolsa).
    ///
    /// Args:
    ///     a: Entrada izquierda.
    ///     b: Entrada derecha.
    ///
    /// Returns:
    ///     Diferencia de bolsa.
    pub fn except_all(&self, a: &[i64], b: &[i64]) -> Vec<i64> {
        self.except(a, b)
    }

    /// Cuenta ocurrencias de cada valor.
    fn counts(values: &[i64]) -> BTreeMap<i64, usize> {
        let mut counts = BTreeMap::new();
        for &value in values {
            *counts.entry(value).or_insert(0) += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::RqlSetOps;

    /// AC-0164-01: `UNION` deduplica.
    #[test]
    // @spec AC-0164-01
    fn test_ac_0164_01_union() {
        let ops = RqlSetOps::new();
        assert_eq!(ops.union(&[1, 2], &[2, 3]), vec![1, 2, 3]);
        assert_eq!(ops.union(&[1, 1], &[1]), vec![1]);
        assert_eq!(ops.union(&[], &[]), Vec::<i64>::new());
    }

    /// AC-0164-02: `UNION ALL` conserva duplicados y orden.
    #[test]
    // @spec AC-0164-02
    fn test_ac_0164_02_union_all() {
        let ops = RqlSetOps::new();
        assert_eq!(ops.union_all(&[1, 2], &[2, 3]), vec![1, 2, 2, 3]);
    }

    /// AC-0164-03: `INTERSECT` con multiplicidad minima.
    #[test]
    // @spec AC-0164-03
    fn test_ac_0164_03_intersect() {
        let ops = RqlSetOps::new();
        assert_eq!(ops.intersect(&[1, 2, 3], &[2, 3, 4]), vec![2, 3]);
        assert_eq!(
            ops.intersect(&[1, 1, 1], &[1, 1]),
            vec![1, 1],
            "min(3,2) = 2 copias"
        );
        assert_eq!(ops.intersect_all(&[5, 5], &[5, 5, 5]), vec![5, 5]);
        assert_eq!(ops.intersect(&[1], &[2]), Vec::<i64>::new());
    }

    /// AC-0164-04: `EXCEPT` resta multiplicidades.
    #[test]
    // @spec AC-0164-04
    fn test_ac_0164_04_except() {
        let ops = RqlSetOps::new();
        assert_eq!(ops.except(&[1, 2, 3], &[2]), vec![1, 3]);
        assert_eq!(
            ops.except(&[1, 1, 2], &[1]),
            vec![1, 2],
            "max(0, 2-1) = 1 copia"
        );
        assert_eq!(ops.except_all(&[1, 1], &[1, 1, 1]), Vec::<i64>::new());
    }
}
