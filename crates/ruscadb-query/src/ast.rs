//! IR (representación intermedia) de una consulta RQL.

use std::fmt;

/// Proyección de columnas de un `SELECT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Projection {
    /// `SELECT *`.
    All,
    /// `SELECT a, b, ...`.
    Columns(Vec<String>),
}

/// Operador de comparación.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    /// `=`
    Eq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    LtEq,
    /// `>`
    Gt,
    /// `>=`
    GtEq,
}

impl CompareOp {
    /// Símbolo textual del operador.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::LtEq => "<=",
            Self::Gt => ">",
            Self::GtEq => ">=",
        }
    }
}

/// Expresión del filtro `WHERE`.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Referencia a una columna.
    Column(String),
    /// Literal entero.
    Int(i64),
    /// Literal flotante.
    Float(f64),
    /// Literal de texto.
    Text(String),
    /// Comparación binaria.
    Compare {
        /// Lado izquierdo.
        left: Box<Expr>,
        /// Operador.
        op: CompareOp,
        /// Lado derecho.
        right: Box<Expr>,
    },
    /// Conjunción lógica.
    And(Box<Expr>, Box<Expr>),
}

/// Sentencia `SELECT` analizada.
#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    /// Columnas proyectadas.
    pub projection: Projection,
    /// Tabla de origen.
    pub from: String,
    /// Filtro `WHERE` (opcional).
    pub filter: Option<Expr>,
    /// Límite `LIMIT` (opcional).
    pub limit: Option<u64>,
}

impl fmt::Display for Projection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(formatter, "*"),
            Self::Columns(columns) => write!(formatter, "{}", columns.join(", ")),
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Column(name) => write!(formatter, "{name}"),
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Float(value) => {
                if value.fract() == 0.0 {
                    write!(formatter, "{value:.1}")
                } else {
                    write!(formatter, "{value}")
                }
            }
            Self::Text(text) => write!(formatter, "'{text}'"),
            Self::Compare { left, op, right } => {
                write!(formatter, "{left} {} {right}", op.as_str())
            }
            Self::And(left, right) => write!(formatter, "{left} AND {right}"),
        }
    }
}

impl fmt::Display for Select {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SELECT {} FROM {}", self.projection, self.from)?;
        if let Some(filter) = &self.filter {
            write!(formatter, " WHERE {filter}")?;
        }
        if let Some(limit) = self.limit {
            write!(formatter, " LIMIT {limit}")?;
        }
        Ok(())
    }
}
