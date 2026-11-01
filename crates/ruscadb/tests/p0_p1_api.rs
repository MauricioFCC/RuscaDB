//! Peticiones P0/P1 del consumidor (SPEC-0182).

#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, ColumnValue, ColumnarTable, Database, DbConfig, EmbeddingMeta, IsoLevel,
    Metric, Row, ScalarMap, ScalarValue,
};

/// Abre una base temporal con la tabla `trades`.
fn open_trades(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    database
        .create_table(
            "trades",
            vec![
                ColumnDef {
                    name: "regime".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "r_multiple".to_string(),
                    col_type: ColumnType::Float,
                },
                ColumnDef {
                    name: "closed".to_string(),
                    col_type: ColumnType::Bool,
                },
            ],
        )
        .expect("create_table");
    for (regime, r_multiple, closed) in [("a", 1.0, true), ("a", -1.0, false), ("b", 2.0, true)] {
        database
            .insert(
                "trades",
                ScalarMap::from([
                    ("regime".to_string(), ScalarValue::Text(regime.to_string())),
                    ("r_multiple".to_string(), ScalarValue::Float(r_multiple)),
                    ("closed".to_string(), ScalarValue::Bool(closed)),
                ]),
            )
            .expect("insert");
    }
    (dir, database)
}

/// Valor entero de una fila.
fn int_of(row: &Row, column: &str) -> i64 {
    match row.get(column) {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en {column}: {other:?}"),
    }
}

/// AC-0182-01: `SUM/AVG/MIN/MAX` sobre `CASE`.
#[test] // @spec AC-0182-01
fn test_ac_0182_01_case_in_aggregate() {
    let (_dir, mut database) = open_trades("case_aggregate");
    let rows = database
        .execute(
            "SELECT regime, COUNT(*), \
             SUM(CASE WHEN r_multiple > 0 THEN 1 ELSE 0 END) AS wins, \
             AVG(CASE WHEN r_multiple > 0 THEN r_multiple ELSE 0.0 END) AS avg_win \
             FROM trades GROUP BY regime ORDER BY regime",
        )
        .expect("agregados con CASE");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get("regime"),
        Some(&ScalarValue::Text("a".to_string()))
    );
    assert_eq!(int_of(&rows[0], "wins"), 1);
    assert_eq!(int_of(&rows[0], "count"), 2);
    assert_eq!(
        rows[0].get("avg_win"),
        Some(&ScalarValue::Float(0.5)),
        "AVG(CASE) de a"
    );
    assert_eq!(int_of(&rows[1], "wins"), 1);

    // MIN/MAX sobre CASE tambien resuelven el tipo.
    let extremes = database
        .execute(
            "SELECT MIN(CASE WHEN r_multiple > 0 THEN r_multiple ELSE 9.0 END) AS low \
             FROM trades",
        )
        .expect("min case");
    assert_eq!(extremes[0].get("low"), Some(&ScalarValue::Float(1.0)));
}

/// AC-0182-02: literales booleanos, columna desnuda e `IS NOT TRUE`.
#[test] // @spec AC-0182-02
fn test_ac_0182_02_boolean_literals() {
    let (_dir, mut database) = open_trades("bool_where");
    for query in [
        "SELECT * FROM trades WHERE closed = TRUE",
        "SELECT * FROM trades WHERE closed",
    ] {
        let rows = database.execute(query).expect(query);
        assert_eq!(rows.len(), 2, "{query}");
    }
    let false_rows = database
        .execute("SELECT * FROM trades WHERE closed = FALSE")
        .expect("false");
    assert_eq!(false_rows.len(), 1);
    let not_true = database
        .execute("SELECT * FROM trades WHERE closed IS NOT TRUE")
        .expect("is not true");
    assert_eq!(not_true.len(), 1);
    assert_eq!(
        not_true[0].get("r_multiple"),
        Some(&ScalarValue::Float(-1.0))
    );
}

/// AC-0182-03: `write_rcol1`/`read_rcol1` (ALP + round-trip exacto).
#[test] // @spec AC-0182-03
fn test_ac_0182_03_rcol1_roundtrip() {
    let mut table = ColumnarTable::new(vec![
        "ts".to_string(),
        "price".to_string(),
        "label".to_string(),
    ]);
    for index in 0..50i64 {
        table
            .append(vec![
                ColumnValue::Int(index),
                ColumnValue::Float(index as f64 * 0.25),
                ColumnValue::Text(format!("row-{index}")),
            ])
            .expect("append");
    }
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("fixture.rcol");
    let written = table.write_rcol1(&path).expect("write_rcol1");
    assert!(written > 0);
    let restored = ColumnarTable::read_rcol1(&path).expect("read_rcol1");
    assert_eq!(restored.columns(), table.columns());
    assert_eq!(restored.scan(), table.scan());
}

/// AC-0182-04: `with_isolation(IsoLevel::Serializable)` activa SSI.
#[test] // @spec AC-0182-04
fn test_ac_0182_04_isolation_api() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("iso.db");
    let database = Database::open(DbConfig::new(&path, 64))
        .expect("open")
        .with_isolation(IsoLevel::Serializable);
    assert_eq!(database.isolation(), IsoLevel::Serializable);
    assert!(database.is_ssi_enabled());

    let snapshot_dir = tempfile::tempdir().expect("dir snapshot");
    let snapshot_path = snapshot_dir.path().join("snapshot.db");
    let snapshot = Database::open(DbConfig::new(&snapshot_path, 64))
        .expect("open snapshot")
        .with_isolation(IsoLevel::Snapshot);
    assert_eq!(snapshot.isolation(), IsoLevel::Snapshot);
    assert!(!snapshot.is_ssi_enabled());
}

/// AC-0182-05: `insert_with_embedding` asocia un vector a una fila relacional
/// y `KNN` lo encuentra.
#[test] // @spec AC-0182-05
fn test_ac_0182_05_insert_with_vector_knn() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("vectors.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "docs",
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

    for (id, name, vector) in [
        (1i64, "origen", vec![0.0f32, 0.0]),
        (2, "cerca", vec![0.1, 0.0]),
        (3, "lejos", vec![9.0, 9.0]),
    ] {
        database
            .insert_with_embedding(
                "docs",
                ScalarMap::from([
                    ("id".to_string(), ScalarValue::Int(id)),
                    ("name".to_string(), ScalarValue::Text(name.to_string())),
                ]),
                vector,
                EmbeddingMeta {
                    model_id: "test".to_string(),
                    dim: 2,
                    metric: Metric::L2,
                },
            )
            .expect("insert_with_embedding");
    }

    let rows = database
        .execute("SELECT * FROM docs KNN embedding <|1|> [0.0, 0.0]")
        .expect("KNN");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("name"),
        Some(&ScalarValue::Text("origen".to_string()))
    );

    // La variante con `Embedding` ya construido tambien es publica.
    let embedding = ruscadb::Embedding::new(
        vec![0.05f32, 0.0],
        EmbeddingMeta {
            model_id: "test".to_string(),
            dim: 2,
            metric: Metric::L2,
        },
    )
    .expect("embedding");
    database
        .insert_with_vector(
            "docs",
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(4)),
                (
                    "name".to_string(),
                    ScalarValue::Text("muy cerca".to_string()),
                ),
            ]),
            embedding,
        )
        .expect("insert_with_vector");
    let rows = database
        .execute("SELECT * FROM docs KNN embedding <|1|> [0.04, 0.0]")
        .expect("KNN 2");
    assert_eq!(
        rows[0].get("name"),
        Some(&ScalarValue::Text("muy cerca".to_string()))
    );
}
