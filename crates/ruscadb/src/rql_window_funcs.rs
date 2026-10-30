//! Funciones de ventana RQL con marcos `ROWS ... AND ...` (SPEC-0165).
//!
//! Implementa `ROW_NUMBER`, `RANK` (con huecos), `DENSE_RANK`, `LAG`/`LEAD` y
//! agregados sobre marco (`SUM`, `AVG`, `COUNT`) con la semantica SQL de NULL:
//! los agregados ignoran NULL y devuelven NULL si el marco no aporta ninguna
//! fila con valor.

/// Limite de marco.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// Desde el inicio de la particion.
    UnboundedPreceding,
    /// `n` filas antes de la actual.
    Preceding(usize),
    /// Fila actual.
    CurrentRow,
    /// `n` filas despues de la actual.
    Following(usize),
    /// Hasta el final de la particion.
    UnboundedFollowing,
}

/// Marco de ventana (`ROWS BETWEEN start AND end`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// Inicio del marco.
    pub start: Bound,
    /// Fin del marco.
    pub end: Bound,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            start: Bound::UnboundedPreceding,
            end: Bound::CurrentRow,
        }
    }
}

/// Funciones de ventana.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RqlWindowFuncs {
    /// Marco aplicado por los agregados.
    pub frame: Frame,
}

impl RqlWindowFuncs {
    /// Crea el motor con marco `RANGE UNBOUNDED PRECEDING .. CURRENT ROW`.
    ///
    /// Returns:
    ///     Instancia lista.
    pub fn new() -> Self {
        Self::default()
    }

    /// Crea el motor con un marco explicito.
    ///
    /// Args:
    ///     frame: Marco a aplicar.
    ///
    /// Returns:
    ///     Instancia configurada.
    pub fn with_frame(frame: Frame) -> Self {
        Self { frame }
    }

    /// `ROW_NUMBER() OVER ()`: 1..=n.
    ///
    /// Args:
    ///     n: Numero de filas.
    ///
    /// Returns:
    ///     Numeracion correlativa desde 1.
    pub fn row_number(&self, n: usize) -> Vec<usize> {
        (1..=n).collect()
    }

    /// `RANK() OVER (ORDER BY values)`: empates comparten rango y dejan huecos.
    ///
    /// Args:
    ///     values: Valores ya ordenados.
    ///
    /// Returns:
    ///     Rango por fila (1, 1, 3, ...).
    pub fn rank(&self, values: &[i64]) -> Vec<usize> {
        let mut ranks = Vec::with_capacity(values.len());
        for (index, value) in values.iter().enumerate() {
            if index > 0 && *value == values[index - 1] {
                ranks.push(ranks[index - 1]);
            } else {
                ranks.push(index + 1);
            }
        }
        ranks
    }

    /// `DENSE_RANK()`: empates comparten rango sin huecos.
    ///
    /// Args:
    ///     values: Valores ya ordenados.
    ///
    /// Returns:
    ///     Rango denso (1, 1, 2, ...).
    pub fn dense_rank(&self, values: &[i64]) -> Vec<usize> {
        let mut ranks = Vec::with_capacity(values.len());
        let mut current = 0usize;
        for (index, value) in values.iter().enumerate() {
            if index == 0 || *value != values[index - 1] {
                current += 1;
            }
            ranks.push(current);
        }
        ranks
    }

    /// `LAG(values, offset)`: valor `offset` filas antes.
    ///
    /// Args:
    ///     values: Valores de la particion.
    ///     offset: Desplazamiento hacia atras.
    ///
    /// Returns:
    ///     `None` donde no hay fila previa.
    pub fn lag(&self, values: &[i64], offset: usize) -> Vec<Option<i64>> {
        values
            .iter()
            .enumerate()
            .map(|(index, _)| index.checked_sub(offset).map(|prev| values[prev]))
            .collect()
    }

    /// `LEAD(values, offset)`: valor `offset` filas despues.
    ///
    /// Args:
    ///     values: Valores de la particion.
    ///     offset: Desplazamiento hacia delante.
    ///
    /// Returns:
    ///     `None` donde no hay fila siguiente.
    pub fn lead(&self, values: &[i64], offset: usize) -> Vec<Option<i64>> {
        (0..values.len())
            .map(|index| values.get(index + offset).copied())
            .collect()
    }

    /// `SUM(values) OVER ()` sin NULL (compatibilidad).
    ///
    /// Args:
    ///     values: Valores.
    ///
    /// Returns:
    ///     Suma total.
    pub fn sum_over(&self, values: &[i64]) -> i64 {
        values.iter().sum()
    }

    /// `SUM(...) OVER (ROWS ...)`: suma por marco, ignorando NULL.
    ///
    /// Args:
    ///     values: Valores de la particion (NULL permitido).
    ///
    /// Returns:
    ///     Un resultado por fila; `None` si el marco no tiene valores.
    pub fn sum_frame(&self, values: &[Option<i64>]) -> Vec<Option<i64>> {
        self.aggregate_frame(values, |window| {
            let sum: i64 = window.iter().flatten().sum();
            if window.iter().any(Option::is_some) {
                Some(sum)
            } else {
                None
            }
        })
    }

    /// `AVG(...) OVER (ROWS ...)`: media por marco, ignorando NULL.
    ///
    /// Args:
    ///     values: Valores de la particion.
    ///
    /// Returns:
    ///     Un resultado por fila; `None` si el marco no tiene valores.
    pub fn avg_frame(&self, values: &[Option<i64>]) -> Vec<Option<f64>> {
        self.aggregate_frame(values, |window| {
            let present: Vec<i64> = window.iter().flatten().copied().collect();
            if present.is_empty() {
                None
            } else {
                let sum: i64 = present.iter().sum();
                Some(sum as f64 / present.len() as f64)
            }
        })
    }

    /// `COUNT(...) OVER (ROWS ...)`: cuenta valores no nulos del marco.
    ///
    /// Args:
    ///     values: Valores de la particion.
    ///
    /// Returns:
    ///     Un conteo por fila.
    pub fn count_frame(&self, values: &[Option<i64>]) -> Vec<usize> {
        self.aggregate_frame(values, |window| Some(window.iter().flatten().count()))
            .into_iter()
            .map(|value| value.unwrap_or(0))
            .collect()
    }

    /// Aplica `aggregate` a la ventana de cada fila.
    fn aggregate_frame<T>(
        &self,
        values: &[Option<i64>],
        aggregate: impl Fn(&[Option<i64>]) -> Option<T>,
    ) -> Vec<Option<T>> {
        (0..values.len())
            .map(|index| {
                let (start, end) = self.window(index, values.len());
                if start > end {
                    None
                } else {
                    aggregate(&values[start..=end])
                }
            })
            .collect()
    }

    /// Resuelve `[start, end]` para la fila `index`.
    ///
    /// Args:
    ///     index: Fila actual.
    ///     len: Tamano de la particion.
    ///
    /// Returns:
    ///     Rango inclusivo; `start > end` significa marco vacio.
    fn window(&self, index: usize, len: usize) -> (usize, usize) {
        if len == 0 {
            return (0, 0);
        }
        let last = len - 1;
        let start = match self.frame.start {
            Bound::UnboundedPreceding => 0,
            Bound::Preceding(offset) => index.saturating_sub(offset),
            Bound::CurrentRow => index,
            Bound::Following(offset) => index.saturating_add(offset).min(last),
            Bound::UnboundedFollowing => last,
        };
        let end = match self.frame.end {
            Bound::UnboundedPreceding => 0,
            Bound::Preceding(offset) => index.saturating_sub(offset),
            Bound::CurrentRow => index,
            Bound::Following(offset) => index.saturating_add(offset).min(last),
            Bound::UnboundedFollowing => last,
        };
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::{Bound, Frame, RqlWindowFuncs};

    /// AC-0165-01: `ROW_NUMBER` y `RANK`/`DENSE_RANK`.
    #[test]
    // @spec AC-0165-01
    fn test_ac_0165_01_row_number() {
        let window = RqlWindowFuncs::new();
        assert_eq!(window.row_number(3), vec![1, 2, 3]);
        assert_eq!(window.row_number(0), Vec::<usize>::new());
        assert_eq!(window.rank(&[10, 10, 20, 30]), vec![1, 1, 3, 4]);
        assert_eq!(window.dense_rank(&[10, 10, 20, 30]), vec![1, 1, 2, 3]);
    }

    /// AC-0165-02: `SUM` global y por marco con NULL.
    #[test]
    // @spec AC-0165-02
    fn test_ac_0165_02_sum_over() {
        let window = RqlWindowFuncs::new();
        assert_eq!(window.sum_over(&[1, 2, 3]), 6);

        // Marco por defecto: acumulado hasta la fila actual.
        let values = [Some(1), Some(2), None, Some(3)];
        assert_eq!(
            window.sum_frame(&values),
            vec![Some(1), Some(3), Some(3), Some(6)]
        );

        // Marco movil de 1 anterior a 1 siguiente: NULL si no hay valores.
        let moving = RqlWindowFuncs::with_frame(Frame {
            start: Bound::Preceding(1),
            end: Bound::Following(1),
        });
        assert_eq!(
            moving.sum_frame(&[Some(10), None, Some(30)]),
            vec![Some(10), Some(40), Some(30)]
        );
        assert_eq!(
            moving.avg_frame(&[Some(2), Some(4)]),
            vec![Some(3.0), Some(3.0)]
        );
        assert_eq!(moving.count_frame(&[Some(1), None, Some(3)]), vec![1, 2, 1]);
    }

    /// AC-0165-03: `LAG`/`LEAD` con desplazamiento.
    #[test]
    // @spec AC-0165-03
    fn test_ac_0165_03_lag() {
        let window = RqlWindowFuncs::new();
        assert_eq!(window.lag(&[10, 20, 30], 1), vec![None, Some(10), Some(20)]);
        assert_eq!(window.lead(&[10, 20, 30], 2), vec![Some(30), None, None]);
        assert_eq!(window.lag(&[1], 0), vec![Some(1)]);
    }

    /// Marco vacio (`start > end`) devuelve NULL, no 0.
    #[test]
    fn empty_frame_is_null() {
        // `ROWS BETWEEN 1 FOLLOWING AND CURRENT ROW` solo es valido en la
        // ultima fila (start > end en el resto).
        let window = RqlWindowFuncs::with_frame(Frame {
            start: Bound::Following(1),
            end: Bound::CurrentRow,
        });
        assert_eq!(
            window.sum_frame(&[Some(1), Some(2)]),
            vec![None, Some(2)],
            "solo la ultima fila tiene marco no vacio"
        );
        assert_eq!(window.count_frame(&[Some(1), Some(2)]), vec![0, 1]);
    }
}
