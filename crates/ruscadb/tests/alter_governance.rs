//! `ALTER TABLE ADD COLUMN` y columnas de gobernanza (SPEC-0160).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Abre una base temporal.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    (dir, database)
}

/// La columna nueva es visible, las filas viejas leen NULL y es idempotente.
#[test]
fn alter_add_column_is_visible_and_idempotent() {
    let (_dir, mut database) = open_test_db("alter");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(1))]),
        )
        .expect("insert viejo");

    database
        .alter_table_add_column("t", "b", ColumnType::Text)
        .expect("alter");
    assert!(database.has_column("t", "b").expect("has_column"));
    assert!(!database.has_column("t", "zzz").expect("has_column"));

    // La escritura nueva aporta la columna (contrato estricto del motor).
    database
        .insert(
            "t",
            ScalarMap::from([
                ("a".to_string(), ScalarValue::Int(2)),
                ("b".to_string(), ScalarValue::Text("x".to_string())),
            ]),
        )
        .expect("insert nuevo");

    let rows = database.execute("SELECT a, b FROM t").expect("select");
    assert_eq!(rows.len(), 2);
    let nulls = rows
        .iter()
        .filter(|row| row.get("b") == Some(&ScalarValue::Null))
        .count();
    assert_eq!(nulls, 1, "la fila vieja lee NULL");
    assert!(
        rows.iter()
            .any(|row| row.get("b") == Some(&ScalarValue::Text("x".to_string())))
    );

    // Duplicada se rechaza con error accionable.
    assert!(
        database
            .alter_table_add_column("t", "b", ColumnType::Text)
            .is_err()
    );
}

/// Las columnas de gobernanza se anaden una vez y son consultables.
#[test]
fn governance_columns_are_added_once() {
    let (_dir, mut database) = open_test_db("governance");
    database
        .create_table(
            "mem",
            vec![ColumnDef {
                name: "texto".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create_table");

    let added = database.add_governance_columns("mem").expect("governance");
    assert_eq!(
        added,
        vec![
            "derived_from".to_string(),
            "session_id".to_string(),
            "write_gate_verdict".to_string(),
            "trust_tier".to_string()
        ]
    );
    for column in [
        "derived_from",
        "session_id",
        "write_gate_verdict",
        "trust_tier",
    ] {
        assert!(database.has_column("mem", column).expect("has_column"));
    }
    // Idempotente: la segunda pasada no anade nada.
    assert!(
        database
            .add_governance_columns("mem")
            .expect("segunda pasada")
            .is_empty()
    );

    // Se puede escribir con las columnas de gobernanza.
    database
        .insert(
            "mem",
            ScalarMap::from([
                ("texto".to_string(), ScalarValue::Text("hola".to_string())),
                (
                    "derived_from".to_string(),
                    ScalarValue::Text("doc-1".to_string()),
                ),
                (
                    "session_id".to_string(),
                    ScalarValue::Text("s-1".to_string()),
                ),
                (
                    "write_gate_verdict".to_string(),
                    ScalarValue::Text("accepted".to_string()),
                ),
                (
                    "trust_tier".to_string(),
                    ScalarValue::Text("verified".to_string()),
                ),
            ]),
        )
        .expect("insert gobernado");
    let rows = database
        .execute("SELECT session_id FROM mem")
        .expect("select");
    assert_eq!(rows.len(), 1);
}

/// Los indices de gobernanza se crean una vez y el planificador los usa.
#[test]
fn governance_indexes_are_created_and_used() {
    let (_dir, mut database) = open_test_db("gov_idx");
    database
        .create_table(
            "mem",
            vec![ColumnDef {
                name: "texto".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create_table");

    let (columns, indexes) = database.ensure_governance("mem").expect("ensure");
    assert_eq!(columns.len(), 4);
    assert_eq!(
        indexes,
        vec![
            "session_id".to_string(),
            "derived_from".to_string(),
            "trust_tier".to_string()
        ]
    );
    let (_, again) = database.ensure_governance("mem").expect("segunda");
    assert!(again.is_empty(), "idempotente: {again:?}");

    database
        .insert(
            "mem",
            ScalarMap::from([
                ("texto".to_string(), ScalarValue::Text("hola".to_string())),
                (
                    "session_id".to_string(),
                    ScalarValue::Text("s-1".to_string()),
                ),
                (
                    "derived_from".to_string(),
                    ScalarValue::Text("doc-1".to_string()),
                ),
                (
                    "write_gate_verdict".to_string(),
                    ScalarValue::Text("accepted".to_string()),
                ),
                (
                    "trust_tier".to_string(),
                    ScalarValue::Text("verified".to_string()),
                ),
            ]),
        )
        .expect("insert");

    let plan = database
        .explain("EXPLAIN SELECT texto FROM mem WHERE session_id = 's-1'")
        .expect("explain");
    assert!(
        plan.iter().any(|line| line.contains("IndexScan")),
        "el plan debe usar el indice: {plan:?}"
    );
    let rows = database
        .execute("SELECT texto FROM mem WHERE session_id = 's-1'")
        .expect("select");
    assert_eq!(rows.len(), 1);
}
