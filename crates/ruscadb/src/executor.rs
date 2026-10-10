//! Planificador y ejecutor `SELECT` (SPEC-0012, FR-0012-04/05).
//!
//! [`plan_for`] elige `IndexScan` cuando el filtro contiene `columna =
//! literal` sobre la columna indexada; en otro caso `FullScan`.
//! [`execute_select`] evalúa filtro (con coerción numérica `Int↔Float`),
//! proyección y `LIMIT`. `NULL` excluye la fila y los tipos incompatibles
//! devuelven [`RuscaError::TypeMismatch`].

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use ruscadb_core::{Record, RecordId, RuscaError, ScalarMap, ScalarValue};
use ruscadb_query::{
    AggFunc, Aggregate, CompareOp, Expr, HavingCondition, KnnClause, OrderBy, Projection, Select,
    TraverseClause,
};
use ruscadb_txn::{Snapshot, Version};

use crate::catalog::{Catalog, ColumnType, TableDef};
use crate::database::Database;
use crate::document::{doc_contains, extract_doc_scalar};
use crate::heap::{heap_read, heap_scan};
use crate::index::{canonical_key, index_lookup_eq};

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
pub(crate) fn plan_for(select: &Select, catalog: &Catalog) -> Result<Plan, RuscaError> {
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
pub(crate) fn execute_select_at(
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
/// 5. `ORDER BY` (orden estable; `NULL` al final en `ASC` y al principio en `DESC`).
/// 6. proyección.
/// 7. `LIMIT`.
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
///
/// Punto de entrada para tests que fijan el plan (solo compilado en tests).
#[cfg(test)]
pub(crate) fn execute_with_plan(
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
pub(crate) fn execute_with_plan_at(
    database: &mut Database,
    select: &Select,
    plan: Plan,
    snapshot: &Snapshot,
) -> Result<Vec<Row>, RuscaError> {
    if let Some(join) = &select.join {
        return crate::join::execute_join_at(database, select, join, snapshot);
    }
    let table = Catalog::load(database)?.get(&select.from)?.clone();
    let candidates = fetch_candidates(database, &table, &plan, select.filter.as_ref())?;
    let (matches, scalar_filter) = split_matches(select.filter.as_ref());
    let mut records = Vec::new();
    for (_, record) in candidates {
        if is_visible(snapshot, &record) && keeps_row(&table, scalar_filter.as_ref(), &record)? {
            records.push(record);
        }
    }
    if !matches.is_empty() {
        records = apply_matches(database, &table, &matches, records)?;
    }
    if let Some(knn) = &select.knn {
        records = apply_knn(database, &table, knn, records, select.filter.is_some())?;
    }
    if let Some(traverse) = &select.traverse {
        records = apply_traverse(database, &table, traverse, records)?;
    }
    if is_aggregate_query(select) {
        return aggregate_records(&table, select, &records);
    }
    if let Some(order_by) = &select.order_by {
        sort_records(&table, order_by, &mut records)?;
    }
    let limit = select.limit.map_or(usize::MAX, |value| value as usize);
    let mut rows = Vec::new();
    for record in &records {
        if rows.len() >= limit {
            break;
        }
        rows.push(project_row(&table, &select.projection, &record.scalars)?);
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
pub(crate) fn is_visible(snapshot: &Snapshot, record: &Record) -> bool {
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

/// Aplica `KNN`: top-k restringido al filtro `WHERE` mediante FVS.
///
/// `records` ya viene filtrado por visibilidad MVCC y por el `WHERE` escalar,
/// de modo que sus ids son el conjunto `allowed`.
///
/// - **Con `WHERE`** (`has_filter`): delega en `ifvs_vector_search`, que cablea
///   `ruscadb_fvs::search_auto_indexed` sobre el `HnswIndex` real de la tabla y
///   elige la estrategia (pre/in/post) por selectividad (SPEC-0048). `pre` e
///   `in` son exactos; `post` es sonoro y recalcula exacto si no cubre `k`.
/// - **Sin `WHERE`**: conserva el comportamiento previo
///   (`filtered_vector_search`), equivalente al KNN clásico.
///
/// Args:
///     database: Base abierta.
///     table: Definición de la tabla.
///     knn: Cláusula `KNN` (columna, `k` y vector de consulta).
///     records: Candidatos visibles y filtrados por `WHERE` (conjunto `allowed`).
///     has_filter: `true` si la consulta lleva `WHERE` (filtro escalar).
///
/// Returns:
///     Los registros del top-k filtrado, en orden de distancia ascendente.
///
/// Errors:
///     [`RuscaError::MissingVector`] si la tabla no tiene índice vectorial;
///     [`RuscaError::DimensionMismatch`] si la dimensión de la consulta no
///     coincide con el índice.
fn apply_knn(
    database: &Database,
    table: &TableDef,
    knn: &KnnClause,
    records: Vec<Record>,
    has_filter: bool,
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
    let allowed: BTreeSet<RecordId> = records.iter().map(|record| record.id).collect();
    let ranked = if has_filter {
        index.ifvs_vector_search(&query, knn.k as usize, &allowed)?
    } else {
        index.filtered_vector_search(&query, knn.k as usize, &allowed)?
    };
    let mut by_id: BTreeMap<RecordId, Record> = records
        .into_iter()
        .map(|record| (record.id, record))
        .collect();
    Ok(ranked
        .into_iter()
        .filter_map(|(id, _)| by_id.remove(&id))
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

/// Ordena los registros por la cláusula `ORDER BY` (estable, fallible).
///
/// `NULL` ordena al final en `ASC` (y al principio en `DESC`); valores de tipos
/// incompatibles devuelven [`RuscaError::TypeMismatch`] y una columna ausente
/// [`RuscaError::ColumnNotFound`]. El orden es estable: los empates conservan
/// su posición previa (p. ej. el ranking de `MATCH`/`KNN`/`TRAVERSE`).
///
/// Args:
///     table: Definición de la tabla (para validar la columna).
///     order_by: Cláusula de ordenamiento.
///     records: Registros a ordenar in situ.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si la columna no existe;
///     [`RuscaError::TypeMismatch`] si dos valores no son comparables.
fn sort_records(
    table: &TableDef,
    order_by: &OrderBy,
    records: &mut [Record],
) -> Result<(), RuscaError> {
    table.column_type(&order_by.column)?;
    let mut failure: Option<RuscaError> = None;
    records.sort_by(|left, right| {
        if failure.is_some() {
            return Ordering::Equal;
        }
        match compare_order_values(
            left.scalars.get(&order_by.column),
            right.scalars.get(&order_by.column),
            order_by.desc,
        ) {
            Ok(ordering) => ordering,
            Err(error) => {
                failure = Some(error);
                Ordering::Equal
            }
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Compara dos valores de la columna de orden (`NULL` al final en `ASC`).
fn compare_order_values(
    left: Option<&ScalarValue>,
    right: Option<&ScalarValue>,
    descending: bool,
) -> Result<Ordering, RuscaError> {
    let ordering = order_with_nulls_last(left, right)?;
    Ok(if descending {
        ordering.reverse()
    } else {
        ordering
    })
}

/// Ordena dos valores colocando `NULL` (o ausente) al final.
fn order_with_nulls_last(
    left: Option<&ScalarValue>,
    right: Option<&ScalarValue>,
) -> Result<Ordering, RuscaError> {
    match (non_null(left), non_null(right)) {
        (None, None) => Ok(Ordering::Equal),
        (None, Some(_)) => Ok(Ordering::Greater),
        (Some(_), None) => Ok(Ordering::Less),
        (Some(first), Some(second)) => {
            order_values(first, second).map_err(|_| RuscaError::TypeMismatch {
                message: format!("ORDER BY no puede comparar {first:?} con {second:?}"),
            })
        }
    }
}

/// Devuelve el valor no nulo, tratando ausente y `Null` como `None`.
fn non_null(value: Option<&ScalarValue>) -> Option<&ScalarValue> {
    match value {
        Some(ScalarValue::Null) | None => None,
        Some(other) => Some(other),
    }
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
        Expr::Column(_)
        | Expr::Compare { .. }
        | Expr::And(_, _)
        | Expr::Match { .. }
        | Expr::DocExtract { .. }
        | Expr::DocContains { .. } => None,
    }
}

/// Convierte un literal del IR a `ScalarValue` o falla con error accionable.
///
/// Args:
///     expression: Expresión del IR que debe ser un literal.
///
/// Returns:
///     El [`ScalarValue`] del literal.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si la expresión no es un literal escalar.
pub(crate) fn literal_scalar(expression: &Expr) -> Result<ScalarValue, RuscaError> {
    expr_literal(expression).ok_or_else(|| RuscaError::TypeMismatch {
        message: "se esperaba un literal (entero, flotante o texto)".to_string(),
    })
}

/// Decide si la fila pasa el filtro (valida las columnas contra el esquema).
pub(crate) fn keeps_row(
    table: &TableDef,
    filter: Option<&Expr>,
    record: &Record,
) -> Result<bool, RuscaError> {
    let Some(expression) = filter else {
        return Ok(true);
    };
    validate_filter_columns(table, expression)?;
    eval_predicate(expression, record)
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
        // índice full-text), así que aquí no valida columnas. Los operadores
        // documentales tampoco referencian columnas del esquema (usan `Record.doc`).
        Expr::Match { .. } | Expr::DocExtract { .. } | Expr::DocContains { .. } => Ok(()),
    }
}

/// Evalúa un predicado sobre el registro (escalares + documento).
fn eval_predicate(expression: &Expr, record: &Record) -> Result<bool, RuscaError> {
    match expression {
        Expr::And(left, right) => {
            Ok(eval_predicate(left, record)? && eval_predicate(right, record)?)
        }
        Expr::Compare { left, op, right } => {
            let first = eval_operand(left, record)?;
            let second = eval_operand(right, record)?;
            compare_values(*op, &first, &second)
        }
        Expr::DocContains { json, .. } => doc_contains(record, json),
        Expr::Column(_) | Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => {
            Err(RuscaError::TypeMismatch {
                message: "el filtro WHERE debe ser una comparación o AND de comparaciones"
                    .to_string(),
            })
        }
        Expr::DocExtract { .. } => Err(RuscaError::TypeMismatch {
            message: "la extracción documental '->' debe usarse dentro de una comparación"
                .to_string(),
        }),
        Expr::Match { .. } => Err(RuscaError::TypeMismatch {
            message: "MATCH se resuelve en el índice full-text, no como comparación escalar"
                .to_string(),
        }),
    }
}

/// Evalúa un operando (columna o extracción documental → valor; ausente → `NULL`).
fn eval_operand(expression: &Expr, record: &Record) -> Result<ScalarValue, RuscaError> {
    match expression {
        Expr::Column(name) => Ok(record
            .scalars
            .get(name)
            .cloned()
            .unwrap_or(ScalarValue::Null)),
        Expr::DocExtract { path, .. } => {
            Ok(extract_doc_scalar(record, path).unwrap_or(ScalarValue::Null))
        }
        Expr::Int(number) => Ok(ScalarValue::Int(*number)),
        Expr::Float(number) => Ok(ScalarValue::Float(*number)),
        Expr::Text(text) => Ok(ScalarValue::Text(text.clone())),
        Expr::Compare { .. } | Expr::And(_, _) | Expr::Match { .. } | Expr::DocContains { .. } => {
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

/// Indica si la consulta requiere agregación (`GROUP BY` o agregados).
///
/// Args:
///     select: Consulta analizada.
///
/// Returns:
///     `true` si hay columnas de agrupación o agregados en la proyección.
fn is_aggregate_query(select: &Select) -> bool {
    !select.group_by.is_empty() || !select.aggregates.is_empty()
}

/// Tipo de acumulador resuelto de un agregado (columna y tipo ya verificados).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AggKind {
    /// `COUNT(*)`.
    CountStar,
    /// `COUNT(columna)`.
    Count,
    /// `SUM` sobre columna entera.
    SumInt,
    /// `SUM` sobre columna flotante.
    SumFloat,
    /// `AVG`.
    Avg,
    /// `MIN`.
    Min,
    /// `MAX`.
    Max,
}

/// Estado acumulado de un grupo: claves representativas + celdas de agregados.
struct GroupAccumulator {
    /// Valores de las columnas de `GROUP BY` (de la primera fila del grupo).
    keys: ScalarMap,
    /// Número de filas del grupo (`COUNT(*)`).
    row_count: u64,
    /// Estado por agregado, alineado con `select.aggregates`.
    cells: Vec<AggregateState>,
}

impl GroupAccumulator {
    /// Crea un grupo vacío (sin filas) para la agregación global.
    fn empty(kinds: &[AggKind]) -> Self {
        Self {
            keys: ScalarMap::new(),
            row_count: 0,
            cells: new_cells(kinds),
        }
    }

    /// Crea un grupo con las claves de su primera fila.
    fn new(keys: ScalarMap, kinds: &[AggKind]) -> Self {
        Self {
            keys,
            row_count: 0,
            cells: new_cells(kinds),
        }
    }

    /// Materializa la fila de salida: columnas de grupo + agregados.
    fn to_row(&self, projection: &Projection, aggregates: &[Aggregate]) -> Row {
        let mut row = Row::new();
        if let Projection::Columns(columns) = projection {
            for column in columns {
                let value = self.keys.get(column).cloned().unwrap_or(ScalarValue::Null);
                row.insert(column.clone(), value);
            }
        }
        for (index, aggregate) in aggregates.iter().enumerate() {
            let value = self.aggregate_value(aggregates, index);
            row.insert(aggregate_output_key(aggregate), value);
        }
        row
    }

    /// Valor final del agregado `aggregates[index]` en este grupo.
    ///
    /// Args:
    ///     aggregates: Lista de agregados alineada con `self.cells`.
    ///     index: Posición del agregado cuyo valor se pide.
    ///
    /// Returns:
    ///     El valor agregado (`COUNT(*)` usa el conteo de filas).
    fn aggregate_value(&self, aggregates: &[Aggregate], index: usize) -> ScalarValue {
        if aggregates[index].func == AggFunc::CountStar {
            ScalarValue::Int(self.row_count as i64)
        } else {
            self.cells[index].finish()
        }
    }
}

/// Estado acumulado de un único agregado en un grupo.
enum AggregateState {
    /// `COUNT(*)` (se resuelve con el conteo de filas del grupo).
    CountStar,
    /// `COUNT(columna)`.
    Count {
        /// Valores no `NULL` vistos.
        count: u64,
    },
    /// `SUM` entero.
    SumInt {
        /// Suma acumulada.
        total: i64,
        /// `true` si se vio algún valor no `NULL`.
        seen: bool,
    },
    /// `SUM` flotante.
    SumFloat {
        /// Suma acumulada.
        total: f64,
        /// `true` si se vio algún valor no `NULL`.
        seen: bool,
    },
    /// `AVG`.
    Avg {
        /// Suma acumulada.
        total: f64,
        /// Número de valores no `NULL`.
        count: u64,
    },
    /// `MIN`/`MAX` (el orden lo decide el tipo de agregado).
    Extreme {
        /// Extremo actual (`None` si aún no hay valores no `NULL`).
        current: Option<ScalarValue>,
    },
}

impl AggregateState {
    /// Crea el estado inicial para un tipo de agregado.
    fn new(kind: AggKind) -> Self {
        match kind {
            AggKind::CountStar => Self::CountStar,
            AggKind::Count => Self::Count { count: 0 },
            AggKind::SumInt => Self::SumInt {
                total: 0,
                seen: false,
            },
            AggKind::SumFloat => Self::SumFloat {
                total: 0.0,
                seen: false,
            },
            AggKind::Avg => Self::Avg {
                total: 0.0,
                count: 0,
            },
            AggKind::Min | AggKind::Max => Self::Extreme { current: None },
        }
    }

    /// Valor final del agregado (semántica SQL de `NULL`).
    fn finish(&self) -> ScalarValue {
        match self {
            Self::CountStar => ScalarValue::Null,
            Self::Count { count } => ScalarValue::Int(*count as i64),
            Self::SumInt { total, seen } => {
                if *seen {
                    ScalarValue::Int(*total)
                } else {
                    ScalarValue::Null
                }
            }
            Self::SumFloat { total, seen } => {
                if *seen {
                    ScalarValue::Float(*total)
                } else {
                    ScalarValue::Null
                }
            }
            Self::Avg { total, count } => {
                if *count == 0 {
                    ScalarValue::Null
                } else {
                    ScalarValue::Float(*total / *count as f64)
                }
            }
            Self::Extreme { current } => current.clone().unwrap_or(ScalarValue::Null),
        }
    }
}

/// Crea las celdas de agregado alineadas con la lista de tipos.
fn new_cells(kinds: &[AggKind]) -> Vec<AggregateState> {
    kinds
        .iter()
        .map(|kind| AggregateState::new(*kind))
        .collect()
}

/// Ejecuta la agregación por hash (*hash aggregation*; patrón DuckDB/DataFusion).
///
/// Agrupa por las columnas de `GROUP BY` con una `BTreeMap` de clave canónica
/// (determinismo del orden de salida) y acumula los agregados en una sola
/// pasada. Sin `GROUP BY` produce una única fila global (incluso con 0 filas).
/// La semántica SQL de `NULL` se respeta: `COUNT(*)` cuenta filas mientras que
/// `COUNT(col)`/`SUM`/`AVG`/`MIN`/`MAX` ignoran los `NULL`.
///
/// Args:
///     table: Definición de la tabla (esquema).
///     select: Consulta con agregados y/o `GROUP BY`.
///     records: Filas ya filtradas (WHERE/MATCH/KNN/TRAVERSE) y visibles a MVCC.
///
/// Returns:
///     Una fila por grupo (o una global), con columnas de grupo + agregados.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si una columna proyectada no está agrupada
///     o si `SUM`/`AVG` se aplican sobre una columna no numérica.
fn aggregate_records(
    table: &TableDef,
    select: &Select,
    records: &[Record],
) -> Result<Vec<Row>, RuscaError> {
    validate_aggregate_query(table, select)?;
    // Agregados del SELECT más los que solo aparecen en el HAVING (SPEC-0051):
    // se acumulan todos, pero `to_row` solo emite los del SELECT.
    let mut all_aggregates: Vec<Aggregate> = select.aggregates.clone();
    all_aggregates.extend(having_aggregates(select));
    let kinds: Vec<AggKind> = all_aggregates
        .iter()
        .map(|aggregate| aggregate_kind(table, aggregate))
        .collect::<Result<_, _>>()?;
    let mut groups: BTreeMap<Vec<u8>, GroupAccumulator> = BTreeMap::new();
    if select.group_by.is_empty() {
        groups.insert(Vec::new(), GroupAccumulator::empty(&kinds));
    }
    for record in records {
        let keys = group_values(&select.group_by, &record.scalars);
        let key = group_key(table, &select.group_by, &keys)?;
        let group = groups
            .entry(key)
            .or_insert_with(|| GroupAccumulator::new(keys, &kinds));
        group.row_count += 1;
        accumulate(&all_aggregates, &kinds, &record.scalars, &mut group.cells)?;
    }
    let mut rows: Vec<Row> = groups
        .values()
        .filter(|group| group_satisfies_having(group, &all_aggregates, &select.having))
        .map(|group| group.to_row(&select.projection, &select.aggregates))
        .collect();
    if let Some(order_by) = &select.order_by {
        sort_rows(order_by, &mut rows)?;
    }
    if let Some(limit) = select.limit {
        rows.truncate(limit as usize);
    }
    Ok(rows)
}

/// Agregados que solo aparecen en el `HAVING` (no están en el `SELECT`).
///
/// Args:
///     select: Consulta con agregados y condiciones `HAVING`.
///
/// Returns:
///     Un [`Aggregate`] sin alias por cada `(func, columna)` del `HAVING`
///     ausente en `select.aggregates` (para acumularlo junto al resto).
fn having_aggregates(select: &Select) -> Vec<Aggregate> {
    let mut extra = Vec::new();
    for condition in &select.having {
        let present = select.aggregates.iter().any(|aggregate| {
            aggregate.func == condition.func && aggregate.column == condition.column
        });
        if !present {
            extra.push(Aggregate {
                func: condition.func,
                column: condition.column.clone(),
                alias: None,
            });
        }
    }
    extra
}

/// Indica si un grupo satisface todas las condiciones del `HAVING` (AND).
///
/// Un literal `NULL` o un agregado `NULL` excluyen al grupo (como en `WHERE`).
/// Un agregado del `HAVING` ausente en la lista acumulada es un error interno.
fn group_satisfies_having(
    group: &GroupAccumulator,
    aggregates: &[Aggregate],
    having: &[HavingCondition],
) -> bool {
    having.iter().all(|condition| {
        let position = aggregates.iter().position(|aggregate| {
            aggregate.func == condition.func && aggregate.column == condition.column
        });
        let Some(index) = position else {
            return false;
        };
        let value = group.aggregate_value(aggregates, index);
        let Ok(literal) = having_literal_value(&condition.literal) else {
            return false;
        };
        compare_values(condition.op, &value, &literal).unwrap_or(false)
    })
}

/// Valor escalar de un literal del `HAVING` (el parser solo admite Int/Float/Text).
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si el literal no es un escalar simple.
fn having_literal_value(literal: &Expr) -> Result<ScalarValue, RuscaError> {
    match literal {
        Expr::Int(number) => Ok(ScalarValue::Int(*number)),
        Expr::Float(number) => Ok(ScalarValue::Float(*number)),
        Expr::Text(text) => Ok(ScalarValue::Text(text.clone())),
        other => Err(RuscaError::TypeMismatch {
            message: format!("HAVING exige un literal, se obtuvo {other:?}"),
        }),
    }
}

/// Valida la consulta agregada (proyección agrupada, columnas y tipos).
///
/// Args:
///     table: Definición de la tabla.
///     select: Consulta con agregados y/o `GROUP BY`.
///
/// Returns:
///     `Ok(())` si la consulta es agregable.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si la proyección viola la regla de
///     agrupación o si un agregado no es aplicable al tipo de la columna.
fn validate_aggregate_query(table: &TableDef, select: &Select) -> Result<(), RuscaError> {
    let Projection::Columns(columns) = &select.projection else {
        return Err(RuscaError::TypeMismatch {
            message: "SELECT * no se puede combinar con GROUP BY ni agregados".to_string(),
        });
    };
    for column in &select.group_by {
        table.column_type(column)?;
    }
    for column in columns {
        table.column_type(column)?;
        if !select.group_by.contains(column) {
            return Err(RuscaError::TypeMismatch {
                message: format!(
                    "la columna '{column}' no está en GROUP BY ni es un agregado (agrúpala con GROUP BY o envuélvela en COUNT/SUM/AVG/MIN/MAX)"
                ),
            });
        }
    }
    let _ = aggregate_kinds(table, select)?;
    // Valida los agregados del HAVING con las mismas reglas (columna
    // existente, SUM/AVG numéricos) para errores accionables (SPEC-0051).
    for aggregate in having_aggregates(select) {
        let _ = aggregate_kind(table, &aggregate)?;
    }
    if let Some(order_by) = &select.order_by {
        if !is_aggregate_output_column(select, &order_by.column) {
            return Err(RuscaError::ColumnNotFound {
                column: order_by.column.clone(),
            });
        }
    }
    Ok(())
}

/// Resuelve el tipo de acumulador de cada agregado (valida columnas/tipos).
fn aggregate_kinds(table: &TableDef, select: &Select) -> Result<Vec<AggKind>, RuscaError> {
    select
        .aggregates
        .iter()
        .map(|aggregate| aggregate_kind(table, aggregate))
        .collect()
}

/// Resuelve el tipo de acumulador de un agregado.
fn aggregate_kind(table: &TableDef, aggregate: &Aggregate) -> Result<AggKind, RuscaError> {
    if aggregate.func == AggFunc::CountStar {
        return Ok(AggKind::CountStar);
    }
    let column = aggregate
        .column
        .as_deref()
        .ok_or_else(|| RuscaError::TypeMismatch {
            message: format!("{} requiere una columna", aggregate.func.as_str()),
        })?;
    let column_type = table.column_type(column)?;
    match aggregate.func {
        AggFunc::CountStar => Ok(AggKind::CountStar),
        AggFunc::Count => Ok(AggKind::Count),
        AggFunc::Min => Ok(AggKind::Min),
        AggFunc::Max => Ok(AggKind::Max),
        AggFunc::Sum | AggFunc::Avg => numeric_kind(aggregate.func, column, column_type),
    }
}

/// Resuelve `SUM`/`AVG` exigiendo una columna numérica.
fn numeric_kind(
    func: AggFunc,
    column: &str,
    column_type: ColumnType,
) -> Result<AggKind, RuscaError> {
    match (func, column_type) {
        (AggFunc::Sum, ColumnType::Int) => Ok(AggKind::SumInt),
        (AggFunc::Sum, ColumnType::Float) => Ok(AggKind::SumFloat),
        (AggFunc::Avg, ColumnType::Int | ColumnType::Float) => Ok(AggKind::Avg),
        _ => Err(RuscaError::TypeMismatch {
            message: format!(
                "{} requiere una columna numérica, pero '{column}' es {column_type:?}",
                func.as_str()
            ),
        }),
    }
}

/// Extrae los valores de las columnas de `GROUP BY` (ausente → `NULL`).
fn group_values(group_by: &[String], scalars: &ScalarMap) -> ScalarMap {
    group_by
        .iter()
        .map(|column| {
            let value = scalars.get(column).cloned().unwrap_or(ScalarValue::Null);
            (column.clone(), value)
        })
        .collect()
}

/// Construye la clave canónica ordenable de un grupo (`NULL` ordena primero).
fn group_key(
    table: &TableDef,
    group_by: &[String],
    keys: &ScalarMap,
) -> Result<Vec<u8>, RuscaError> {
    let mut encoded = Vec::new();
    for column in group_by {
        match keys.get(column).unwrap_or(&ScalarValue::Null) {
            ScalarValue::Null => encoded.push(0x00),
            other => encoded.extend_from_slice(&canonical_key(table.column_type(column)?, other)?),
        }
        encoded.push(0xFF);
    }
    Ok(encoded)
}

/// Acumula una fila en las celdas de su grupo (ignorando `NULL`).
fn accumulate(
    aggregates: &[Aggregate],
    kinds: &[AggKind],
    scalars: &ScalarMap,
    cells: &mut [AggregateState],
) -> Result<(), RuscaError> {
    for ((aggregate, kind), state) in aggregates.iter().zip(kinds).zip(cells.iter_mut()) {
        let value = match aggregate.column.as_deref() {
            Some(column) => scalars
                .get(column)
                .filter(|value| **value != ScalarValue::Null),
            None => None,
        };
        apply_aggregate(*kind, value, state)?;
    }
    Ok(())
}

/// Aplica un valor al estado del agregado correspondiente.
fn apply_aggregate(
    kind: AggKind,
    value: Option<&ScalarValue>,
    state: &mut AggregateState,
) -> Result<(), RuscaError> {
    match (kind, state) {
        (AggKind::CountStar, AggregateState::CountStar) => {}
        (AggKind::Count, AggregateState::Count { count }) => {
            if value.is_some() {
                *count += 1;
            }
        }
        (AggKind::SumInt, AggregateState::SumInt { total, seen }) => {
            if let Some(value) = value {
                *total += numeric_i64(value)?;
                *seen = true;
            }
        }
        (AggKind::SumFloat, AggregateState::SumFloat { total, seen }) => {
            if let Some(value) = value {
                *total += numeric_f64(value)?;
                *seen = true;
            }
        }
        (AggKind::Avg, AggregateState::Avg { total, count }) => {
            if let Some(value) = value {
                *total += numeric_f64(value)?;
                *count += 1;
            }
        }
        (AggKind::Min, AggregateState::Extreme { current }) => {
            update_extreme(current, value, false)?;
        }
        (AggKind::Max, AggregateState::Extreme { current }) => {
            update_extreme(current, value, true)?;
        }
        _ => {}
    }
    Ok(())
}

/// Actualiza el extremo actual con un candidato no nulo (`MIN` o `MAX`).
fn update_extreme(
    current: &mut Option<ScalarValue>,
    candidate: Option<&ScalarValue>,
    want_max: bool,
) -> Result<(), RuscaError> {
    let Some(candidate) = candidate else {
        return Ok(());
    };
    match current {
        None => *current = Some(candidate.clone()),
        Some(existing) => {
            let ordering = order_values(candidate, existing)?;
            let replace = if want_max {
                ordering == Ordering::Greater
            } else {
                ordering == Ordering::Less
            };
            if replace {
                *current = Some(candidate.clone());
            }
        }
    }
    Ok(())
}

/// Convierte un valor entero para `SUM` entero.
fn numeric_i64(value: &ScalarValue) -> Result<i64, RuscaError> {
    match value {
        ScalarValue::Int(number) => Ok(*number),
        other => Err(RuscaError::TypeMismatch {
            message: format!("SUM sobre una columna no entera: {other:?}"),
        }),
    }
}

/// Convierte un valor numérico a `f64` para `SUM`/`AVG`.
fn numeric_f64(value: &ScalarValue) -> Result<f64, RuscaError> {
    match value {
        ScalarValue::Int(number) => Ok(*number as f64),
        ScalarValue::Float(number) => Ok(*number),
        ScalarValue::UInt(number) => Ok(*number as f64),
        other => Err(RuscaError::TypeMismatch {
            message: format!("se esperaba un valor numérico para SUM/AVG, se obtuvo {other:?}"),
        }),
    }
}

/// Nombre de salida de un agregado: el alias si existe, o uno derivado.
fn aggregate_output_key(aggregate: &Aggregate) -> String {
    if let Some(alias) = &aggregate.alias {
        return alias.clone();
    }
    let column = aggregate.column.as_deref().unwrap_or_default();
    match aggregate.func {
        AggFunc::CountStar => "count".to_string(),
        AggFunc::Count => format!("count_{column}"),
        AggFunc::Sum => format!("sum_{column}"),
        AggFunc::Avg => format!("avg_{column}"),
        AggFunc::Min => format!("min_{column}"),
        AggFunc::Max => format!("max_{column}"),
    }
}

/// Indica si `column` aparece en la salida agregada (columna proyectada o agregado).
fn is_aggregate_output_column(select: &Select, column: &str) -> bool {
    let projected = match &select.projection {
        Projection::All => false,
        Projection::Columns(columns) => columns.iter().any(|name| name == column),
    };
    projected
        || select
            .aggregates
            .iter()
            .any(|aggregate| aggregate_output_key(aggregate) == column)
}

/// Ordena las filas agregadas por la cláusula `ORDER BY` (estable, fallible).
fn sort_rows(order_by: &OrderBy, rows: &mut [Row]) -> Result<(), RuscaError> {
    let mut failure: Option<RuscaError> = None;
    rows.sort_by(|left, right| {
        if failure.is_some() {
            return Ordering::Equal;
        }
        match compare_order_values(
            left.get(&order_by.column),
            right.get(&order_by.column),
            order_by.desc,
        ) {
            Ok(ordering) => ordering,
            Err(error) => {
                failure = Some(error);
                Ordering::Equal
            }
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
