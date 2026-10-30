//! Subconsultas RQL: escalar, `EXISTS` e `IN` (SPEC-0162).
//!
//! Una subconsulta escalar debe devolver **como maximo una fila**; si devuelve
//! mas, SQL-92 exige error en tiempo de ejecucion (no se toma la primera en
//! silencio). `IN` usa logica trivaluada: si no hay coincidencia pero la
//! subconsulta contiene NULL, el resultado es NULL, no `false`.

/// Error de evaluacion de una subconsulta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubqueryError {
    /// La subconsulta escalar devolvio mas de una fila.
    MultipleRows(usize),
}

impl std::fmt::Display for SubqueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubqueryError::MultipleRows(rows) => write!(
                formatter,
                "subconsulta escalar devolvio {rows} filas (maximo 1)"
            ),
        }
    }
}

impl std::error::Error for SubqueryError {}

/// Tipo de subconsulta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubqueryKind {
    /// Devuelve un unico valor.
    Scalar,
    /// Devuelve un booleano de existencia.
    Exists,
    /// Compara un valor contra el conjunto.
    In,
}

/// Motor de subconsultas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RqlSubquery {
    /// Limite defensivo de filas inspeccionadas (0 = sin limite).
    pub max_rows: usize,
}

impl RqlSubquery {
    /// Crea el motor.
    ///
    /// Returns:
    ///     Motor sin limite de filas.
    pub fn new() -> Self {
        Self::default()
    }

    /// Crea el motor con limite de filas.
    ///
    /// Args:
    ///     max_rows: Maximo de filas a inspeccionar (0 = sin limite).
    ///
    /// Returns:
    ///     Motor configurado.
    pub fn with_limit(max_rows: usize) -> Self {
        Self { max_rows }
    }

    /// Subconsulta escalar sobre una columna de enteros.
    ///
    /// Args:
    ///     rows: Primera columna de la subconsulta.
    ///
    /// Returns:
    ///     `Ok(Some(v))` con una fila, `Ok(None)` sin filas.
    ///
    /// Errors:
    ///     [`SubqueryError::MultipleRows`] si hay mas de una fila.
    pub fn scalar(&self, rows: &[i64]) -> Result<Option<i64>, SubqueryError> {
        match rows {
            [] => Ok(None),
            [only] => Ok(Some(*only)),
            many => Err(SubqueryError::MultipleRows(many.len())),
        }
    }

    /// `EXISTS (subconsulta)`.
    ///
    /// Args:
    ///     rows: Filas de la subconsulta.
    ///
    /// Returns:
    ///     `true` si hay al menos una fila.
    pub fn exists(&self, rows: &[i64]) -> bool {
        !rows.is_empty()
    }

    /// `v IN (subconsulta)` sobre enteros, sin NULL.
    ///
    /// Args:
    ///     v: Valor buscado.
    ///     rows: Conjunto de la subconsulta.
    ///
    /// Returns:
    ///     `true` si `v` pertenece al conjunto.
    pub fn in_subquery(&self, v: i64, rows: &[i64]) -> bool {
        rows.contains(&v)
    }

    /// Evalua una subconsulta de forma uniforme (enteros, sin NULL).
    ///
    /// Args:
    ///     kind: Tipo de subconsulta.
    ///     value: Valor de comparacion (ignorado por `Exists`).
    ///     rows: Filas de la subconsulta.
    ///
    /// Returns:
    ///     `Some(valor)` para escalar, `Some(1/0)` para `Exists`, `Some(1/0)`
    ///     para `In`.
    ///
    /// Errors:
    ///     [`SubqueryError::MultipleRows`] en escalar con mas de una fila.
    pub fn eval(
        &self,
        kind: SubqueryKind,
        value: Option<i64>,
        rows: &[i64],
    ) -> Result<Option<i64>, SubqueryError> {
        let limited = self.limit(rows);
        match kind {
            SubqueryKind::Scalar => self.scalar(limited),
            SubqueryKind::Exists => Ok(Some(i64::from(self.exists(limited)))),
            SubqueryKind::In => {
                let matched = value.is_some_and(|needle| self.in_subquery(needle, limited));
                Ok(Some(i64::from(matched)))
            }
        }
    }

    /// Evalua una subconsulta con celdas anulables y logica trivaluada.
    ///
    /// Args:
    ///     kind: Tipo de subconsulta.
    ///     value: Valor de comparacion (`None` = NULL).
    ///     rows: Filas de la subconsulta (cada celda es `Option<i64>`).
    ///
    /// Returns:
    ///     `None` representa NULL SQL.
    ///
    /// Errors:
    ///     [`SubqueryError::MultipleRows`] en escalar con mas de una fila.
    pub fn eval_cells(
        &self,
        kind: SubqueryKind,
        value: Option<i64>,
        rows: &[Vec<Option<i64>>],
    ) -> Result<Option<bool>, SubqueryError> {
        let limited = if self.max_rows == 0 {
            rows
        } else {
            &rows[..rows.len().min(self.max_rows)]
        };
        match kind {
            SubqueryKind::Scalar => match limited {
                [] => Ok(None),
                [single] => Ok(single.first().copied().flatten().map(|_| true)),
                many => Err(SubqueryError::MultipleRows(many.len())),
            },
            SubqueryKind::Exists => Ok(Some(!limited.is_empty())),
            SubqueryKind::In => {
                let mut saw_null = false;
                let mut matched = false;
                for row in limited {
                    match row.first().copied().flatten() {
                        Some(candidate) if Some(candidate) == value => {
                            matched = true;
                            break;
                        }
                        Some(_) => {}
                        None => saw_null = true,
                    }
                }
                if matched {
                    Ok(Some(true))
                } else if saw_null {
                    Ok(None)
                } else {
                    Ok(Some(false))
                }
            }
        }
    }

    /// Aplica el limite de filas.
    fn limit<'a>(&self, rows: &'a [i64]) -> &'a [i64] {
        if self.max_rows == 0 {
            rows
        } else {
            &rows[..rows.len().min(self.max_rows)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RqlSubquery, SubqueryError, SubqueryKind};

    /// AC-0162-01: escalar devuelve una fila o `None`, y falla con varias.
    #[test]
    // @spec AC-0162-01
    fn test_ac_0162_01_scalar() {
        let subquery = RqlSubquery::new();
        assert_eq!(subquery.scalar(&[42]), Ok(Some(42)));
        assert_eq!(subquery.scalar(&[]), Ok(None));
        assert_eq!(
            subquery.scalar(&[42, 7]),
            Err(SubqueryError::MultipleRows(2))
        );
    }

    /// AC-0162-02: `IN` con y sin coincidencia, y NULL trivaluado.
    #[test]
    // @spec AC-0162-02
    fn test_ac_0162_02_in() {
        let subquery = RqlSubquery::new();
        assert!(subquery.in_subquery(2, &[1, 2, 3]));
        assert!(!subquery.in_subquery(9, &[1, 2, 3]));

        let cells = vec![vec![Some(1)], vec![Some(2)]];
        assert_eq!(
            subquery.eval_cells(SubqueryKind::In, Some(2), &cells),
            Ok(Some(true))
        );
        assert_eq!(
            subquery.eval_cells(SubqueryKind::In, Some(9), &cells),
            Ok(Some(false))
        );
        let with_null = vec![vec![Some(1)], vec![None]];
        assert_eq!(
            subquery.eval_cells(SubqueryKind::In, Some(9), &with_null),
            Ok(None)
        );
    }

    /// AC-0162-03: `EXISTS` distingue conjunto vacio de no vacio.
    #[test]
    // @spec AC-0162-03
    fn test_ac_0162_03_exists() {
        let subquery = RqlSubquery::new();
        assert!(subquery.exists(&[1]));
        assert!(!subquery.exists(&[]));
        assert_eq!(subquery.eval(SubqueryKind::Exists, None, &[0]), Ok(Some(1)));
    }

    /// AC-0162-04: subconsulta derivada (tabla) y limite defensivo.
    #[test]
    // @spec AC-0162-04
    fn test_ac_0162_04_derived() {
        let subquery = RqlSubquery::with_limit(2);
        let rows = vec![vec![Some(10)], vec![Some(20)], vec![Some(30)]];
        assert_eq!(
            subquery.eval_cells(SubqueryKind::Exists, None, &rows),
            Ok(Some(true))
        );
        // El limite corta la inspeccion: 30 queda fuera.
        assert_eq!(
            subquery.eval_cells(SubqueryKind::In, Some(30), &rows),
            Ok(Some(false))
        );
        assert_eq!(subquery.scalar(&[5]), Ok(Some(5)));
    }
}
