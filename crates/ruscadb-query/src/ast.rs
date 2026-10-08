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

/// Función de agregación soportada por RQL.
///
/// `CountStar` es `COUNT(*)` (cuenta filas, incluidas las de columnas `NULL`);
/// el resto ignora los `NULL` (semántica SQL:2016). La estrategia de ejecución
/// es *hash aggregation* (como DuckDB/DataFusion), no *sort-based*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggFunc {
    /// `COUNT(*)`: cuenta todas las filas del grupo.
    CountStar,
    /// `COUNT(columna)`: cuenta valores no `NULL`.
    Count,
    /// `SUM(columna)`: suma valores numéricos no `NULL`.
    Sum,
    /// `AVG(columna)`: media de valores numéricos no `NULL`.
    Avg,
    /// `MIN(columna)`: mínimo de valores no `NULL`.
    Min,
    /// `MAX(columna)`: máximo de valores no `NULL`.
    Max,
}

impl AggFunc {
    /// Texto canónico de la función.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CountStar | Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }
}

/// Agregado de la proyección (p. ej. `COUNT(*)`, `SUM(a) AS total`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Aggregate {
    /// Función de agregación.
    pub func: AggFunc,
    /// Columna de entrada (`None` solo para `COUNT(*)`).
    pub column: Option<String>,
    /// Alias opcional (`AS nombre`); si falta, el executor deriva el nombre.
    pub alias: Option<String>,
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
    /// Búsqueda full-text `MATCH(columna, 'texto')` dentro del `WHERE`.
    Match {
        /// Columna de texto indexada.
        column: String,
        /// Texto de la consulta (se tokeniza al buscar).
        query: String,
    },
}

/// Cláusula `KNN <columna> <|k|> [v1, v2, ...]` de búsqueda de vecinos.
#[derive(Clone, Debug, PartialEq)]
pub struct KnnClause {
    /// Columna de embedding sobre la que buscar.
    pub column: String,
    /// Número de vecinos a recuperar.
    pub k: u64,
    /// Vector de consulta (puede ser vacío).
    pub query: Vec<f64>,
}

/// Cláusula `TRAVERSE <columna> DEPTH <n>` de recorrido de grafo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraverseClause {
    /// Columna de aristas sobre la que recorrer.
    pub column: String,
    /// Profundidad máxima del recorrido.
    pub depth: u16,
}

/// Cláusula `ORDER BY <columna> [ASC|DESC]` de ordenamiento de filas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderBy {
    /// Columna de ordenamiento.
    pub column: String,
    /// `true` para orden descendente; `false` para ascendente.
    pub desc: bool,
}

/// Sentencia `SELECT` analizada.
#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    /// Columnas proyectadas.
    pub projection: Projection,
    /// Agregados de la proyección (`COUNT(*)`, `SUM(a)`, ...).
    ///
    /// Se mantienen separados de [`Projection`] para no romper la variante
    /// pública `Projection::Columns` (consumida por adaptadores como
    /// `ruscadb-wasm`); el orden canónico los serializa tras las columnas.
    pub aggregates: Vec<Aggregate>,
    /// Tabla de origen.
    pub from: String,
    /// Filtro `WHERE` (opcional).
    pub filter: Option<Expr>,
    /// Cláusula `KNN` (opcional).
    pub knn: Option<KnnClause>,
    /// Cláusula `TRAVERSE` (opcional).
    pub traverse: Option<TraverseClause>,
    /// Columnas de `GROUP BY` (vacío si no hay agrupación).
    pub group_by: Vec<String>,
    /// Cláusula `ORDER BY` (opcional).
    pub order_by: Option<OrderBy>,
    /// Límite `LIMIT` (opcional).
    pub limit: Option<u64>,
}

/// Sentencia `EXPLAIN <select>`: envuelve el IR para inspeccionar el plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Explain {
    /// `SELECT` envuelto.
    pub inner: Box<Select>,
}

/// Sentencia RQL de nivel superior.
///
/// Se permite `large_enum_variant`: `Select` es la variante dominante (la otra
/// solo guarda un `Box`), y envolverla cambiaría el patrón público
/// `Statement::Select` consumido por adaptadores fuera de este crate.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    /// `SELECT ...`.
    Select(Select),
    /// `EXPLAIN <select>`.
    Explain(Explain),
}

impl fmt::Display for Projection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(formatter, "*"),
            Self::Columns(columns) => write!(formatter, "{}", columns.join(", ")),
        }
    }
}

/// Escribe un flotante conservando el punto decimal cuando es entero.
///
/// Args:
///     formatter: Formateador de destino.
///     value: Valor flotante a escribir.
///
/// Returns:
///     `Ok(())` si la escritura tuvo éxito.
///
/// Errors:
///     Propaga cualquier error de [`fmt::Write`] de `formatter`.
fn write_float(formatter: &mut fmt::Formatter<'_>, value: f64) -> fmt::Result {
    if value.fract() == 0.0 {
        write!(formatter, "{value:.1}")
    } else {
        write!(formatter, "{value}")
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Column(name) => write!(formatter, "{name}"),
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Float(value) => write_float(formatter, *value),
            Self::Text(text) => write!(formatter, "'{text}'"),
            Self::Compare { left, op, right } => {
                write!(formatter, "{left} {} {right}", op.as_str())
            }
            Self::And(left, right) => write!(formatter, "{left} AND {right}"),
            Self::Match { column, query } => write!(formatter, "MATCH({column}, '{query}')"),
        }
    }
}

impl fmt::Display for KnnClause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "KNN {} <|{}|> [", self.column, self.k)?;
        for (index, value) in self.query.iter().enumerate() {
            if index > 0 {
                write!(formatter, ", ")?;
            }
            write_float(formatter, *value)?;
        }
        write!(formatter, "]")
    }
}

impl fmt::Display for TraverseClause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "TRAVERSE {} DEPTH {}", self.column, self.depth)
    }
}

impl fmt::Display for OrderBy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ORDER BY {}", self.column)?;
        if self.desc {
            write!(formatter, " DESC")?;
        }
        Ok(())
    }
}

impl fmt::Display for Aggregate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.column {
            Some(column) => write!(formatter, "{}({column})", self.func.as_str())?,
            None => write!(formatter, "{}(*)", self.func.as_str())?,
        }
        if let Some(alias) = &self.alias {
            write!(formatter, " AS {alias}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Select {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SELECT ")?;
        write_projection(formatter, &self.projection, &self.aggregates)?;
        write!(formatter, " FROM {}", self.from)?;
        if let Some(filter) = &self.filter {
            write!(formatter, " WHERE {filter}")?;
        }
        if let Some(knn) = &self.knn {
            write!(formatter, " {knn}")?;
        }
        if let Some(traverse) = &self.traverse {
            write!(formatter, " {traverse}")?;
        }
        if !self.group_by.is_empty() {
            write!(formatter, " GROUP BY {}", self.group_by.join(", "))?;
        }
        if let Some(order_by) = &self.order_by {
            write!(formatter, " {order_by}")?;
        }
        if let Some(limit) = self.limit {
            write!(formatter, " LIMIT {limit}")?;
        }
        Ok(())
    }
}

/// Escribe la proyección canónica: columnas y, tras ellas, los agregados.
///
/// Args:
///     formatter: Formateador de destino.
///     projection: Proyección `*` o lista de columnas.
///     aggregates: Agregados de la proyección (en su orden de aparición).
///
/// Returns:
///     `Ok(())` si la escritura tuvo éxito.
///
/// Errors:
///     Propaga cualquier error de [`fmt::Write`] de `formatter`.
fn write_projection(
    formatter: &mut fmt::Formatter<'_>,
    projection: &Projection,
    aggregates: &[Aggregate],
) -> fmt::Result {
    let mut wrote = false;
    match projection {
        Projection::All => {
            write!(formatter, "*")?;
            wrote = true;
        }
        Projection::Columns(columns) => {
            for (index, column) in columns.iter().enumerate() {
                if index > 0 {
                    write!(formatter, ", ")?;
                }
                write!(formatter, "{column}")?;
                wrote = true;
            }
        }
    }
    for aggregate in aggregates {
        if wrote {
            write!(formatter, ", ")?;
        }
        write!(formatter, "{aggregate}")?;
        wrote = true;
    }
    Ok(())
}

impl fmt::Display for Explain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "EXPLAIN {}", self.inner)
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Select(select) => write!(formatter, "{select}"),
            Self::Explain(explain) => write!(formatter, "{explain}"),
        }
    }
}
