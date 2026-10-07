//! Planificador y ejecutor `SELECT` (SPEC-0012, FR-0012-04/05).
//!
//! [`plan_for`] elige `IndexScan` cuando el filtro contiene `columna =
//! literal` sobre la columna indexada; en otro caso `FullScan`.
//! [`execute_select`] evalúa filtro (con coerción numérica `Int↔Float`),
//! proyección y `LIMIT`. `NULL` excluye la fila y los tipos incompatibles
//! devuelven [`RuscaError::TypeMismatch`].

use std::cmp::Ordering;
use std::collections::BTreeMap;

use ruscadb_core::{Record, RecordId, RuscaError, ScalarMap, ScalarValue};
use ruscadb_query::{CompareOp, Expr, KnnClause, Projection, Select, TraverseClause};
use ruscadb_txn::{Snapshot, Version};

use crate::catalog::{Catalog, ColumnType, TableDef};
use crate::database::Database;
use crate::heap::{heap_read, heap_scan};
use crate::index::index_lookup_eq;

/// Fila resultado: escalares proyectados por nombre de columna.
pub type Row = BTreeMap<String, ScalarValue>;

/// Plan de acceso a una tabla.
///
/// `KnnScan`/`TraverseScan` señalan que la consulta incluye las cláusulas
/// `KNN`/`TRAVERSE`: la base se obtiene por recorrido del heap (o por índice de
/// igualdad) y la cláusula se resuelve después contra los índices en memoria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Recorrido completo del heap.
    FullScan,
    /// Búsqueda de igualdad en el índice de la columna.
    IndexScan {
        /// Columna indexada usada.
        column: String,
    },
    /// La consulta pide vecinos `KNN` (resueltos por el índice HNSW).
    KnnScan {
        /// Columna de embedding pedida.
        column: String,
    },
    /// La consulta pide un recorrido `TRAVERSE` (resuelto por el CSR).
    TraverseScan {
        /// Columna de aristas pedida.
        column: String,
    },
}

/// Elige el plan de acceso para un `SELECT`.
///
/// Prioridad: `KNN` > `TRAVERSE` > `IndexScan` (filtro `columna = literal`
/// sobre la columna indexada) > `FullScan`.
///
/// Args:
///     select: Consulta analizada.
///     catalog: Catálogo con las tablas e índices.
///
/// Returns:
///     El [`Plan`] de acceso elegido.
///
/// Errors:
///     [`RuscaError::TableNotFound`] si la tabla no existe.
pub fn plan_for(select: &Select, catalog: &Catalog) -> Result<Plan, RuscaError> {
    let table = catalog.get(&select.from)?;
    if let Some(knn) = &select.knn {
        return Ok(Plan::KnnScan {
            column: knn.column.clone(),
        });
    }
    if let Some(traverse) = &select.traverse {
        return Ok(Plan::TraverseScan {
            column: traverse.column.clone(),
        });
    }
    let indexed = table.index.as_ref().map(|index| index.column.clone());
    let Some(column) = indexed else {
        return Ok(Plan::FullScan);
    };
    if find_eq_literal(select.filter.as_ref(), &column).is_some() {
        return Ok(Plan::IndexScan { column });
    }
    Ok(Plan::FullScan)
}

/// Ejecuta un `SELECT` analizado con el snapshot más reciente.
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada.
///
/// Returns:
///     Filas proyectadas (hasta `LIMIT`, en orden de inserción).
///
/// Errors:
///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] /
///     [`RuscaError::TypeMismatch`] ante errores de esquema.
pub fn execute_select(database: &mut Database, select: &Select) -> Result<Vec<Row>, RuscaError> {
    let snapshot = database.snapshot();
    execute_select_at(database, select, &snapshot)
}

/// Ejecuta un `SELECT` analizado con visibilidad "as of" `snapshot` (SPEC-0019).
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada.
///     snapshot: Vista fija de visibilidad MVCC.
///
/// Returns:
///     Filas visibles para `snapshot` (hasta `LIMIT`).
///
/// Errors:
///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] /
///     [`RuscaError::TypeMismatch`] ante errores de esquema.
pub fn execute_select_at(
    database: &mut Database,
    select: &Select,
    snapshot: &Snapshot,
) -> Result<Vec<Row>, RuscaError> {
    let catalog = Catalog::load(database)?;
    let plan = plan_for(select, &catalog)?;
    execute_with_plan_at(database, select, plan, snapshot)
}

/// Ejecuta un `SELECT` con un plan fijado resolviendo cláusulas en orden.
///
/// Orden de resolución documentado (SPEC-0017 §Diseño):
///
/// 1. `WHERE` escalar (comparaciones, incluidas dentro de `AND`).
/// 2. `MATCH` full-text (rank BM25) sobre el índice invertido de la columna.
/// 3. `KNN` vectorial (índice HNSW, distancia ascendente, respeta `k`).
/// 4. `TRAVERSE` de grafo (CSR, BFS acotado por `DEPTH`).
/// 5. proyección.
/// 6. `LIMIT`.
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada.
///     plan: Plan de acceso a usar para la lectura base.
///
/// Returns:
///     Filas proyectadas (hasta `LIMIT`), en el orden que fijen `MATCH`/`KNN`/
///     `TRAVERSE` o, si no hay cláusulas de ranking, en orden de inserción.
///
/// Errors:
///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] /
///     [`RuscaError::TypeMismatch`] ante errores de esquema;
///     [`RuscaError::MissingTextColumn`] / [`RuscaError::MissingVector`] /
///     [`RuscaError::MissingGraph`] / [`RuscaError::DimensionMismatch`] ante
///     cláusulas que no encajan con los datos de la tabla.
pub fn execute_with_plan(
    database: &mut Database,
    select: &Select,
    plan: Plan,
) -> Result<Vec<Row>, RuscaError> {
    let snapshot = database.snapshot();
    execute_with_plan_at(database, select, plan, &snapshot)
}

/// Ejecuta un `SELECT` con un plan fijado y visibilidad "as of" `snapshot`.
///
/// Mismo pipeline que [`execute_with_plan`], pero descarta las filas no
/// visibles para el snapshot MVCC antes de resolver `WHERE`/`MATCH`/`KNN`/
/// `TRAVERSE` (SPEC-0019, NF-0019-01).
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada.
///     plan: Plan de acceso a usar para la lectura base.
///     snapshot: Vista fija de visibilidad MVCC.
///
/// Returns:
///     Filas visibles y proyectadas (hasta `LIMIT`).
///
/// Errors:
///     [`RuscaError::TableNotFound`] / [`RuscaError::ColumnNotFound`] /
///     [`RuscaError::TypeMismatch`] ante errores de esquema;
///     [`RuscaError::MissingTextColumn`] / [`RuscaError::MissingVector`] /
///     [`RuscaError::MissingGraph`] / [`RuscaError::DimensionMismatch`] ante
///     cláusulas que no encajan con los datos de la tabla.
pub fn execute_with_plan_at(
    database: &mut Database,
    select: &Select,
    plan: Plan,
    snapshot: &Snapshot,
) -> Result<Vec<Row>, RuscaError> {
    let table = Catalog::load(database)?.get(&select.from)?.clone();
    let candidates = fetch_candidates(database, &table, &plan, select.filter.as_ref())?;
    let (matches, scalar_filter) = split_matches(select.filter.as_ref());
    let mut records = Vec::new();
    for (_, record) in candidates {
        if is_visible(snapshot, &record)
            && keeps_row(&table, scalar_filter.as_ref(), &record.scalars)?
        {
            records.push(record);
        }
    }
    if !matches.is_empty() {
        records = apply_matches(database, &table, &matches, records)?;
    }
    if let Some(knn) = &select.knn {
        records = apply_knn(database, &table, knn, records)?;
    }
    if let Some(traverse) = &select.traverse {
        records = apply_traverse(database, &table, traverse, records)?;
    }
    let mut rows = Vec::new();
    for record in &records {
        rows.push(project_row(&table, &select.projection, &record.scalars)?);
        if let Some(limit) = select.limit {
            if rows.len() >= limit as usize {
                break;
            }
        }
    }
    Ok(rows)
}

/// Decide si un registro es visible para el snapshot MVCC (SPEC-0019).
///
/// Traduce la metadata del registro (`created_tx`/`deleted_tx`) a una
/// [`Version`] y delega en [`Snapshot::is_visible`].
///
/// Args:
///     snapshot: Vista fija de visibilidad.
///     record: Registro candidato.
///
/// Returns:
///     `true` si la versión del registro es visible para el snapshot.
fn is_visible(snapshot: &Snapshot, record: &Record) -> bool {
    let version = Version {
        created_tx: record.meta.created_tx,
        deleted_tx: record.meta.deleted_tx,
    };
    snapshot.is_visible(&version)
}

/// Obtiene los registros candidatos según el plan.
///
/// `KnnScan`/`TraverseScan` se resuelven con un recorrido del heap: las
/// cláusulas de búsqueda se aplican después sobre los índices en memoria.
fn fetch_candidates(
    database: &mut Database,
    table: &TableDef,
    plan: &Plan,
    filter: Option<&Expr>,
) -> Result<Vec<(crate::heap::RowLocator, Record)>, RuscaError> {
    match plan {
        Plan::FullScan | Plan::KnnScan { .. } | Plan::TraverseScan { .. } => {
            heap_scan(database, table)
        }
        Plan::IndexScan { column } => match find_eq_literal(filter, column) {
            Some(literal) => fetch_by_index(database, table, &literal),
            None => heap_scan(database, table),
        },
    }
}

/// Separa los predicados `MATCH` del resto del `WHERE`.
///
/// Returns:
///     `(matches, filtro_escalar)`: la lista `(columna, texto)` de los `MATCH`
///     hallados y el `WHERE` sin ellos (`None` si no queda nada).
fn split_matches(filter: Option<&Expr>) -> (Vec<(String, String)>, Option<Expr>) {
    let mut matches = Vec::new();
    let scalar = collect_matches(filter, &mut matches);
    (matches, scalar)
}

/// Recursivamente extrae los `MATCH` y reconstruye el filtro escalar.
fn collect_matches(expression: Option<&Expr>, matches: &mut Vec<(String, String)>) -> Option<Expr> {
    match expression? {
        Expr::And(left, right) => {
            let left = collect_matches(Some(left), matches);
            let right = collect_matches(Some(right), matches);
            combine_and(left, right)
        }
        Expr::Match { column, query } => {
            matches.push((column.clone(), query.clone()));
            None
        }
        other => Some(other.clone()),
    }
}

/// Reconstruye un `AND` con los lados no vacíos.
fn combine_and(left: Option<Expr>, right: Option<Expr>) -> Option<Expr> {
    match (left, right) {
        (Some(left), Some(right)) => Some(Expr::And(Box::new(left), Box::new(right))),
        (Some(single), None) | (None, Some(single)) => Some(single),
        (None, None) => None,
    }
}

/// Aplica los `MATCH` (conjunción) y ordena por relevancia BM25.
fn apply_matches(
    database: &Database,
    table: &TableDef,
    matches: &[(String, String)],
    records: Vec<Record>,
) -> Result<Vec<Record>, RuscaError> {
    let mut by_id: BTreeMap<RecordId, Record> = records
        .into_iter()
        .map(|record| (record.id, record))
        .collect();
    let mut ordered: Vec<RecordId> = by_id.keys().copied().collect();
    for (column, query) in matches {
        if table.column_type(column)? != ColumnType::Text {
            return Err(RuscaError::MissingTextColumn {
                table: table.name.clone(),
                column: column.clone(),
            });
        }
        let Some(index) = database.table_indexes(&table.name) else {
            ordered.clear();
            break;
        };
        let ranked = index.text_search(column, query, usize::MAX);
        let position: BTreeMap<RecordId, usize> = ranked
            .iter()
            .enumerate()
            .map(|(rank, id)| (*id, rank))
            .collect();
        let mut next: Vec<RecordId> = ordered
            .iter()
            .copied()
            .filter(|id| position.contains_key(id))
            .collect();
        next.sort_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX));
        ordered = next;
        if ordered.is_empty() {
            break;
        }
    }
    Ok(ordered
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect())
}

/// Aplica `KNN`: ordena por distancia HNSW y respeta `k`.
fn apply_knn(
    database: &Database,
    table: &TableDef,
    knn: &KnnClause,
    records: Vec<Record>,
) -> Result<Vec<Record>, RuscaError> {
    let index = database
        .table_indexes(&table.name)
        .ok_or(RuscaError::MissingVector {
            table: table.name.clone(),
            column: knn.column.clone(),
        })?;
    if !index.has_vector() {
        return Err(RuscaError::MissingVector {
            table: table.name.clone(),
            column: knn.column.clone(),
        });
    }
    if knn.k == 0 {
        return Ok(Vec::new());
    }
    let query: Vec<f32> = knn.query.iter().map(|value| *value as f32).collect();
    let ranked = index.vector_search(&query)?;
    let position: BTreeMap<RecordId, usize> = ranked
        .iter()
        .enumerate()
        .map(|(rank, id)| (*id, rank))
        .collect();
    let mut by_id: BTreeMap<RecordId, Record> = records
        .into_iter()
        .map(|record| (record.id, record))
        .collect();
    let mut ordered: Vec<RecordId> = by_id
        .keys()
        .copied()
        .filter(|id| position.contains_key(id))
        .collect();
    ordered.sort_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX));
    ordered.truncate(knn.k as usize);
    Ok(ordered
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect())
}

/// Aplica `TRAVERSE`: recorre el grafo desde las raíces de los candidatos.
fn apply_traverse(
    database: &Database,
    table: &TableDef,
    traverse: &TraverseClause,
    records: Vec<Record>,
) -> Result<Vec<Record>, RuscaError> {
    let index = database
        .table_indexes(&table.name)
        .ok_or(RuscaError::MissingGraph {
            table: table.name.clone(),
            column: traverse.column.clone(),
        })?;
    if !index.has_graph() {
        return Err(RuscaError::MissingGraph {
            table: table.name.clone(),
            column: traverse.column.clone(),
        });
    }
    let mut by_id: BTreeMap<RecordId, Record> = records
        .into_iter()
        .map(|record| (record.id, record))
        .collect();
    let candidates: Vec<RecordId> = by_id.keys().copied().collect();
    let seeds = index.traversal_seeds(&candidates);
    let reachable = index.traverse(&seeds, traverse.depth);
    Ok(reachable
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect())
}

/// Lee por índice los registros que igualan el literal.
fn fetch_by_index(
    database: &mut Database,
    table: &TableDef,
    literal: &ScalarValue,
) -> Result<Vec<(crate::heap::RowLocator, Record)>, RuscaError> {
    let mut rows = Vec::new();
    for locator in index_lookup_eq(database, table, literal)? {
        rows.push((locator, heap_read(database, locator)?));
    }
    Ok(rows)
}

/// Busca `columna = literal` en una conjunción `AND` (ambos órdenes).
fn find_eq_literal(filter: Option<&Expr>, column: &str) -> Option<ScalarValue> {
    let expression = filter?;
    match expression {
        Expr::And(left, right) => {
            find_eq_literal(Some(left), column).or_else(|| find_eq_literal(Some(right), column))
        }
        Expr::Compare { left, op, right } if *op == CompareOp::Eq => {
            eq_literal_sides(left, right, column)
        }
        _ => None,
    }
}

/// Extrae el literal si un lado es la columna y el otro un literal.
fn eq_literal_sides(left: &Expr, right: &Expr, column: &str) -> Option<ScalarValue> {
    if let Expr::Column(name) = left {
        if name == column {
            return expr_literal(right);
        }
    }
    if let Expr::Column(name) = right {
        if name == column {
            return expr_literal(left);
        }
    }
    None
}

/// Convierte un literal del IR a `ScalarValue`.
fn expr_literal(expression: &Expr) -> Option<ScalarValue> {
    match expression {
        Expr::Int(number) => Some(ScalarValue::Int(*number)),
        Expr::Float(number) => Some(ScalarValue::Float(*number)),
        Expr::Text(text) => Some(ScalarValue::Text(text.clone())),
        Expr::Column(_) | Expr::Compare { .. } | Expr::And(_, _) | Expr::Match { .. } => None,
    }
}

/// Decide si la fila pasa el filtro (valida las columnas contra el esquema).
fn keeps_row(
    table: &TableDef,
    filter: Option<&Expr>,
    scalars: &ScalarMap,
) -> Result<bool, RuscaError> {
    let Some(expression) = filter else {
        return Ok(true);
    };
    validate_filter_columns(table, expression)?;
    eval_predicate(expression, scalars)
}

/// Comprueba que cada columna del filtro exista en el esquema.
fn validate_filter_columns(table: &TableDef, expression: &Expr) -> Result<(), RuscaError> {
    match expression {
        Expr::Column(name) => table.column_type(name).map(|_| ()),
        Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => Ok(()),
        Expr::Compare { left, right, .. } => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
        Expr::And(left, right) => {
            validate_filter_columns(table, left)?;
            validate_filter_columns(table, right)
        }
        // `MATCH` se extrae antes de evaluar el filtro escalar (lo resuelve el
        // índice full-text), así que aquí no valida columnas.
        Expr::Match { .. } => Ok(()),
    }
}

/// Evalúa un predicado sobre los escalares de la fila.
fn eval_predicate(expression: &Expr, scalars: &ScalarMap) -> Result<bool, RuscaError> {
    match expression {
        Expr::And(left, right) => {
            Ok(eval_predicate(left, scalars)? && eval_predicate(right, scalars)?)
        }
        Expr::Compare { left, op, right } => {
            let first = eval_operand(left, scalars)?;
            let second = eval_operand(right, scalars)?;
            compare_values(*op, &first, &second)
        }
        Expr::Column(_) | Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => {
            Err(RuscaError::TypeMismatch {
                message: "el filtro WHERE debe ser una comparación o AND de comparaciones"
                    .to_string(),
            })
        }
        Expr::Match { .. } => Err(RuscaError::TypeMismatch {
            message: "MATCH se resuelve en el índice full-text, no como comparación escalar"
                .to_string(),
        }),
    }
}

/// Evalúa un operando (columna → valor de la fila, ausente → `NULL`).
fn eval_operand(expression: &Expr, scalars: &ScalarMap) -> Result<ScalarValue, RuscaError> {
    match expression {
        Expr::Column(name) => Ok(scalars.get(name).cloned().unwrap_or(ScalarValue::Null)),
        Expr::Int(number) => Ok(ScalarValue::Int(*number)),
        Expr::Float(number) => Ok(ScalarValue::Float(*number)),
        Expr::Text(text) => Ok(ScalarValue::Text(text.clone())),
        Expr::Compare { .. } | Expr::And(_, _) | Expr::Match { .. } => {
            Err(RuscaError::TypeMismatch {
                message: "una comparación no puede anidarse como operando".to_string(),
            })
        }
    }
}

/// Compara dos valores (`NULL` excluye la fila; `Int↔Float` con coerción).
fn compare_values(
    operation: CompareOp,
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<bool, RuscaError> {
    if *left == ScalarValue::Null || *right == ScalarValue::Null {
        return Ok(false);
    }
    let ordering = order_values(left, right)?;
    Ok(match operation {
        CompareOp::Eq => ordering == Ordering::Equal,
        CompareOp::NotEq => ordering != Ordering::Equal,
        CompareOp::Lt => ordering == Ordering::Less,
        CompareOp::LtEq => ordering != Ordering::Greater,
        CompareOp::Gt => ordering == Ordering::Greater,
        CompareOp::GtEq => ordering != Ordering::Less,
    })
}

/// Ordena dos valores no nulos del mismo dominio.
fn order_values(left: &ScalarValue, right: &ScalarValue) -> Result<Ordering, RuscaError> {
    match (left, right) {
        (ScalarValue::Int(first), ScalarValue::Int(second)) => Ok(first.cmp(second)),
        (ScalarValue::Float(first), ScalarValue::Float(second)) => {
            Ok(compare_floats(*first, *second))
        }
        (ScalarValue::Int(number), ScalarValue::Float(other)) => {
            Ok(compare_floats(*number as f64, *other))
        }
        (ScalarValue::Float(other), ScalarValue::Int(number)) => {
            Ok(compare_floats(*other, *number as f64))
        }
        (ScalarValue::Text(first), ScalarValue::Text(second)) => Ok(first.cmp(second)),
        (ScalarValue::Bool(first), ScalarValue::Bool(second)) => Ok(first.cmp(second)),
        _ => Err(RuscaError::TypeMismatch {
            message: format!("no se puede comparar {left:?} con {right:?} en el filtro"),
        }),
    }
}

/// Compara flotantes (`NaN` nunca iguala: excluye la fila).
fn compare_floats(first: f64, second: f64) -> Ordering {
    first.partial_cmp(&second).unwrap_or(Ordering::Less)
}

/// Proyecta las columnas pedidas (valida el esquema primero).
fn project_row(
    table: &TableDef,
    projection: &Projection,
    scalars: &ScalarMap,
) -> Result<Row, RuscaError> {
    match projection {
        Projection::All => Ok(scalars.clone()),
        Projection::Columns(columns) => {
            let mut row = Row::new();
            for column in columns {
                table.column_type(column)?;
                let value = scalars.get(column).cloned().unwrap_or(ScalarValue::Null);
                row.insert(column.clone(), value);
            }
            Ok(row)
        }
    }
}
