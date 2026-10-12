//! Tests de aceptación del caché semántico + coste por query (SPEC-0065).
//!
//! Cubre los criterios AC-0065-01..05: hit por similitud de coseno de la
//! consulta KNN, OFF por defecto (cero regresión), reseteo explícito de las
//! métricas, miss con un vector no similar y el contador acumulable/reseteable
//! de coste por query.
//!
//! Estos tests son RED por diseño: la API de SPEC-0065 aún no existe en la
//! fachada `ruscadb`, por lo que la compilación falla al resolver los símbolos
//! `CacheMetrics`, `CostReport`, `Database::set_semantic_cache`,
//! `Database::semantic_cache_metrics`, `Database::clear_semantic_cache`,
//! `Database::cost_report` y `Database::reset_cost_report`.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    CacheMetrics, ColumnDef, ColumnType, CostReport, Database, DbConfig, EdgeSet, Embedding,
    EmbeddingMeta, Metric, Record, RecordId, RecordMeta, Row, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Dimensión de los vectores de prueba (coincide con las queries KNN).
const VECTOR_DIM: usize = 3;

/// Número de vecinos solicitado en las consultas KNN de comparación.
const KNN_K: usize = 2;

/// Umbral de coseno del caché semántico (hit si `cos >= 0.8`).
const SEMANTIC_CACHE_THRESHOLD: f64 = 0.8;

/// Vector de consulta canónico de las pruebas vectoriales (clave de caché).
const QUERY_VECTOR: [f64; VECTOR_DIM] = [0.1, 0.2, 0.3];

/// Vector casi idéntico a [`QUERY_VECTOR`] (cos ≈ 1.0) para provocar un hit.
const NEAR_QUERY_VECTOR: [f64; VECTOR_DIM] = [0.11, 0.21, 0.31];

/// Clave previa del test de miss: casi ortogonal a [`ORTHOGONAL_QUERY_VECTOR`].
///
/// `cos(PRIOR_KEY_VECTOR, ORTHOGONAL_QUERY_VECTOR) ≈ 0.13 < 0.8`, de modo que
/// una consulta con el vector ortogonal NO puede reutilizar esta clave.
const PRIOR_KEY_VECTOR: [f64; VECTOR_DIM] = [0.3, -0.3, 0.1];

/// Vector ortogonal a la clave previa: su vecino más cercano es "lejos".
const ORTHOGONAL_QUERY_VECTOR: [f64; VECTOR_DIM] = [9.0, 9.0, 9.0];

/// Consulta RQL simple (sin KNN) para el contador de coste.
const SIMPLE_QUERY: &str = "SELECT * FROM items";

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database =
        Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura de la base");
    (dir, database)
}

/// Crea `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base destino.
///     table: Nombre de la tabla a crear.
fn create_items(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Construye escalares `(id, name)`.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     name: Valor textual de la columna `name`.
///
/// Returns:
///     El mapa de escalares listo para un registro.
fn scalars(id: i64, name: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ])
}

/// Crea un embedding L2 de la dimensión del vector.
///
/// Args:
///     values: Componentes del vector en `f64`.
///
/// Returns:
///     El embedding L2 construido.
fn embedding(values: &[f64]) -> Embedding {
    let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
    Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "test".to_string(),
            dim: values.len(),
            metric: Metric::L2,
        },
    )
    .expect("embedding válido")
}

/// Construye un `Record` con escalares y vector (sin aristas).
///
/// Args:
///     id: Identificador del registro.
///     scalars: Escalares del registro.
///     vector: Componentes del embedding.
///
/// Returns:
///     El registro listo para `insert_record`.
fn vector_record(id: RecordId, scalars: ScalarMap, vector: &[f64]) -> Record {
    Record {
        id,
        scalars,
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(embedding(vector)),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Construye la consulta KNN de prueba sobre `items`.
///
/// Args:
///     vector: Vector de consulta en `f64`.
///
/// Returns:
///     El texto RQL `SELECT * FROM items KNN embedding <|k|> [...]`.
fn knn_query(vector: &[f64; VECTOR_DIM]) -> String {
    format!(
        "SELECT * FROM items KNN embedding <|{KNN_K}|> [{}, {}, {}]",
        vector[0], vector[1], vector[2]
    )
}

/// Siembra `items` con cuatro filas vectoriales deterministas.
///
/// Los dos puntos más cercanos a [`QUERY_VECTOR`] son siempre id 2 ("cerca") e
/// id 1 ("origen"): el resto queda a distancia L2 manifiestamente mayor.
///
/// Args:
///     database: Base destino (debe tener la tabla `items`).
fn seed_rows(database: &mut Database) {
    let data: [(i64, &str, [f64; VECTOR_DIM]); 4] = [
        (1, "origen", [0.0, 0.0, 0.0]),
        (2, "cerca", [0.1, 0.2, 0.3]),
        (3, "lejos", [9.0, 9.0, 9.0]),
        (4, "medio", [3.0, 3.0, 3.0]),
    ];
    for (id, name, vector) in data {
        let record = vector_record(RecordId::new(), scalars(id, name), &vector);
        database
            .insert_record("items", record)
            .expect("insert_record");
    }
}

/// Extrae el valor entero de una columna `Int` de una fila.
///
/// Args:
///     row: Fila devuelta por `execute`.
///     column: Nombre de la columna entera.
///
/// Returns:
///     El valor entero de la columna.
fn int_of(row: &Row, column: &str) -> i64 {
    match row.get(column) {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en '{column}', se obtuvo {other:?}"),
    }
}

/// Extrae el valor textual de una columna de una fila.
///
/// Args:
///     row: Fila devuelta por `execute`.
///     column: Nombre de la columna textual.
///
/// Returns:
///     El valor textual de la columna.
fn text_of(row: &Row, column: &str) -> String {
    match row.get(column) {
        Some(ScalarValue::Text(value)) => value.clone(),
        other => panic!("se esperaba Text en '{column}', se obtuvo {other:?}"),
    }
}

/// Calcula la similitud de coseno entre dos vectores (oráculo del test).
///
/// Args:
///     a: Primer vector.
///     b: Segundo vector.
///
/// Returns:
///     La similitud de coseno en `[-1.0, 1.0]`.
fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    dot / (norm_a * norm_b)
}

/// AC-0065-01 — una consulta casi idéntica reutiliza el resultado (hit).
///
/// Con la clave `v` ya almacenada por la consulta sonda, habilitar el caché y
/// consultar `v'` (cos(v, v') >= 0.8) devuelve exactamente el resultado de `v`,
/// con `hits == 1` y `misses == 0`.
#[test]
// @spec AC-0065-01
fn test_ac_0065_01_knn_semantic_hit() {
    let (_dir, mut database) = open_test_db("ac0065_01");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    // Clamp determinista: el vector cercano supera el umbral de coseno.
    let similarity = cosine(&QUERY_VECTOR, &NEAR_QUERY_VECTOR);
    assert!(
        similarity >= SEMANTIC_CACHE_THRESHOLD,
        "v y v' deben ser semánticamente iguales (cos={similarity} < {SEMANTIC_CACHE_THRESHOLD})"
    );

    // La cache OFF por defecto solo sondea/almacena, sin contar métricas.
    let result_a = database
        .execute(&knn_query(&QUERY_VECTOR))
        .expect("KNN con v");

    database.set_semantic_cache(true);

    // Given: cache habilitado y v' con cos >= 0.8 respecto a v.
    let result_b = database
        .execute(&knn_query(&NEAR_QUERY_VECTOR))
        .expect("KNN con v'");

    // Then: hit servido desde la cache, idéntico al resultado original.
    assert_eq!(
        result_b, result_a,
        "un hit debe devolver exactamente el resultado cacheado de v"
    );

    let metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        metrics.hits, 1,
        "la consulta v' debe contabilizarse como hit"
    );
    assert_eq!(
        metrics.misses, 0,
        "no debe haber misses con el caché caliente"
    );
}

/// AC-0065-02 — OFF por defecto: sin reutilización y resultados correctos.
///
/// Dos consultas KNN idénticas con el caché deshabilitado devuelven los mismos
/// resultados y contabilizan `hits == 0`.
#[test]
// @spec AC-0065-02
fn test_ac_0065_02_cache_off_by_default() {
    let (_dir, mut database) = open_test_db("ac0065_02");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    // Given/When: dos consultas KNN idénticas con el caché OFF por defecto.
    let first = database.execute(&knn_query(&QUERY_VECTOR)).expect("KNN 1");
    let second = database.execute(&knn_query(&QUERY_VECTOR)).expect("KNN 2");

    // Then: resultados idénticos entre sí, servidos por el motor (sin hits).
    assert_eq!(first, second, "sin caché ambas consultas son idénticas");

    let metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        metrics.hits, 0,
        "OFF por defecto nunca debe contabilizar hits"
    );

    let ids: Vec<i64> = first.iter().map(|row| int_of(row, "id")).collect();
    assert_eq!(ids, [2, 1], "el KNN devuelve el vecindario correcto");
}

/// AC-0065-03 — `clear_semantic_cache` resetea hits/misses y vuelve a miss.
///
/// Con hits acumulados, el reseteo deja las métricas a cero y una consulta
/// nueva vuelve a contabilizarse como miss.
#[test]
// @spec AC-0065-03
fn test_ac_0065_03_cache_clear_resets_metrics() {
    let (_dir, mut database) = open_test_db("ac0065_03");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    database.set_semantic_cache(true);

    // Given: se acumula un hit (primera consulta miss + segunda hit).
    let _prime = database
        .execute(&knn_query(&QUERY_VECTOR))
        .expect("KNN con v");
    let _hit = database
        .execute(&knn_query(&NEAR_QUERY_VECTOR))
        .expect("KNN con v'");
    let with_hits: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(with_hits.hits, 1, "se acumula un hit antes del clear");

    // When: se limpia el caché.
    database.clear_semantic_cache();

    // Then: las métricas vuelven a cero.
    let cleared: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(cleared.hits, 0, "clear resetea hits");
    assert_eq!(cleared.misses, 0, "clear resetea misses");
    assert_eq!(cleared.entries, 0, "clear vacía las entradas");

    // And: una consulta nueva vuelve a ser miss de forma determinista.
    let _fresh = database
        .execute(&knn_query(&QUERY_VECTOR))
        .expect("KNN tras clear");
    let after: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        after.misses, 1,
        "una consulta nueva tras clear vuelve a contabilizarse como miss"
    );
}

/// AC-0065-04 — el coste por query acumula N consultas y filas, y se resetea.
///
/// Tres `execute` (mezcla `SELECT` y KNN) dejan `queries == 3`,
/// `rows_returned` igual a la suma real de filas y `nanos > 0`;
/// `reset_cost_report` lo pone todo a cero y una consulta más deja
/// `queries == 1`.
#[test]
// @spec AC-0065-04
fn test_ac_0065_04_cost_report_accumulates_and_resets() {
    let (_dir, mut database) = open_test_db("ac0065_04");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    // Given/When: N = 3 consultas (1 SELECT simple + 2 KNN).
    let select_rows = database.execute(SIMPLE_QUERY).expect("SELECT simple");
    let knn_rows = database.execute(&knn_query(&QUERY_VECTOR)).expect("KNN 1");
    let knn_other_rows = database
        .execute(&knn_query(&NEAR_QUERY_VECTOR))
        .expect("KNN 2");

    assert_eq!(
        select_rows.len(),
        4,
        "el SELECT simple devuelve las 4 filas"
    );
    assert_eq!(knn_rows.len(), KNN_K, "el primer KNN devuelve k filas");
    assert_eq!(
        knn_other_rows.len(),
        KNN_K,
        "el segundo KNN devuelve k filas"
    );

    let expected_rows = (select_rows.len() + knn_rows.len() + knn_other_rows.len()) as u64;

    // Then: el reporte acumula consultas, filas y tiempo positivo.
    let report: CostReport = database.cost_report();
    assert_eq!(
        report.queries, 3,
        "se contabilizan las 3 consultas ejecutadas"
    );
    assert_eq!(
        report.rows_returned, expected_rows,
        "rows_returned es la suma de filas devueltas por cada consulta"
    );
    assert!(report.nanos > 0, "el tiempo acumulado debe ser positivo");

    // When: se resetea el reporte.
    database.reset_cost_report();

    // Then: todo queda a cero.
    let reset: CostReport = database.cost_report();
    assert_eq!(reset.queries, 0, "reset deja queries a cero");
    assert_eq!(reset.rows_returned, 0, "reset deja rows_returned a cero");
    assert_eq!(reset.nanos, 0, "reset deja nanos a cero");

    // And: una consulta adicional vuelve a contar desde 1.
    let _after = database.execute(SIMPLE_QUERY).expect("SELECT tras reset");
    let after: CostReport = database.cost_report();
    assert_eq!(after.queries, 1, "tras el reset la nueva consulta cuenta 1");
}

/// AC-0065-05 — un vector no similar produce miss y resultado del motor.
///
/// Con una clave previa almacenada, una consulta KNN con un vector
/// casi ortogonal (cos < 0.8) contabiliza `misses == 1`, no reutiliza la clave
/// y devuelve el vecino más cercano real del motor.
#[test]
// @spec AC-0065-05
fn test_ac_0065_05_distinct_vector_misses() {
    let (_dir, mut database) = open_test_db("ac0065_05");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    // Clamp determinista: el vector de consulta está bajo el umbral del key.
    let similarity = cosine(&PRIOR_KEY_VECTOR, &ORTHOGONAL_QUERY_VECTOR);
    assert!(
        similarity < SEMANTIC_CACHE_THRESHOLD,
        "el vector ortogonal no debe ser similar a la clave (cos={similarity} >= {SEMANTIC_CACHE_THRESHOLD})"
    );

    // Given: clave previa almacenada por una consulta KNN (cache OFF).
    let _prior = database
        .execute(&knn_query(&PRIOR_KEY_VECTOR))
        .expect("KNN de la clave previa");

    database.set_semantic_cache(true);

    // When: consulta KNN con el vector casi ortogonal.
    let rows = database
        .execute(&knn_query(&ORTHOGONAL_QUERY_VECTOR))
        .expect("KNN ortogonal");

    // Then: miss sin reutilización.
    let metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        metrics.misses, 1,
        "un vector no similar debe contabilizar miss"
    );
    assert_eq!(metrics.hits, 0, "no debe servirse la clave no similar");

    // And: el resultado es el vecindario real del motor.
    assert_eq!(
        int_of(&rows[0], "id"),
        3,
        "el vecino más cercano a [9,9,9] es id 3"
    );
    assert_eq!(
        text_of(&rows[0], "name"),
        "lejos",
        "el vecino más cercano a [9,9,9] se llama 'lejos'"
    );
}
