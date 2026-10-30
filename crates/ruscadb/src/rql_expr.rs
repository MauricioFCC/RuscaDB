//! Motor de expresiones RQL con logica trivaluada SQL (SPEC-0161).
//!
//! Implementa el arbol de expresiones y su evaluacion sobre una fila
//! (`BTreeMap` columna -> valor) respetando la semantica SQL-92 de NULL:
//! cualquier comparacion con NULL produce NULL, y `AND`/`OR`/`NOT` usan las
//! tablas de verdad de tres valores (no se colapsa NULL a `false`).
//!
//! Operadores cubiertos: `AND`, `OR`, `NOT`, comparaciones, `IN`, `BETWEEN`,
//! `IS [NOT] NULL`, `COALESCE`, `CASE WHEN`, aritmetica basica y columnas.

use std::collections::BTreeMap;

/// Valor de una celda/expresion.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Ausencia de valor (NULL SQL).
    Null,
    /// Entero de 64 bits.
    Int(i64),
    /// Coma flotante.
    Float(f64),
    /// Texto UTF-8.
    Text(String),
    /// Booleano.
    Bool(bool),
}

impl Value {
    /// Interpreta el valor como booleano SQL.
    ///
    /// Returns:
    ///     `None` para NULL; `Some(b)` en otro caso (0/vacio = falso).
    pub fn truth(&self) -> Option<bool> {
        match self {
            Value::Null => None,
            Value::Bool(b) => Some(*b),
            Value::Int(v) => Some(*v != 0),
            Value::Float(v) => Some(*v != 0.0),
            Value::Text(t) => Some(!t.is_empty()),
        }
    }

    /// Interpreta el valor como numero.
    ///
    /// Returns:
    ///     `None` si es NULL, texto o booleano.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            _ => None,
        }
    }
}

/// Operador de comparacion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// Operador aritmetico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
}

/// Expresion RQL.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Literal.
    Literal(Value),
    /// Referencia a columna.
    Column(String),
    /// `NOT e`
    Not(Box<Expr>),
    /// `a AND b`
    And(Box<Expr>, Box<Expr>),
    /// `a OR b`
    Or(Box<Expr>, Box<Expr>),
    /// Comparacion binaria.
    Compare(CmpOp, Box<Expr>, Box<Expr>),
    /// Aritmetica binaria.
    Arithmetic(ArithOp, Box<Expr>, Box<Expr>),
    /// `e IN (v1, v2, ...)`
    In(Box<Expr>, Vec<Expr>),
    /// `e BETWEEN lo AND hi`
    Between(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `e IS NULL` (`negated = false`) o `e IS NOT NULL`.
    IsNull {
        /// Expresion comprobada.
        expr: Box<Expr>,
        /// `true` para `IS NOT NULL`.
        negated: bool,
    },
    /// `COALESCE(a, b, ...)`
    Coalesce(Vec<Expr>),
    /// `CASE WHEN c1 THEN r1 ... ELSE r END`
    Case {
        /// Pares condicion/resultado.
        whens: Vec<(Expr, Expr)>,
        /// Resultado por defecto.
        otherwise: Option<Box<Expr>>,
    },
}

/// Error de evaluacion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    /// Columna inexistente en la fila.
    UnknownColumn(String),
    /// Profundidad de recursion superada.
    DepthExceeded,
    /// Division por cero.
    DivisionByZero,
}

/// Motor de evaluacion de expresiones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RqlExprEngine {
    /// Profundidad maxima de recursion (defensa ante expresiones patologicas).
    pub max_depth: usize,
}

impl Default for RqlExprEngine {
    fn default() -> Self {
        Self { max_depth: 128 }
    }
}

impl RqlExprEngine {
    /// Crea el motor con la profundidad por defecto.
    ///
    /// Returns:
    ///     Motor listo para evaluar.
    pub fn new() -> Self {
        Self::default()
    }

    /// `AND` trivaluado.
    ///
    /// Args:
    ///     a: Operando izquierdo.
    ///     b: Operando derecho.
    ///
    /// Returns:
    ///     `false` si algun operando es falso; `true` si ambos son ciertos;
    ///     `None` (NULL) en otro caso.
    pub fn and3(&self, a: Option<bool>, b: Option<bool>) -> Option<bool> {
        match (a, b) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        }
    }

    /// `OR` trivaluado.
    ///
    /// Args:
    ///     a: Operando izquierdo.
    ///     b: Operando derecho.
    ///
    /// Returns:
    ///     `true` si algun operando es cierto; `false` si ambos son falsos;
    ///     `None` (NULL) en otro caso.
    pub fn or3(&self, a: Option<bool>, b: Option<bool>) -> Option<bool> {
        match (a, b) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        }
    }

    /// `NOT` trivaluado.
    ///
    /// Args:
    ///     a: Operando.
    ///
    /// Returns:
    ///     Negacion, o `None` si el operando es NULL.
    pub fn not3(&self, a: Option<bool>) -> Option<bool> {
        a.map(|value| !value)
    }

    /// Compatibilidad booleana estricta (sin NULL).
    ///
    /// Args:
    ///     a: Operando izquierdo.
    ///     b: Operando derecho.
    ///
    /// Returns:
    ///     `a OR b`.
    pub fn eval_or(&self, a: bool, b: bool) -> bool {
        a || b
    }

    /// Compatibilidad booleana estricta (sin NULL).
    ///
    /// Args:
    ///     a: Operando izquierdo.
    ///     b: Operando derecho.
    ///
    /// Returns:
    ///     `a AND b`.
    pub fn eval_and(&self, a: bool, b: bool) -> bool {
        a && b
    }

    /// Compatibilidad booleana estricta (sin NULL).
    ///
    /// Args:
    ///     a: Operando.
    ///
    /// Returns:
    ///     `NOT a`.
    pub fn eval_not(&self, a: bool) -> bool {
        !a
    }

    /// `v IN (lista)` con semantica trivaluada.
    ///
    /// Args:
    ///     v: Valor buscado.
    ///     list: Valores candidatos.
    ///
    /// Returns:
    ///     `true` si hay coincidencia; `false` si no la hay y no hay NULL en la
    ///     lista; `None` si no hay coincidencia pero algun candidato es NULL.
    pub fn in_list3(&self, v: &Value, list: &[Value]) -> Option<bool> {
        if list.contains(v) {
            return Some(true);
        }
        if list.contains(&Value::Null) {
            None
        } else {
            Some(false)
        }
    }

    /// Compatibilidad: `IN` sobre enteros, sin NULL.
    ///
    /// Args:
    ///     v: Valor buscado.
    ///     list: Candidatos.
    ///
    /// Returns:
    ///     `true` si `v` esta en la lista.
    pub fn in_list(&self, v: i64, list: &[i64]) -> bool {
        list.contains(&v)
    }

    /// `BETWEEN lo AND hi` inclusivo.
    ///
    /// Args:
    ///     v: Valor.
    ///     lo: Limite inferior.
    ///     hi: Limite superior.
    ///
    /// Returns:
    ///     `true` si `lo <= v <= hi`.
    pub fn between(&self, v: i64, lo: i64, hi: i64) -> bool {
        v >= lo && v <= hi
    }

    /// Primer valor no nulo.
    ///
    /// Args:
    ///     vals: Candidatos en orden.
    ///
    /// Returns:
    ///     Primer `Some`, o `None` si todos son nulos/vacios.
    pub fn coalesce(&self, vals: &[Option<i64>]) -> Option<i64> {
        vals.iter().find_map(|value| *value)
    }

    /// `COALESCE` sobre valores SQL.
    ///
    /// Args:
    ///     vals: Candidatos en orden.
    ///
    /// Returns:
    ///     Primer valor distinto de NULL, o `Value::Null`.
    pub fn coalesce_values(&self, vals: &[Value]) -> Value {
        vals.iter()
            .find(|value| **value != Value::Null)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// Evalua una expresion sobre una fila.
    ///
    /// Args:
    ///     expr: Expresion a evaluar.
    ///     row: Mapa columna -> valor.
    ///
    /// Returns:
    ///     Valor resultante (`Null` propaga).
    ///
    /// Errors:
    ///     [`ExprError::UnknownColumn`] si falta una columna;
    ///     [`ExprError::DepthExceeded`] si se supera `max_depth`;
    ///     [`ExprError::DivisionByZero`] en division entera por cero.
    pub fn eval(&self, expr: &Expr, row: &BTreeMap<String, Value>) -> Result<Value, ExprError> {
        self.eval_at(expr, row, 0)
    }

    /// Evaluacion recursiva con contador de profundidad.
    fn eval_at(
        &self,
        expr: &Expr,
        row: &BTreeMap<String, Value>,
        depth: usize,
    ) -> Result<Value, ExprError> {
        if depth > self.max_depth {
            return Err(ExprError::DepthExceeded);
        }
        let next = depth + 1;
        match expr {
            Expr::Literal(value) => Ok(value.clone()),
            Expr::Column(name) => row
                .get(name)
                .cloned()
                .ok_or_else(|| ExprError::UnknownColumn(name.clone())),
            Expr::Not(inner) => {
                let value = self.eval_at(inner, row, next)?;
                Ok(match value.truth() {
                    Some(truth) => Value::Bool(!truth),
                    None => Value::Null,
                })
            }
            Expr::And(a, b) => {
                let left = self.eval_at(a, row, next)?.truth();
                // Cortocircuito SQL: false AND NULL = false.
                if left == Some(false) {
                    return Ok(Value::Bool(false));
                }
                let right = self.eval_at(b, row, next)?.truth();
                Ok(match self.and3(left, right) {
                    Some(truth) => Value::Bool(truth),
                    None => Value::Null,
                })
            }
            Expr::Or(a, b) => {
                let left = self.eval_at(a, row, next)?.truth();
                if left == Some(true) {
                    return Ok(Value::Bool(true));
                }
                let right = self.eval_at(b, row, next)?.truth();
                Ok(match self.or3(left, right) {
                    Some(truth) => Value::Bool(truth),
                    None => Value::Null,
                })
            }
            Expr::Compare(op, a, b) => {
                let left = self.eval_at(a, row, next)?;
                let right = self.eval_at(b, row, next)?;
                Ok(self.compare(*op, &left, &right))
            }
            Expr::Arithmetic(op, a, b) => {
                let left = self.eval_at(a, row, next)?;
                let right = self.eval_at(b, row, next)?;
                self.arithmetic(*op, &left, &right)
            }
            Expr::In(target, options) => {
                let value = self.eval_at(target, row, next)?;
                let mut candidates = Vec::with_capacity(options.len());
                for option in options {
                    candidates.push(self.eval_at(option, row, next)?);
                }
                Ok(match self.in_list3(&value, &candidates) {
                    Some(truth) => Value::Bool(truth),
                    None => Value::Null,
                })
            }
            Expr::Between(target, lo, hi) => {
                let value = self.eval_at(target, row, next)?;
                let low = self.eval_at(lo, row, next)?;
                let high = self.eval_at(hi, row, next)?;
                let lower = self.compare(CmpOp::Ge, &value, &low);
                let upper = self.compare(CmpOp::Le, &value, &high);
                let combined = self.and3(lower.truth(), upper.truth());
                Ok(match combined {
                    Some(truth) => Value::Bool(truth),
                    None => Value::Null,
                })
            }
            Expr::IsNull { expr, negated } => {
                let value = self.eval_at(expr, row, next)?;
                let is_null = value == Value::Null;
                Ok(Value::Bool(if *negated { !is_null } else { is_null }))
            }
            Expr::Coalesce(options) => {
                for option in options {
                    let value = self.eval_at(option, row, next)?;
                    if value != Value::Null {
                        return Ok(value);
                    }
                }
                Ok(Value::Null)
            }
            Expr::Case { whens, otherwise } => {
                for (condition, result) in whens {
                    if self.eval_at(condition, row, next)?.truth() == Some(true) {
                        return self.eval_at(result, row, next);
                    }
                }
                match otherwise {
                    Some(fallback) => self.eval_at(fallback, row, next),
                    None => Ok(Value::Null),
                }
            }
        }
    }

    /// Compara dos valores con semantica SQL (NULL propaga).
    fn compare(&self, op: CmpOp, left: &Value, right: &Value) -> Value {
        if *left == Value::Null || *right == Value::Null {
            return Value::Null;
        }
        let ordering = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Value::Text(a), Value::Text(b)) => a.partial_cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            _ => match (left.as_f64(), right.as_f64()) {
                (Some(a), Some(b)) => a.partial_cmp(&b),
                _ => None,
            },
        };
        let Some(ordering) = ordering else {
            return Value::Null;
        };
        let result = match op {
            CmpOp::Eq => ordering.is_eq(),
            CmpOp::Ne => ordering.is_ne(),
            CmpOp::Lt => ordering.is_lt(),
            CmpOp::Le => ordering.is_le(),
            CmpOp::Gt => ordering.is_gt(),
            CmpOp::Ge => ordering.is_ge(),
        };
        Value::Bool(result)
    }

    /// Aritmetica con promocion a `f64` y NULL propagado.
    fn arithmetic(&self, op: ArithOp, left: &Value, right: &Value) -> Result<Value, ExprError> {
        if *left == Value::Null || *right == Value::Null {
            return Ok(Value::Null);
        }
        if let (Value::Int(a), Value::Int(b)) = (left, right) {
            return match op {
                ArithOp::Add => Ok(Value::Int(a.wrapping_add(*b))),
                ArithOp::Sub => Ok(Value::Int(a.wrapping_sub(*b))),
                ArithOp::Mul => Ok(Value::Int(a.wrapping_mul(*b))),
                ArithOp::Div => {
                    if *b == 0 {
                        Err(ExprError::DivisionByZero)
                    } else {
                        Ok(Value::Int(a / b))
                    }
                }
            };
        }
        let (Some(a), Some(b)) = (left.as_f64(), right.as_f64()) else {
            return Ok(Value::Null);
        };
        Ok(Value::Float(match op {
            ArithOp::Add => a + b,
            ArithOp::Sub => a - b,
            ArithOp::Mul => a * b,
            ArithOp::Div => a / b,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{CmpOp, Expr, ExprError, RqlExprEngine, Value};
    use std::collections::BTreeMap;

    fn row() -> BTreeMap<String, Value> {
        let mut row = BTreeMap::new();
        row.insert("a".to_string(), Value::Int(5));
        row.insert("b".to_string(), Value::Int(10));
        row.insert("n".to_string(), Value::Null);
        row
    }

    /// AC-0161-01: precedencia AND > OR y logica trivaluada.
    #[test]
    // @spec AC-0161-01
    fn test_ac_0161_01_precedence() {
        let engine = RqlExprEngine::new();
        assert!(engine.eval_or(engine.eval_and(true, false), engine.eval_not(false)));
        assert_eq!(engine.and3(Some(false), None), Some(false));
        assert_eq!(engine.or3(Some(true), None), Some(true));
        assert_eq!(engine.and3(Some(true), None), None);
        assert_eq!(engine.not3(None), None);

        // `true OR (NULL AND false)` = true; `false OR (true AND NULL)` = NULL.
        assert_eq!(engine.and3(Some(true), None), None);
        assert_eq!(engine.or3(Some(false), None), None);
    }

    /// AC-0161-02: `IN` con coincidencia, sin coincidencia y con NULL.
    #[test]
    // @spec AC-0161-02
    fn test_ac_0161_02_in_list() {
        let engine = RqlExprEngine::new();
        assert!(engine.in_list(2, &[1, 2, 3]));
        assert_eq!(
            engine.in_list3(&Value::Int(9), &[Value::Int(1)]),
            Some(false)
        );
        assert_eq!(
            engine.in_list3(&Value::Int(9), &[Value::Int(1), Value::Null]),
            None
        );
        assert_eq!(
            engine.in_list3(&Value::Int(1), &[Value::Int(1), Value::Null]),
            Some(true)
        );
    }

    /// AC-0161-03: `BETWEEN` inclusivo y con NULL.
    #[test]
    // @spec AC-0161-03
    fn test_ac_0161_03_between() {
        let engine = RqlExprEngine::new();
        assert!(engine.between(5, 1, 10));
        assert!(engine.between(1, 1, 10), "limite inferior inclusivo");
        assert!(!engine.between(11, 1, 10));

        let expression = Expr::Between(
            Box::new(Expr::Column("a".into())),
            Box::new(Expr::Literal(Value::Int(1))),
            Box::new(Expr::Literal(Value::Int(10))),
        );
        assert_eq!(engine.eval(&expression, &row()), Ok(Value::Bool(true)));

        let with_null = Expr::Between(
            Box::new(Expr::Column("n".into())),
            Box::new(Expr::Literal(Value::Int(1))),
            Box::new(Expr::Literal(Value::Int(10))),
        );
        assert_eq!(engine.eval(&with_null, &row()), Ok(Value::Null));
    }

    /// AC-0161-04: `CASE WHEN` elige la primera condicion cierta.
    #[test]
    // @spec AC-0161-04
    fn test_ac_0161_04_case() {
        let engine = RqlExprEngine::new();
        let expression = Expr::Case {
            whens: vec![
                (
                    Expr::Compare(
                        CmpOp::Gt,
                        Box::new(Expr::Column("a".into())),
                        Box::new(Expr::Literal(Value::Int(100))),
                    ),
                    Expr::Literal(Value::Text("alto".into())),
                ),
                (
                    Expr::Compare(
                        CmpOp::Gt,
                        Box::new(Expr::Column("a".into())),
                        Box::new(Expr::Literal(Value::Int(1))),
                    ),
                    Expr::Literal(Value::Text("medio".into())),
                ),
            ],
            otherwise: Some(Box::new(Expr::Literal(Value::Text("bajo".into())))),
        };
        assert_eq!(
            engine.eval(&expression, &row()),
            Ok(Value::Text("medio".into()))
        );
        // Sin WHEN cumplido y sin ELSE -> NULL.
        let empty = Expr::Case {
            whens: vec![],
            otherwise: None,
        };
        assert_eq!(engine.eval(&empty, &row()), Ok(Value::Null));
    }

    /// AC-0161-05: `COALESCE` devuelve el primer no nulo.
    #[test]
    // @spec AC-0161-05
    fn test_ac_0161_05_coalesce() {
        let engine = RqlExprEngine::new();
        assert_eq!(engine.coalesce(&[Some(1), Some(2)]), Some(1));
        assert_eq!(engine.coalesce(&[None, None]), None);
        assert_eq!(
            engine.coalesce_values(&[Value::Null, Value::Int(7)]),
            Value::Int(7)
        );

        let expression = Expr::Coalesce(vec![
            Expr::Column("n".into()),
            Expr::Literal(Value::Int(42)),
        ]);
        assert_eq!(engine.eval(&expression, &row()), Ok(Value::Int(42)));
    }

    /// Errores accionables: columna inexistente, division por cero, profundidad.
    #[test]
    fn errors_are_actionable() {
        let engine = RqlExprEngine::new();
        let missing = Expr::Column("zzz".into());
        assert_eq!(
            engine.eval(&missing, &row()),
            Err(ExprError::UnknownColumn("zzz".into()))
        );

        let division = Expr::Arithmetic(
            super::ArithOp::Div,
            Box::new(Expr::Literal(Value::Int(1))),
            Box::new(Expr::Literal(Value::Int(0))),
        );
        assert_eq!(
            engine.eval(&division, &row()),
            Err(ExprError::DivisionByZero)
        );

        // Expresion profundamente anidada: corta antes de desbordar la pila.
        let mut deep = Expr::Literal(Value::Int(1));
        for _ in 0..200 {
            deep = Expr::Not(Box::new(deep));
        }
        assert_eq!(engine.eval(&deep, &row()), Err(ExprError::DepthExceeded));
    }

    /// Ramas de `IS [NOT] NULL`, comparacion de texto/bool y tipos incompatibles.
    #[test]
    fn null_text_and_bool_branches() {
        let engine = RqlExprEngine::new();
        let mut row = row();
        row.insert("t".to_string(), Value::Text("b".to_string()));
        row.insert("f".to_string(), Value::Bool(true));

        let is_null = Expr::IsNull {
            expr: Box::new(Expr::Column("n".into())),
            negated: false,
        };
        assert_eq!(engine.eval(&is_null, &row), Ok(Value::Bool(true)));
        let is_not_null = Expr::IsNull {
            expr: Box::new(Expr::Column("a".into())),
            negated: true,
        };
        assert_eq!(engine.eval(&is_not_null, &row), Ok(Value::Bool(true)));

        let text_cmp = Expr::Compare(
            CmpOp::Lt,
            Box::new(Expr::Column("t".into())),
            Box::new(Expr::Literal(Value::Text("c".into()))),
        );
        assert_eq!(engine.eval(&text_cmp, &row), Ok(Value::Bool(true)));

        let bool_cmp = Expr::Compare(
            CmpOp::Eq,
            Box::new(Expr::Column("f".into())),
            Box::new(Expr::Literal(Value::Bool(true))),
        );
        assert_eq!(engine.eval(&bool_cmp, &row), Ok(Value::Bool(true)));

        // Tipos incompatibles (texto vs numero) -> NULL, no panic.
        let incompatible = Expr::Compare(
            CmpOp::Eq,
            Box::new(Expr::Column("t".into())),
            Box::new(Expr::Literal(Value::Int(1))),
        );
        assert_eq!(engine.eval(&incompatible, &row), Ok(Value::Null));
    }

    /// Aritmetica: promocion a float y division real por cero (no error).
    #[test]
    fn arithmetic_promotes_and_handles_float_zero() {
        let engine = RqlExprEngine::new();
        let mixed = Expr::Arithmetic(
            super::ArithOp::Mul,
            Box::new(Expr::Literal(Value::Int(2))),
            Box::new(Expr::Literal(Value::Float(1.5))),
        );
        assert_eq!(engine.eval(&mixed, &row()), Ok(Value::Float(3.0)));

        let float_zero = Expr::Arithmetic(
            super::ArithOp::Div,
            Box::new(Expr::Literal(Value::Float(1.0))),
            Box::new(Expr::Literal(Value::Float(0.0))),
        );
        assert_eq!(
            engine.eval(&float_zero, &row()),
            Ok(Value::Float(f64::INFINITY))
        );

        let null_arith = Expr::Arithmetic(
            super::ArithOp::Add,
            Box::new(Expr::Column("n".into())),
            Box::new(Expr::Literal(Value::Int(1))),
        );
        assert_eq!(engine.eval(&null_arith, &row()), Ok(Value::Null));
    }

    /// Cortocircuito SQL: `false AND NULL = false`, `true OR NULL = true`.
    #[test]
    fn boolean_short_circuit_with_null() {
        let engine = RqlExprEngine::new();
        let false_and_null = Expr::And(
            Box::new(Expr::Literal(Value::Bool(false))),
            Box::new(Expr::Column("n".into())),
        );
        assert_eq!(engine.eval(&false_and_null, &row()), Ok(Value::Bool(false)));
        let true_or_null = Expr::Or(
            Box::new(Expr::Literal(Value::Bool(true))),
            Box::new(Expr::Column("n".into())),
        );
        assert_eq!(engine.eval(&true_or_null, &row()), Ok(Value::Bool(true)));
        let not_null = Expr::Not(Box::new(Expr::Column("n".into())));
        assert_eq!(engine.eval(&not_null, &row()), Ok(Value::Null));
    }
}
