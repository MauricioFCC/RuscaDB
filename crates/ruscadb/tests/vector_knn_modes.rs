//! Modos de resolucion `KNN ... USING` cableados a RQL (SPEC-0171/0172).

#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, Row, ScalarMap, ScalarValue,
};

/// Abre una base temporal.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    (dir, database)
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

/// Inserta una fila con embedding L2.
fn insert_vector(database: &mut Database, id: i64, name: &str, vector: &[f32]) {
    let scalars = ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ]);
    let record = Record {
        id: RecordId::new(),
        scalars,
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(
            Embedding::new(
                vector.to_vec(),
                EmbeddingMeta {
                    model_id: "test".to_string(),
                    dim: vector.len(),
                    metric: Metric::L2,
                },
            )
            .expect("embedding"),
        ),
        blob: None,
        meta: RecordMeta::default(),
    };
    database.insert_record("items", record).expect("insert");
}

/// Siembra cuatro documentos con distancias conocidas al vector `[0, 0, 0]`.
fn seed(database: &mut Database) {
    create_items(database);
    insert_vector(database, 1, "origen", &[0.0, 0.0, 0.0]);
    insert_vector(database, 2, "cerca", &[0.1, 0.1, 0.1]);
    insert_vector(database, 3, "lejos", &[9.0, 9.0, 9.0]);
    insert_vector(database, 4, "medio", &[3.0, 3.0, 3.0]);
}

/// Nombres devueltos por el KNN, en orden.
fn names(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row.get("name") {
            Some(ScalarValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// `USING EXACT` ejecuta fuerza bruta y devuelve el vecino mas cercano primero.
#[test] // @spec AC-0181-01
fn test_ac_0181_01_exact_nearest_first() {
    let (_dir, mut database) = open_test_db("knn_exact");
    seed(&mut database);
    let rows = database
        .execute("SELECT * FROM items KNN embedding <|2|> [0.0, 0.0, 0.0] USING EXACT")
        .expect("EXACT");
    assert_eq!(
        names(&rows),
        vec!["origen".to_string(), "cerca".to_string()]
    );
}

/// `USING QUANTIZED` usa el indice cuantizado y conserva el mejor vecino.
#[test] // @spec AC-0181-02
fn test_ac_0181_02_quantized_finds_nearest() {
    let (_dir, mut database) = open_test_db("knn_quantized");
    seed(&mut database);
    let rows = database
        .execute("SELECT * FROM items KNN embedding <|2|> [0.0, 0.0, 0.0] USING QUANTIZED")
        .expect("QUANTIZED");
    assert!(rows.len() <= 2);
    assert!(
        names(&rows).contains(&"origen".to_string()),
        "nombres: {:?}",
        names(&rows)
    );
}

/// `USING FILTERED` aplica ACORN solo sobre las filas que pasan el `WHERE`.
#[test] // @spec AC-0181-03
fn test_ac_0181_03_filtered_respects_where() {
    let (_dir, mut database) = open_test_db("knn_filtered");
    seed(&mut database);
    let rows = database
        .execute(
            "SELECT * FROM items WHERE name = 'lejos' \
             KNN embedding <|2|> [0.0, 0.0, 0.0] USING FILTERED",
        )
        .expect("FILTERED");
    assert_eq!(names(&rows), vec!["lejos".to_string()]);
}

/// Sin `USING` se conserva el comportamiento previo (AUTO).
#[test] // @spec AC-0181-04
fn test_ac_0181_04_default_mode_is_auto() {
    let (_dir, mut database) = open_test_db("knn_auto");
    seed(&mut database);
    let rows = database
        .execute("SELECT * FROM items KNN embedding <|2|> [0.0, 0.0, 0.0]")
        .expect("AUTO");
    assert_eq!(rows.len(), 2);
}

/// Un modo desconocido es error de sintaxis accionable.
#[test] // @spec AC-0181-07
fn test_ac_0181_07_unknown_mode_is_parse_error() {
    let (_dir, mut database) = open_test_db("knn_bad_mode");
    seed(&mut database);
    let error = database
        .execute("SELECT * FROM items KNN embedding <|2|> [0.0, 0.0, 0.0] USING turbo")
        .expect_err("modo invalido");
    let text = error.to_string();
    assert!(text.contains("QUANTIZED"), "{text}");
}
