//! Tests de aceptación de la coherencia del caché semántico (SPEC-0069).
//!
//! Cubre los criterios AC-0069-01..05: la invalidación por escritura
//! (conservando métricas), la firma de query que incluye tabla y `k`, el
//! rechazo de vectores con dimensiones distintas y el mantenimiento de
//! hits/misses tras vaciar las entradas.
//!
//! Estos tests son **RED por aserción** (no por símbolos): la API pública que
//! usan ya existe en la fachada `ruscadb` (SPEC-0065), por lo que la suite
//! compila, pero el caché actual (a) no se invalida al escribir, (b) usa como
//! clave solo el vector (ignora tabla y `k`) y (c) compara dimensiones con
//! `zip`, de modo que vectores de distinta dim pueden producir un falso hit.
//! Cada aserción reproduce un hallazgo P1 de la retrospectiva 3.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    CacheMetrics, ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta,
    Metric, Record, RecordId, RecordMeta, Row, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Dimensión de los vectores `items`/`otros` (coincide con sus queries KNN).
const VECTOR_DIM: usize = 3;

/// Número de vecinos solicitado en las consultas KNN de comparación (k=2).
const KNN_K: usize = 2;

/// Número de vecinos para los escenarios que necesitan `k=1`.
const KNN_K_SINGLE: usize = 1;

/// Umbral de coseno del caché semántico (hit si `cos >= 0.8`).
const SEMANTIC_CACHE_THRESHOLD: f64 = 0.8;

/// Vector de consulta canónico de las pruebas vectoriales (clave de caché).
const QUERY_VECTOR: [f64; VECTOR_DIM] = [0.1, 0.2, 0.3];

/// Vector casi idéntico a [`QUERY_VECTOR`] (cos ≈ 1.0) para provocar un hit.
const NEAR_QUERY_VECTOR: [f64; VECTOR_DIM] = [0.11, 0.21, 0.31];

/// Vector de la fila nueva del escenario de invalidación.
///
/// Es cercano a [`QUERY_VECTOR`] para que, tras invalidar, la KNN fresca lo
/// incluya entre los vecinos y se pueda distinguir de la respuesta obsoleta.
const NEW_ROW_VECTOR: [f64; VECTOR_DIM] = [0.11, 0.21, 0.31];

/// Vector de consulta 2D (dimensiones distintas a [`QUERY_VECTOR`]).
const TWO_D_VECTOR: [f64; 2] = [0.1, 0.2];

/// Identificador de la fila nueva insertada en el escenario de invalidación.
const NEW_ROW_ID: i64 = 5;

/// Identificador del vecino más cercano en la tabla `flat` (dim 2).
const FLAT_NEAR_ID: i64 = 21;

/// Identificador del vecino más cercano en la tabla `otros`.
const OTROS_NEAR_ID: i64 = 11;

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

/// Crea `<table>(id INT, name TEXT)`.
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

/// Construye la consulta KNN de prueba sobre una tabla y `k` dados.
///
/// Args:
///     table: Tabla destino (`FROM`).
///     k: Número de vecinos pedidos.
///     vector: Vector de consulta en `f64`.
///
/// Returns:
///     El texto RQL `SELECT * FROM <table> KNN embedding <|k|> [...]`.
fn knn_query(table: &str, k: usize, vector: &[f64]) -> String {
    let components: Vec<String> = vector.iter().map(|value| value.to_string()).collect();
    format!(
        "SELECT * FROM {table} KNN embedding <|{k}|> [{}]",
        components.join(", ")
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

/// Siembra una tabla con filas vectoriales `(id, name, vector)`.
///
/// Args:
///     database: Base destino (la tabla debe existir).
///     table: Nombre de la tabla a poblar.
///     data: Filas `(id, name, components)` a insertar.
fn seed_vectors(database: &mut Database, table: &str, data: &[(i64, &str, Vec<f64>)]) {
    for (id, name, vector) in data {
        let record = vector_record(RecordId::new(), scalars(*id, name), vector);
        database
            .insert_record(table, record)
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

/// Extrae la columna `id` de todas las filas, en orden.
///
/// Args:
///     rows: Filas devueltas por `execute`.
///
/// Returns:
///     Los identificadores enteros en el mismo orden que las filas.
fn ids_of(rows: &[Row]) -> Vec<i64> {
    rows.iter().map(|row| int_of(row, "id")).collect()
}

/// Calcula la similitud de coseno entre dos vectores (oráculo del test).
///
/// Contrato SPEC-0069 (FR-0069-03): si las dimensiones difieren, el coseno es
/// `0.0` (nunca un hit por truncado).
///
/// Args:
///     a: Primer vector.
///     b: Segundo vector.
///
/// Returns:
///     La similitud de coseno en `[-1.0, 1.0]` (o `0.0` si las dim difieren).
fn cosine(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// AC-0069-01 — una mutación invalida el caché y la KNN siguiente es miss.
///
/// Con el caché ON y una KNN ya cacheada, insertar una fila nueva (con un
/// embedding cercano a `v`) y repetir la KNN con `v'` (cos(v, v') >= 0.8) debe
/// ser MISS —hoy es HIT con filas obsoletas— y el resultado debe incluir la
/// fila nueva.
///
/// Nota: la fila nueva se inserta con [`Database::insert_record`] (lleva
/// `vector`) porque un registro sin embedding no es candidato del índice
/// vectorial y no podría aparecer en ningún KNN.
#[test]
// @spec AC-0069-01
fn test_ac_0069_01_write_invalidates_cache() {
    let (_dir, mut database) = open_test_db("ac0069_01");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    // Clamp determinista: v y v' superan el umbral de coseno.
    let similarity = cosine(&QUERY_VECTOR, &NEAR_QUERY_VECTOR);
    assert!(
        similarity >= SEMANTIC_CACHE_THRESHOLD,
        "v y v' deben ser semánticamente iguales (cos={similarity} < {SEMANTIC_CACHE_THRESHOLD})"
    );

    database.set_semantic_cache(true);

    // Given: una KNN con v ya cacheada (primer miss + almacenamiento).
    let first = database
        .execute(&knn_query("items", KNN_K, &QUERY_VECTOR))
        .expect("KNN con v");
    assert_eq!(
        ids_of(&first),
        [2, 1],
        "la consulta inicial debe devolver el vecindario de items"
    );

    // When: se inserta una fila nueva (con embedding cercano a v).
    let record = vector_record(
        RecordId::new(),
        scalars(NEW_ROW_ID, "nueva"),
        &NEW_ROW_VECTOR,
    );
    database
        .insert_record("items", record)
        .expect("insert_record de la fila nueva");

    // Then: la entrada anterior queda invalidada → la KNN es miss (no hit).
    let result = database
        .execute(&knn_query("items", KNN_K, &NEAR_QUERY_VECTOR))
        .expect("KNN con v' tras la mutación");

    let metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        metrics.hits, 0,
        "una mutación debe invalidar el caché: no puede servirse el hit obsoleto"
    );
    assert_eq!(
        metrics.misses, 2,
        "la KNN posterior a la mutación debe contabilizarse como miss"
    );

    // And: el resultado fresco incluye la fila nueva (sin datos obsoletos).
    let ids = ids_of(&result);
    assert!(
        ids.contains(&NEW_ROW_ID),
        "el resultado debe incluir la fila nueva id {NEW_ROW_ID} (sin datos obsoletos): {ids:?}"
    );
}

/// AC-0069-02 — la firma incluye la tabla (no hay hit cruzado).
///
/// Con una KNN cacheada sobre `items`, la misma KNN (mismo vector y `k`) sobre
/// `otros` debe ser MISS —hoy reutiliza el hit de `items`— y devolver las filas
/// de `otros`.
#[test]
// @spec AC-0069-02
fn test_ac_0069_02_signature_includes_table() {
    let (_dir, mut database) = open_test_db("ac0069_02");
    create_items(&mut database, "items");
    create_items(&mut database, "otros");
    seed_rows(&mut database);
    seed_vectors(
        &mut database,
        "otros",
        &[
            (10, "diez", vec![0.0, 0.0, 0.0]),
            (OTROS_NEAR_ID, "once", vec![0.1, 0.2, 0.3]),
            (12, "doce", vec![9.0, 9.0, 9.0]),
            (13, "trece", vec![3.0, 3.0, 3.0]),
        ],
    );

    database.set_semantic_cache(true);

    // Given: una KNN ya cacheada sobre items.
    let items_rows = database
        .execute(&knn_query("items", KNN_K, &QUERY_VECTOR))
        .expect("KNN sobre items");
    assert_eq!(
        ids_of(&items_rows),
        [2, 1],
        "la KNN sobre items devuelve el vecindario de items"
    );

    // When: la misma KNN (mismo vector y k) sobre otra tabla.
    let otros_rows = database
        .execute(&knn_query("otros", KNN_K, &QUERY_VECTOR))
        .expect("KNN sobre otros");

    // Then: la firma incluye la tabla → miss (no hit cruzado).
    let metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        metrics.hits, 0,
        "una KNN sobre otra tabla no puede reutilizar el hit de items"
    );
    assert_eq!(
        metrics.misses, 2,
        "cada tabla contabiliza su propio miss con el caché ON"
    );

    // And: se devuelven las filas de la tabla consultada, no las de items.
    assert_eq!(
        ids_of(&otros_rows),
        [OTROS_NEAR_ID, 10],
        "debe devolverse el vecindario de otros, no el de items"
    );
}

/// AC-0069-03 — la firma incluye `k` (no hay hit entre vecindarios distintos).
///
/// Con una KNN `k=1` cacheada con `v`, la KNN `k=2` con `v'` (cos(v, v') >= 0.8)
/// debe ser MISS —hoy reutiliza la única fila cacheada— y devolver 2 filas.
#[test]
// @spec AC-0069-03
fn test_ac_0069_03_signature_includes_k() {
    let (_dir, mut database) = open_test_db("ac0069_03");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    // Clamp determinista: v y v' superan el umbral de coseno.
    let similarity = cosine(&QUERY_VECTOR, &NEAR_QUERY_VECTOR);
    assert!(
        similarity >= SEMANTIC_CACHE_THRESHOLD,
        "v y v' deben ser semánticamente iguales (cos={similarity} < {SEMANTIC_CACHE_THRESHOLD})"
    );

    database.set_semantic_cache(true);

    // Given: una KNN k=1 cacheada con v.
    let single = database
        .execute(&knn_query("items", KNN_K_SINGLE, &QUERY_VECTOR))
        .expect("KNN k=1 con v");
    assert_eq!(
        single.len(),
        KNN_K_SINGLE,
        "la KNN k=1 debe devolver una sola fila"
    );

    // When: la KNN con k=2 y un vector similar a v.
    let double = database
        .execute(&knn_query("items", KNN_K, &NEAR_QUERY_VECTOR))
        .expect("KNN k=2 con v'");

    // Then: la firma incluye k → miss y 2 filas (no la única fila cacheada).
    let metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        metrics.hits, 0,
        "un k distinto no puede reutilizar el hit de k=1"
    );
    assert_eq!(
        metrics.misses, 2,
        "cada k contabiliza su propio miss con el caché ON"
    );
    assert_eq!(
        double.len(),
        KNN_K,
        "la KNN con k=2 debe devolver 2 filas, no la única fila cacheada de k=1"
    );
}

/// AC-0069-04 — dimensiones distintas nunca hacen hit.
///
/// Con una clave de dimensión 3 cacheada, consultar con un vector 2D sobre una
/// tabla de embeddings 2D debe ser MISS —hoy `cosine` trunca con `zip` y
/// produce un falso hit— y devolver las filas del motor.
#[test]
// @spec AC-0069-04
fn test_ac_0069_04_dimension_mismatch_never_hits() {
    let (_dir, mut database) = open_test_db("ac0069_04");
    create_items(&mut database, "items");
    create_items(&mut database, "flat");
    seed_rows(&mut database);
    seed_vectors(
        &mut database,
        "flat",
        &[
            (20, "plano-cero", vec![0.0, 0.0]),
            (FLAT_NEAR_ID, "plano-cerca", vec![0.1, 0.2]),
            (22, "plano-lejos", vec![9.0, 9.0]),
        ],
    );

    // Clamp: el coseno entre dimensiones distintas es 0.0 (nunca hit).
    assert_eq!(
        cosine(&QUERY_VECTOR, &TWO_D_VECTOR),
        0.0,
        "cosine con dimensiones distintas debe ser 0.0"
    );

    database.set_semantic_cache(true);

    // Given: una clave de dimensión 3 cacheada (KNN k=1 sobre items).
    let _probe = database
        .execute(&knn_query("items", KNN_K_SINGLE, &QUERY_VECTOR))
        .expect("KNN 3D sobre items");

    // When: se consulta con un vector 2D sobre una tabla de dim 2.
    let result = database
        .execute(&knn_query("flat", KNN_K_SINGLE, &TWO_D_VECTOR))
        .expect("KNN 2D sobre flat");

    // Then: dimensiones distintas → sin hit; el motor resuelve la consulta.
    let metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        metrics.hits, 0,
        "dimensiones distintas no pueden producir un hit"
    );
    assert_eq!(
        metrics.misses, 2,
        "la consulta 2D debe contabilizarse como miss"
    );
    assert_eq!(
        int_of(&result[0], "id"),
        FLAT_NEAR_ID,
        "debe devolverse la fila del motor de la tabla de dimensión 2"
    );
}

/// AC-0069-05 — la invalidación vacía entradas pero conserva hits/misses.
///
/// Con 1 hit acumulado, una mutación deja `entries == 0` y conserva
/// `hits == 1`; la KNN siguiente con `v` vuelve a ser miss.
#[test]
// @spec AC-0069-05
fn test_ac_0069_05_invalidation_keeps_metrics() {
    let (_dir, mut database) = open_test_db("ac0069_05");
    create_items(&mut database, "items");
    seed_rows(&mut database);

    database.set_semantic_cache(true);

    // Given: se acumula un miss (v) y un hit (v' ≈ v).
    let _prime = database
        .execute(&knn_query("items", KNN_K, &QUERY_VECTOR))
        .expect("KNN con v");
    let _hit = database
        .execute(&knn_query("items", KNN_K, &NEAR_QUERY_VECTOR))
        .expect("KNN con v'");
    let with_hit: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(with_hit.hits, 1, "se acumula un hit antes de la mutación");

    // When: se ejecuta una mutación.
    database
        .insert("items", scalars(NEW_ROW_ID, "nueva"))
        .expect("insert de la fila nueva");

    // Then: las entradas quedan a cero pero las métricas se conservan.
    let after_write: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        after_write.entries, 0,
        "la mutación debe vaciar las entradas del caché"
    );
    assert_eq!(
        after_write.hits, 1,
        "la invalidación debe conservar los hits acumulados"
    );
    assert_eq!(
        after_write.misses, 1,
        "la invalidación debe conservar los misses acumulados"
    );

    // And: la consulta siguiente vuelve a ser miss.
    let _fresh = database
        .execute(&knn_query("items", KNN_K, &QUERY_VECTOR))
        .expect("KNN tras la mutación");
    let final_metrics: CacheMetrics = database.semantic_cache_metrics();
    assert_eq!(
        final_metrics.misses, 2,
        "la query posterior a la mutación debe contabilizarse como miss"
    );
    assert_eq!(
        final_metrics.hits, 1,
        "el hit previo a la mutación se conserva"
    );
}
