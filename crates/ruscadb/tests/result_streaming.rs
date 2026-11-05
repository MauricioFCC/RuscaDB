//! SPEC-0210 — streaming de resultados con memoria O(1) en filas.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, RuscaError, ScalarMap, ScalarValue};

fn open(dir: &std::path::Path) -> Database {
    let path = dir.join("streaming.db");
    let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
    db.create_table(
        "t",
        vec![
            ColumnDef {
                name: "k".to_string(),
                col_type: ColumnType::Int,
            },
            ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            },
        ],
    )
    .expect("create t");
    db.create_table(
        "u",
        vec![ColumnDef {
            name: "k".to_string(),
            col_type: ColumnType::Int,
        }],
    )
    .expect("create u");
    for k in 0..30 {
        db.insert(
            "t",
            ScalarMap::from([
                ("k".to_string(), ScalarValue::Int(k)),
                ("name".to_string(), ScalarValue::Text(format!("n{k}"))),
            ]),
        )
        .expect("insert t");
    }
    db
}

/// AC-0210-01 — el streaming devuelve las mismas filas que `execute`.
#[test] // @spec AC-0210-01
fn test_ac_0210_01_stream_matches_execute() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    for query in [
        "SELECT * FROM t WHERE k >= 10 LIMIT 5 OFFSET 3",
        "SELECT name FROM t WHERE k < 4",
        "SELECT k FROM t LIMIT 0",
    ] {
        let expected = db.execute(query).expect("execute");
        let mut streamed = Vec::new();
        let count = db
            .execute_stream(query, |row| {
                streamed.push(row.clone());
                Ok(true)
            })
            .expect("stream");
        assert_eq!(count, expected.len(), "{query}");
        assert_eq!(streamed, expected, "{query}");
    }
}

/// AC-0210-02 — el callback puede cortar el recorrido (parada temprana).
#[test] // @spec AC-0210-02
fn test_ac_0210_02_stream_early_stop() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    let mut seen = Vec::new();
    let count = db
        .execute_stream("SELECT * FROM t", |row| {
            seen.push(row.get("k").cloned());
            Ok(seen.len() < 3)
        })
        .expect("stream");
    assert_eq!(count, 3);
    assert_eq!(seen.len(), 3);
}

/// AC-0210-03 — las formas materializantes se rechazan con `execute()`.
#[test] // @spec AC-0210-03
fn test_ac_0210_03_stream_rejects_materializing() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());
    for query in [
        "SELECT * FROM t ORDER BY k",
        "SELECT DISTINCT k FROM t",
        "SELECT COUNT(*) FROM t",
        "SELECT * FROM t JOIN u ON t.k = u.k",
        "SELECT * FROM t WHERE MATCH(name, 'gato')",
    ] {
        match db.execute_stream(query, |_row| Ok(true)) {
            Err(RuscaError::TypeMismatch { message }) => {
                assert!(message.contains("execute()"), "{query}: {message}");
            }
            other => panic!("{query} debía rechazarse: {other:?}"),
        }
    }
    // DML no es un SELECT: error accionable, no un panic ni materialización.
    match db.execute_stream("DELETE FROM t", |_row| Ok(true)) {
        Err(RuscaError::TypeMismatch { message }) => {
            assert!(message.contains("solo admite SELECT"), "{message}");
        }
        other => panic!("DELETE debía rechazarse: {other:?}"),
    }
    // Las mismas consultas siguen disponibles por el camino materializado.
    assert_eq!(
        db.execute("SELECT COUNT(*) FROM t").expect("count").len(),
        1
    );
}

/// AC-0210-04 — robustez: errores del callback, tope de filas y snapshot fresco.
#[test] // @spec AC-0210-04
fn test_ac_0210_04_stream_robustness() {
    let dir = tempfile::tempdir().expect("dir");
    let mut db = open(dir.path());

    // Un error del callback se propaga y corta el recorrido.
    let failure = db.execute_stream("SELECT * FROM t", |_row| {
        Err(RuscaError::TypeMismatch {
            message: "stop desde el cliente".to_string(),
        })
    });
    match failure {
        Err(RuscaError::TypeMismatch { message }) => {
            assert_eq!(message, "stop desde el cliente");
        }
        other => panic!("el error del callback debía propagarse: {other:?}"),
    }

    // Una tabla vacía no invoca el callback.
    assert_eq!(
        db.execute_stream("SELECT * FROM u", |_row| Ok(true))
            .expect("tabla vacía"),
        0
    );

    // Cada llamada toma un snapshot nuevo: ve las filas insertadas después.
    let before = db
        .execute_stream("SELECT * FROM t", |_row| Ok(true))
        .expect("antes");
    db.insert(
        "t",
        ScalarMap::from([
            ("k".to_string(), ScalarValue::Int(99)),
            ("name".to_string(), ScalarValue::Text("n99".to_string())),
        ]),
    )
    .expect("insert");
    let after = db
        .execute_stream("SELECT * FROM t", |_row| Ok(true))
        .expect("después");
    assert_eq!(after, before + 1);

    // El tope `max_result_rows` también aplica al streaming.
    let path = dir.path().join("capped.db");
    let mut config = DbConfig::new(&path, 64);
    config.max_result_rows = Some(2);
    let mut capped = Database::open(config).expect("open capped");
    capped
        .create_table(
            "t",
            vec![ColumnDef {
                name: "k".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create capped");
    for k in 0..5 {
        capped
            .insert(
                "t",
                ScalarMap::from([("k".to_string(), ScalarValue::Int(k))]),
            )
            .expect("insert capped");
    }
    let mut seen = 0usize;
    let count = capped
        .execute_stream("SELECT * FROM t", |_row| {
            seen += 1;
            Ok(true)
        })
        .expect("stream capped");
    assert_eq!(count, 2);
    assert_eq!(seen, 2);
}
