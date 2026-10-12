//! Tests de aceptación de robustez miscelánea (SPEC-0071).
//!
//! RED **por aserción**: el registry puede quedar **parcial** si el JSON
//! persistido es inválido a mitad de lista (hallazgo de la retrospectiva 3).
//!
//! El AC-0071-01 (escala de scores híbridos con `k > profundidad`) vive como
//! unit test interno en `src/hybrid.rs`: requiere listas artificiales para
//! disparar el caso de forma determinista (ver la spec).

#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool en los tests.
const POOL_CAPACITY: usize = 128;

/// Dimensión de los vectores de prueba.
const VECTOR_DIM: usize = 3;

/// Construye un embedding L2.
///
/// Args:
///     values: Componentes.
///
/// Returns:
///     El embedding construido.
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

/// Construye un registro con escalares y vector.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     titulo: Texto del documento.
///     vector: Componentes del embedding.
///
/// Returns:
///     El registro listo para `insert_record`.
fn record(id: i64, titulo: &str, vector: &[f64]) -> Record {
    Record {
        id: RecordId::new(),
        scalars: ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("titulo".to_string(), ScalarValue::Text(titulo.to_string())),
        ]),
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(embedding(vector)),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// AC-0071-02 — registry atómico y sin estado parcial.
///
/// Un `models.json` con una entrada inválida a mitad NO debe dejar registrada
/// la entrada válida previa (hoy `restore_registry` registra hasta el fallo).
#[test]
// @spec AC-0071-02
fn test_ac_0071_02_registry_atomic_and_no_partial() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac0071_02.db");
    let models_path = path.with_extension("models.json");

    // (a) Un registro limpio no deja temporales huérfanos.
    {
        let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
        database
            .register_model("m1", VECTOR_DIM, Metric::L2, 1)
            .expect("register_model");
        database.close().expect("cierre");
    }
    let leftovers: Vec<String> = std::fs::read_dir(dir.path())
        .expect("lee dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no deben quedar ficheros temporales: {leftovers:?}"
    );

    // (b) JSON con la segunda entrada inválida: la apertura falla.
    let partial = r#"[{"model_id":"m1","dim":3,"metric":"L2","version":1},{"model_id":"m2"}]"#;
    std::fs::write(&models_path, partial).expect("escribe registry parcial");
    let reopened = Database::open(DbConfig::new(&path, POOL_CAPACITY));
    assert!(
        reopened.is_err(),
        "un registry parcial debe fallar la apertura, no registrarse a medias"
    );

    // (c) Sin registry persistido, no debe quedar la entrada parcial m1.
    std::fs::remove_file(&models_path).expect("borra registry parcial");
    let mut clean = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("reapertura");
    clean
        .create_table(
            "docs",
            vec![ColumnDef {
                name: "titulo".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create_table");
    clean.set_ingestion_policy(ruscadb::IngestionPolicy {
        max_record_bytes: 0,
        require_registry: true,
    });
    let with_vector = record(1, "x", &[0.0, 0.0, 0.0]);
    let error = clean
        .insert_record("docs", with_vector)
        .expect_err("el registry debe estar vacío (sin entrada parcial de m1)");
    assert!(
        !error.to_string().is_empty(),
        "el rechazo debe ser accionable: {error}"
    );
}
