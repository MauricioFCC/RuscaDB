//! Tests de aceptación de la persistencia del caché semántico (SPEC-0093).
//!
//! Cubre AC-0093-01..04: el caché KNN sobrevive a un `close()`/reopen (con hit
//! servido desde la clave restaurada), una mutación invalida y retira el
//! sidecar, un sidecar corrupto o con versión desconocida se ignora sin romper
//! la apertura, y una base cifrada no persiste ni confía en el sidecar en claro.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    CacheMetrics, ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta,
    EncryptionConfig, Metric, Record, RecordId, RecordMeta, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Dimensión de los vectores de prueba (coincide con las queries KNN).
const VECTOR_DIM: usize = 3;

/// Número de vecinos solicitado en las consultas KNN.
const KNN_K: usize = 2;

/// Vector de consulta canónico (clave del caché).
const QUERY_VECTOR: [f64; VECTOR_DIM] = [0.1, 0.2, 0.3];

/// Vector casi idéntico a [`QUERY_VECTOR`] (cos ≈ 1.0) para provocar un hit.
const NEAR_QUERY_VECTOR: [f64; VECTOR_DIM] = [0.11, 0.21, 0.31];

/// Clave de 32 bytes determinista para el test de base cifrada.
const ENCRYPTION_KEY: [u8; 32] = [7_u8; 32];

/// Abre una base temporal con pool amplio (sin backpressure).
///
/// Args:
///     path: Ruta del fichero de datos.
///
/// Returns:
///     La base abierta.
fn open_at(path: &std::path::Path) -> Database {
    Database::open(DbConfig::new(path, POOL_CAPACITY)).expect("apertura de la base")
}

/// Crea `items(id INT, name TEXT)`.
fn create_items(database: &mut Database) {
    database
        .create_table(
            "items",
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

/// Construye un `Record` con escalares y vector L2.
fn vector_record(scalars: ScalarMap, vector: &[f64; VECTOR_DIM]) -> Record {
    let floats: Vec<f32> = vector.iter().map(|value| *value as f32).collect();
    Record {
        id: RecordId::new(),
        scalars,
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(
            Embedding::new(
                floats,
                EmbeddingMeta {
                    model_id: "test".to_string(),
                    dim: VECTOR_DIM,
                    metric: Metric::L2,
                },
            )
            .expect("embedding válido"),
        ),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Siembra `items` con cuatro filas vectoriales deterministas.
fn seed_rows(database: &mut Database) {
    let data: [(i64, &str, [f64; VECTOR_DIM]); 4] = [
        (1, "origen", [0.0, 0.0, 0.0]),
        (2, "cerca", [0.1, 0.2, 0.3]),
        (3, "lejos", [9.0, 9.0, 9.0]),
        (4, "medio", [3.0, 3.0, 3.0]),
    ];
    for (id, name, vector) in data {
        let scalars = ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("name".to_string(), ScalarValue::Text(name.to_string())),
        ]);
        database
            .insert_record("items", vector_record(scalars, &vector))
            .expect("insert_record");
    }
}

/// Construye la consulta KNN de prueba sobre `items`.
fn knn_query(vector: &[f64; VECTOR_DIM]) -> String {
    format!(
        "SELECT * FROM items KNN embedding <|{KNN_K}|> [{}, {}, {}]",
        vector[0], vector[1], vector[2]
    )
}

/// Ruta del sidecar persistido del caché (`<data>.cache.json`).
fn cache_sidecar(data_path: &std::path::Path) -> std::path::PathBuf {
    data_path.with_extension("cache.json")
}

/// AC-0093-01 — el caché sobrevive a `close()`/reopen y sirve un hit restaurado.
#[test] // @spec AC-0093-01
fn test_ac_0093_01_cache_persists_across_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac0093_01.db");

    {
        let mut database = open_at(&path);
        create_items(&mut database);
        seed_rows(&mut database);
        // Calienta el caché en OFF (almacena la clave sin contar métricas).
        let _ = database
            .execute(&knn_query(&QUERY_VECTOR))
            .expect("KNN de calentamiento");
        assert!(
            database.semantic_cache_metrics().entries >= 1,
            "la consulta KNN debe almacenar al menos una entrada"
        );
        database.close().expect("close limpio");
    }

    assert!(
        cache_sidecar(&path).exists(),
        "close() debe persistir el sidecar del caché"
    );

    let mut reopened = open_at(&path);
    assert!(
        reopened.semantic_cache_metrics().entries >= 1,
        "reopen debe restaurar las entradas del caché"
    );
    reopened.set_semantic_cache(true);
    let _ = reopened
        .execute(&knn_query(&NEAR_QUERY_VECTOR))
        .expect("KNN con vector cercano");
    let metrics: CacheMetrics = reopened.semantic_cache_metrics();
    assert_eq!(
        metrics.hits, 1,
        "un vector cercano debe servirse desde la clave restaurada"
    );
}

/// AC-0093-02 — una mutación invalida el caché y retira el sidecar.
#[test] // @spec AC-0093-02
fn test_ac_0093_02_mutation_invalidates_persisted_cache() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac0093_02.db");
    let mut database = open_at(&path);
    create_items(&mut database);
    seed_rows(&mut database);
    let _ = database
        .execute(&knn_query(&QUERY_VECTOR))
        .expect("KNN de calentamiento");
    // Persiste explícitamente para tener un sidecar vivo que invalidar.
    database.persist_semantic_cache().expect("persist");
    assert!(
        cache_sidecar(&path).exists(),
        "el sidecar debe existir tras persistir"
    );

    database
        .insert(
            "items",
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(5)),
                ("name".to_string(), ScalarValue::Text("nuevo".to_string())),
            ]),
        )
        .expect("insert que muta la base");

    assert_eq!(
        database.semantic_cache_metrics().entries,
        0,
        "una mutación debe vaciar las entradas en memoria"
    );
    assert!(
        !cache_sidecar(&path).exists(),
        "una mutación debe retirar el sidecar para no resucitar filas obsoletas"
    );
}

/// AC-0093-03 — sidecar corrupto o con versión desconocida se ignora.
#[test] // @spec AC-0093-03
fn test_ac_0093_03_corrupt_or_unknown_version_is_ignored() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac0093_03.db");
    {
        let mut database = open_at(&path);
        create_items(&mut database);
        seed_rows(&mut database);
        let _ = database
            .execute(&knn_query(&QUERY_VECTOR))
            .expect("KNN de calentamiento");
        database.close().expect("close limpio");
    }
    let sidecar = cache_sidecar(&path);
    assert!(sidecar.exists(), "el close debe haber dejado un sidecar");

    // (a) Versión desconocida: se ignora sin romper la apertura.
    std::fs::write(&sidecar, r#"{"version": 99, "entries": []}"#).expect("escribir versión mala");
    let reopened = open_at(&path);
    assert_eq!(
        reopened.semantic_cache_metrics().entries,
        0,
        "una versión desconocida no debe cargar entradas"
    );
    drop(reopened);

    // (b) JSON corrupto: tampoco rompe la apertura.
    std::fs::write(&sidecar, "{no-es-json").expect("escribir basura");
    let reopened = open_at(&path);
    assert_eq!(
        reopened.semantic_cache_metrics().entries,
        0,
        "un sidecar corrupto no debe cargar entradas"
    );
}

/// AC-0093-04 — una base cifrada no persiste ni carga el sidecar en claro.
#[test] // @spec AC-0093-04
fn test_ac_0093_04_encrypted_database_never_persists_plaintext_cache() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac0093_04.db");
    {
        let mut config = DbConfig::new(&path, POOL_CAPACITY);
        config.encryption = Some(EncryptionConfig::new(ENCRYPTION_KEY));
        let mut database = Database::open(config).expect("apertura cifrada");
        create_items(&mut database);
        seed_rows(&mut database);
        let _ = database
            .execute(&knn_query(&QUERY_VECTOR))
            .expect("KNN de calentamiento");
        assert!(
            database.semantic_cache_metrics().entries >= 1,
            "el caché en memoria debe tener entradas"
        );
        database.close().expect("close limpio");
    }
    assert!(
        !cache_sidecar(&path).exists(),
        "una base cifrada no debe dejar el caché en claro en disco"
    );

    // Un sidecar plantado a mano tampoco debe cargarse en una base cifrada.
    std::fs::write(
        cache_sidecar(&path),
        r#"{"version": 1, "entries": [{"signature": "x", "vector": [1.0], "rows": []}]}"#,
    )
    .expect("plantar sidecar en claro");
    let mut config = DbConfig::new(&path, POOL_CAPACITY);
    config.encryption = Some(EncryptionConfig::new(ENCRYPTION_KEY));
    let reopened = Database::open(config).expect("reapertura cifrada");
    assert_eq!(
        reopened.semantic_cache_metrics().entries,
        0,
        "una base cifrada no debe confiar en un sidecar en claro"
    );
}
