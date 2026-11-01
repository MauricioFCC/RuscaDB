//! Clausulas `MATCH`, `KNN` y `TRAVERSE` sobre los indices en memoria.

use super::*;

/// Aplica los `MATCH` (conjunción) y ordena por relevancia BM25.
pub(crate) fn apply_matches(
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
/// Aplica `KNN`: top-k según el modo declarado con `USING`.
///
/// `records` ya viene filtrado por visibilidad MVCC y por el `WHERE` escalar,
/// de modo que sus ids son el conjunto `allowed`.
///
/// - [`KnnMode::Auto`] (por defecto): con `WHERE` delega en
///   `ifvs_vector_search` (FVS pre/in/post por selectividad, SPEC-0048); sin
///   `WHERE` usa `filtered_vector_search` (KNN clásico).
/// - [`KnnMode::Quantized`]: ANN en dos etapas sobre el índice cuantizado
///   (`search_vectors_quantized`, RaBitQ + re-rank) y recorte a `allowed`.
/// - [`KnnMode::Filtered`]: ACORN filtrado (`search_vectors_filtered`, SPEC-0172),
///   que respeta `allowed` durante el recorrido.
/// - [`KnnMode::Exact`]: fuerza bruta exacta sobre los candidatos visibles.
///
/// Args:
///     database: Base abierta.
///     table: Definición de la tabla.
///     knn: Cláusula `KNN` (columna, `k`, vector de consulta y modo).
///     records: Candidatos visibles y filtrados por `WHERE` (conjunto `allowed`).
///     has_filter: `true` si la consulta lleva `WHERE` (filtro escalar).
///
/// Returns:
///     Los registros del top-k, en orden de distancia ascendente.
///
/// Errors:
///     [`RuscaError::MissingVector`] si la tabla no tiene índice vectorial;
///     [`RuscaError::DimensionMismatch`] si la dimensión de la consulta no
///     coincide con el índice.
pub(crate) fn apply_knn(
    database: &mut Database,
    table: &TableDef,
    knn: &KnnClause,
    records: Vec<Record>,
    has_filter: bool,
) -> Result<Vec<Record>, RuscaError> {
    let query: Vec<f32> = knn.query.iter().map(|value| *value as f32).collect();
    let k = knn.k as usize;
    if k == 0 {
        return Ok(Vec::new());
    }
    let (served, expected) = {
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
        let allowed: BTreeSet<RecordId> = records.iter().map(|record| record.id).collect();
        let metric = index.vector_metric();
        let mut served: Vec<RecordId> = match knn.mode {
            KnnMode::Auto => {
                let ranked = if has_filter {
                    index.ifvs_vector_search(&query, k, &allowed)?
                } else {
                    index.filtered_vector_search(&query, k, &allowed)?
                };
                ranked.into_iter().map(|(id, _)| id).collect()
            }
            KnnMode::Quantized => database
                .search_vectors_quantized(&table.name, &query, k)?
                .into_iter()
                .map(|(id, _)| id)
                .collect(),
            KnnMode::Filtered => database
                .search_vectors_filtered(&table.name, &query, k, &allowed)?
                .into_iter()
                .map(|(id, _)| id)
                .collect(),
            KnnMode::Exact => Vec::new(),
        };
        let expected = exact_top_k(&records, &query, k, metric);
        if knn.mode == KnnMode::Exact {
            served = expected.clone();
        } else {
            served.retain(|id| allowed.contains(id));
            served.truncate(k);
        }
        (served, expected)
    };
    if database.shadow_reads_enabled() {
        let label = format!("KNN {} k={k} mode={}", knn.column, knn.mode.as_str());
        database.record_shadow_check(label, expected, served.clone());
    }
    let mut by_id: BTreeMap<RecordId, Record> = records
        .into_iter()
        .map(|record| (record.id, record))
        .collect();
    Ok(served
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect())
}
/// Top-k exacto por distancia real sobre los vectores de `records` (SPEC-0062).
///
/// Es el oráculo brute-force de shadow reads: usa
/// [`ruscadb_vector::distance`] con la métrica del índice, ignora los registros
/// sin vector o con dimensión incompatible (el índice ya rechaza esa consulta)
/// y desempata por `RecordId` para ser determinista.
///
/// Args:
///     records: Candidatos visibles (tras WHERE/MVCC).
///     query: Vector de consulta.
///     k: Número máximo de resultados.
///     metric: Métrica del índice vectorial (`None` si no hay vectores).
///
/// Returns:
///     Hasta `k` `RecordId` en orden de distancia ascendente.
pub(crate) fn exact_top_k(
    records: &[Record],
    query: &[f32],
    k: usize,
    metric: Option<Metric>,
) -> Vec<RecordId> {
    let Some(metric) = metric else {
        return Vec::new();
    };
    let mut scored: Vec<(f32, RecordId)> = records
        .iter()
        .filter_map(|record| {
            let embedding = record.vector.as_ref()?;
            let distance = ruscadb_vector::distance(metric, query, &embedding.values).ok()?;
            Some((distance, record.id))
        })
        .collect();
    scored.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    scored.into_iter().take(k).map(|(_, id)| id).collect()
}
/// Aplica `TRAVERSE`: recorre el grafo desde las raíces de los candidatos.
pub(crate) fn apply_traverse(
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
pub(crate) fn sort_records(
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
pub(crate) fn compare_order_values(
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
pub(crate) fn order_with_nulls_last(
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
pub(crate) fn non_null(value: Option<&ScalarValue>) -> Option<&ScalarValue> {
    match value {
        Some(ScalarValue::Null) | None => None,
        Some(other) => Some(other),
    }
}
