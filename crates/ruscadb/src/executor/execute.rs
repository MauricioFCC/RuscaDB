//! Orquestacion de `SELECT`: candidatos, visibilidad y clausulas.

use super::*;

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
#[allow(dead_code)] // API interna histórica; el executor usa `_ctx`
pub(crate) fn execute_select_at(
    database: &mut Database,
    select: &Select,
    snapshot: &Snapshot,
) -> Result<Vec<Row>, RuscaError> {
    execute_select_at_ctx(database, select, snapshot, &BTreeMap::new())
}

/// Ejecuta un `SELECT` con un contexto de CTEs ya materializados (SPEC-0163).
///
/// Args:
///     database: Base abierta.
///     select: Consulta analizada.
///     snapshot: Vista fija de visibilidad MVCC.
///     ctes: Tablas virtuales visibles por nombre (`WITH ...`).
///
/// Returns:
///     Filas visibles para `snapshot` (hasta `LIMIT`).
///
/// Errors:
///     Los del executor (`TableNotFound`, `ColumnNotFound`, `TypeMismatch`).
pub(crate) fn execute_select_at_ctx(
    database: &mut Database,
    select: &Select,
    snapshot: &Snapshot,
    ctes: &BTreeMap<String, VirtualTable>,
) -> Result<Vec<Row>, RuscaError> {
    execute_select_inner(database, select, snapshot, ctes, None)
}

/// Ejecuta un `SELECT` con un plan ya calculado (caché, SPEC-0203).
pub(crate) fn execute_select_with_plan(
    database: &mut Database,
    select: &Select,
    plan: Plan,
    snapshot: &Snapshot,
) -> Result<Vec<Row>, RuscaError> {
    execute_select_inner(database, select, snapshot, &BTreeMap::new(), Some(plan))
}

/// Núcleo de un `SELECT`: presupuesto, subconsultas, límite y plan.
fn execute_select_inner(
    database: &mut Database,
    select: &Select,
    snapshot: &Snapshot,
    ctes: &BTreeMap<String, VirtualTable>,
    plan_override: Option<Plan>,
) -> Result<Vec<Row>, RuscaError> {
    // Planner (SPEC-0054): budget pre-ejecución + LIMIT implícito + deadline.
    check_budget(select)?;
    let mut effective = select.clone();
    if let Some(filter) = effective.filter.clone() {
        effective.filter = Some(materialize_subqueries_ctx(
            database, &filter, snapshot, ctes,
        )?);
    }
    if effective.limit.is_none() {
        effective.limit = Some(IMPLICIT_LIMIT);
    }
    let budget = database.query_budget();
    if effective.from_derived.is_some() || ctes.contains_key(&effective.from) {
        return execute_virtual_select(database, &effective, snapshot, &budget, ctes);
    }
    let plan = match plan_override {
        Some(plan) => plan,
        None => {
            let catalog = Catalog::load(database)?;
            let stats = database.stats(&effective.from);
            plan_for(&effective, &catalog, stats)?
        }
    };
    execute_with_plan_at_ctx(database, &effective, plan, snapshot, &budget, ctes)
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
    let budget = QueryBudget::default_budget();
    execute_with_plan_at(database, select, plan, &snapshot, &budget)
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
///     cláusulas que no encajan con los datos de la tabla;
///     [`RuscaError::QueryTimeout`] si vence el deadline del scan (SPEC-0054).
#[allow(dead_code)] // usado por tests del planner; el executor usa `_ctx`
pub(crate) fn execute_with_plan_at(
    database: &mut Database,
    select: &Select,
    plan: Plan,
    snapshot: &Snapshot,
    budget: &QueryBudget,
) -> Result<Vec<Row>, RuscaError> {
    execute_with_plan_at_ctx(database, select, plan, snapshot, budget, &BTreeMap::new())
}

/// Igual que [`execute_with_plan_at`], con CTEs visibles por nombre.
pub(crate) fn execute_with_plan_at_ctx(
    database: &mut Database,
    select: &Select,
    plan: Plan,
    snapshot: &Snapshot,
    budget: &QueryBudget,
    ctes: &BTreeMap<String, VirtualTable>,
) -> Result<Vec<Row>, RuscaError> {
    if select.from_derived.is_some() || ctes.contains_key(&select.from) {
        return execute_virtual_select(database, select, snapshot, budget, ctes);
    }
    if let Some(join) = &select.join {
        return crate::join::execute_join_at(database, select, join, snapshot);
    }
    let table = Catalog::load(database)?.get(&select.from)?.clone();
    let candidates = fetch_candidates(database, &table, &plan, select.filter.as_ref())?;
    let (matches, scalar_filter) = split_matches(select.filter.as_ref());
    let mut records = Vec::new();
    for (index, (_, record)) in candidates.into_iter().enumerate() {
        budget.check(index as u64)?;
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
        reject_extras_with_aggregates(select)?;
        return aggregate_records(&table, select, &records);
    }
    if let Some(order_by) = &select.order_by {
        sort_records(&table, order_by, &mut records)?;
    }
    let mut rows = project_records(database, &table, select, &records, snapshot)?;
    if select.distinct {
        dedup_rows(&mut rows);
    }
    let offset = select.offset.unwrap_or(0) as usize;
    let limit = select.limit.map_or(usize::MAX, |value| value as usize);
    Ok(rows.into_iter().skip(offset).take(limit).collect())
}
/// Ejecuta un `SELECT` sobre una tabla virtual (derivada o CTE).
///
/// Reutiliza el pipeline de filas (filtro, agregación, orden, proyección,
/// ventanas, expresiones y paths) construyendo un [`TableDef`] sintético y
/// registros efímeros. `JOIN`/`KNN`/`TRAVERSE`/`MATCH` quedan fuera de alcance
/// sobre tablas virtuales (error accionable).
///
/// Args:
///     database: Base abierta (para subconsultas y paths de grafo).
///     select: Consulta analizada.
///     snapshot: Vista fija de visibilidad MVCC.
///     budget: Presupuesto de ejecución.
///     ctes: CTEs visibles por nombre.
///
/// Returns:
///     Filas proyectadas de la tabla virtual.
///
/// Errors:
///     [`RuscaError::TableNotFound`] si el CTE no existe;
///     [`RuscaError::TypeMismatch`] si se usa una cláusula no soportada.
pub(crate) fn execute_virtual_select(
    database: &mut Database,
    select: &Select,
    snapshot: &Snapshot,
    budget: &QueryBudget,
    ctes: &BTreeMap<String, VirtualTable>,
) -> Result<Vec<Row>, RuscaError> {
    if select.join.is_some() {
        return Err(RuscaError::TypeMismatch {
            message: "JOIN sobre tabla virtual no está soportado (SPEC-0162)".to_string(),
        });
    }
    if select.knn.is_some() || select.traverse.is_some() {
        return Err(RuscaError::TypeMismatch {
            message: "KNN/TRAVERSE sobre tabla virtual no está soportado".to_string(),
        });
    }
    let table = match &select.from_derived {
        Some(derived) => VirtualTable::materialize_select(database, derived, snapshot, ctes)?,
        None => ctes
            .get(&select.from)
            .cloned()
            .ok_or_else(|| RuscaError::TableNotFound {
                table: select.from.clone(),
            })?,
    };
    let definition = table.table_def(&select.from);
    let (matches, scalar_filter) = split_matches(select.filter.as_ref());
    if !matches.is_empty() {
        return Err(RuscaError::TypeMismatch {
            message: "MATCH no aplica a tablas virtuales".to_string(),
        });
    }
    let mut records = Vec::new();
    for record in table.records() {
        budget.check(records.len() as u64)?;
        if keeps_row(&definition, scalar_filter.as_ref(), &record)? {
            records.push(record);
        }
    }
    if is_aggregate_query(select) {
        reject_extras_with_aggregates(select)?;
        return aggregate_records(&definition, select, &records);
    }
    if let Some(order_by) = &select.order_by {
        sort_records(&definition, order_by, &mut records)?;
    }
    let mut rows = project_records(database, &definition, select, &records, snapshot)?;
    if select.distinct {
        dedup_rows(&mut rows);
    }
    let offset = select.offset.unwrap_or(0) as usize;
    let limit = select.limit.map_or(usize::MAX, |value| value as usize);
    Ok(rows.into_iter().skip(offset).take(limit).collect())
}

/// Proyecta columnas, ventanas, expresiones y paths en una sola pasada.
///
/// Args:
///     database: Base abierta (para resolver paths de grafo).
///     table: Definición de la tabla (real o virtual).
///     select: Consulta analizada.
///     records: Registros ya filtrados y ordenados.
///
/// Returns:
///     Una fila por registro con todas las columnas de salida.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna proyectada no existe;
///     errores de evaluación de expresiones o ventanas.
pub(crate) fn project_records(
    database: &mut Database,
    table: &TableDef,
    select: &Select,
    records: &[Record],
    snapshot: &Snapshot,
) -> Result<Vec<Row>, RuscaError> {
    let windows = evaluate_windows(&select.windows, records, table)?;
    let paths = evaluate_paths(database, &select.paths, records, &table.name, snapshot)?;
    let mut rows = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        let mut row = project_row(table, &select.projection, &record.scalars)?;
        for (window, values) in select.windows.iter().zip(&windows) {
            row.insert(window.output_name(), values[index].clone());
        }
        for expression in &select.expressions {
            let value = eval_scalar(&expression.expr, &record.scalars)?;
            let name = expression
                .alias
                .clone()
                .unwrap_or_else(|| expression.expr.to_string());
            row.insert(name, value);
        }
        for (path, values) in select.paths.iter().zip(&paths) {
            row.insert(path.output_name(), values[index].clone());
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Rechaza ventanas/expresiones/paths combinados con agregación.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si la consulta mezcla ambas formas.
pub(crate) fn reject_extras_with_aggregates(select: &Select) -> Result<(), RuscaError> {
    if select.windows.is_empty() && select.expressions.is_empty() && select.paths.is_empty() {
        return Ok(());
    }
    Err(RuscaError::TypeMismatch {
        message: "ventanas, expresiones y paths no se combinan con GROUP BY/agregados".to_string(),
    })
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
pub(crate) fn fetch_candidates(
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
            Some(literal) => fetch_by_index(database, table, column, &literal),
            None => heap_scan(database, table),
        },
    }
}
/// Separa los predicados `MATCH` del resto del `WHERE`.
///
/// Returns:
///     `(matches, filtro_escalar)`: la lista `(columna, texto)` de los `MATCH`
///     hallados y el `WHERE` sin ellos (`None` si no queda nada).
pub(crate) fn split_matches(filter: Option<&Expr>) -> (Vec<(String, String)>, Option<Expr>) {
    let mut matches = Vec::new();
    let scalar = collect_matches(filter, &mut matches);
    (matches, scalar)
}
/// Recursivamente extrae los `MATCH` y reconstruye el filtro escalar.
pub(crate) fn collect_matches(
    expression: Option<&Expr>,
    matches: &mut Vec<(String, String)>,
) -> Option<Expr> {
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
pub(crate) fn combine_and(left: Option<Expr>, right: Option<Expr>) -> Option<Expr> {
    match (left, right) {
        (Some(left), Some(right)) => Some(Expr::And(Box::new(left), Box::new(right))),
        (Some(single), None) | (None, Some(single)) => Some(single),
        (None, None) => None,
    }
}
/// Lee por índice los registros que igualan el literal.
///
/// Args:
///     database: Base abierta.
///     table: Tabla dueña del índice.
///     column: Columna del `IndexScan` (SPEC-0104: puede haber varios índices).
///     literal: Valor de igualdad.
///
/// Returns:
///     Los registros leídos por sus localizadores.
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si la tabla no tiene índice en la columna;
///     [`RuscaError::CorruptManifest`] si una página del índice/heap falla.
pub(crate) fn fetch_by_index(
    database: &mut Database,
    table: &TableDef,
    column: &str,
    literal: &ScalarValue,
) -> Result<Vec<(crate::heap::RowLocator, Record)>, RuscaError> {
    let mut rows = Vec::new();
    for locator in index_lookup_eq(database, table, column, literal)? {
        rows.push((locator, heap_read(database, locator)?));
    }
    Ok(rows)
}
/// Busca `columna = literal` en una conjunción `AND` (ambos órdenes).
pub(crate) fn find_eq_literal(filter: Option<&Expr>, column: &str) -> Option<ScalarValue> {
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
pub(crate) fn eq_literal_sides(left: &Expr, right: &Expr, column: &str) -> Option<ScalarValue> {
    if let Expr::Column(name) = left
        && name == column
    {
        return expr_literal(right);
    }
    if let Expr::Column(name) = right
        && name == column
    {
        return expr_literal(left);
    }
    None
}
