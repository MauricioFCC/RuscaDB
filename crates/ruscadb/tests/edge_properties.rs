//! Aceptación de aristas con payload y `ts_unix` (SPEC-0193).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, Edge, EdgeSet, Record, RecordId, RecordMeta,
    ScalarMap, ScalarValue,
};
use serde_json::Value;

/// Abre una base temporal con `person(name TEXT)` y dos registros.
fn seed(tag: &str) -> (tempfile::TempDir, Database, RecordId, RecordId) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "person",
            vec![ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create");
    let alice = database
        .insert(
            "person",
            ScalarMap::from([("name".to_string(), ScalarValue::Text("alice".to_string()))]),
        )
        .expect("alice");
    let bob = database
        .insert(
            "person",
            ScalarMap::from([("name".to_string(), ScalarValue::Text("bob".to_string()))]),
        )
        .expect("bob");
    (dir, database, alice, bob)
}

/// Lee el JSON de la proyección de metadatos de la arista alice->bob.
fn edge_json(database: &mut Database) -> Value {
    let rows = database
        .execute("SELECT ->knows{payload, ts_unix}->person AS e FROM person WHERE name = 'alice'")
        .expect("edge projection");
    assert_eq!(rows.len(), 1);
    let Some(ScalarValue::Text(text)) = rows[0].get("e") else {
        panic!("se esperaba un Text JSON, se obtuvo {:?}", rows[0].get("e"));
    };
    serde_json::from_str(text).expect("JSON de arista")
}

/// AC-0193-01 — RELATE con payload es idempotente y preserva `ts_unix`.
#[test] // @spec AC-0193-01
fn test_ac_0193_01_relate_payload_idempotent() {
    let (_dir, mut database, alice, bob) = seed("edge_idem");
    let query = format!("RELATE person:{alice}->knows->person:{bob} SET {{\"since\":2020}}");
    let first = database.execute(&query).expect("crear");
    assert_eq!(first[0].get("affected"), Some(&ScalarValue::Int(1)));
    let meta = edge_json(&mut database);
    assert_eq!(meta["payload"]["since"], Value::from(2020));
    let ts = meta["ts_unix"].as_i64().expect("ts_unix");
    assert!(ts > 0);

    let second = database.execute(&query).expect("idempotente");
    assert_eq!(second[0].get("affected"), Some(&ScalarValue::Int(0)));
    let meta2 = edge_json(&mut database);
    assert_eq!(meta2["ts_unix"].as_i64(), Some(ts));
    assert_eq!(meta2["payload"]["since"], Value::from(2020));
}

/// AC-0193-02 — conflicto, DO NOTHING y DO UPDATE de payload.
#[test] // @spec AC-0193-02
fn test_ac_0193_02_relate_conflict_and_update() {
    let (_dir, mut database, alice, bob) = seed("edge_conflict");
    database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} SET {{\"since\":2020}} AT 1700000000"
        ))
        .expect("crear");
    let error = database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} SET {{\"since\":2021}}"
        ))
        .expect_err("conflicto");
    assert!(
        format!("{error}").contains("EDGE_CONFLICT"),
        "se esperaba EDGE_CONFLICT: {error}"
    );

    let noop = database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} SET {{\"since\":2021}} ON CONFLICT DO NOTHING"
        ))
        .expect("do nothing");
    assert_eq!(noop[0].get("affected"), Some(&ScalarValue::Int(0)));

    let updated = database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} ON CONFLICT DO UPDATE SET payload = {{\"since\":2022}}"
        ))
        .expect("do update");
    assert_eq!(updated[0].get("affected"), Some(&ScalarValue::Int(1)));
    let meta = edge_json(&mut database);
    assert_eq!(meta["payload"]["since"], Value::from(2022));
    assert_eq!(meta["ts_unix"].as_i64(), Some(1_700_000_000));

    let immutable = database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} AT 1700000001 ON CONFLICT DO UPDATE SET payload = {{\"since\":2023}}"
        ))
        .expect_err("ts inmutable");
    assert!(format!("{immutable}").contains("EDGE_CONFLICT"));
}

/// AC-0193-03 — validación del payload (objeto, reservadas, 64 KiB).
#[test] // @spec AC-0193-03
fn test_ac_0193_03_payload_validation() {
    let (_dir, mut database, alice, bob) = seed("edge_payload");
    let reserved = database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} SET {{\"from\":1}}"
        ))
        .expect_err("clave reservada");
    assert!(format!("{reserved}").contains("reservada"));

    let huge = format!("{{\"blob\":\"{}\"}}", "x".repeat(70 * 1024));
    let too_big = database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} SET {huge}"
        ))
        .expect_err("payload grande");
    assert!(format!("{too_big}").contains("excede"));
}

/// AC-0193-04 — proyección `->label{payload, ts_unix}->tabla`.
#[test] // @spec AC-0193-04
fn test_ac_0193_04_edge_read_projection() {
    let (_dir, mut database, alice, bob) = seed("edge_read");
    database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} SET {{\"weight\":7}} AT 1700000000"
        ))
        .expect("crear");
    let meta = edge_json(&mut database);
    assert_eq!(meta["payload"]["weight"], Value::from(7));
    assert_eq!(meta["ts_unix"], Value::from(1_700_000_000));

    // Arista legacy: `Record.edges` sin fila de metadatos.
    let mut legacy = Record {
        id: RecordId::new(),
        scalars: ScalarMap::from([("name".to_string(), ScalarValue::Text("legacy".to_string()))]),
        doc: None,
        edges: EdgeSet {
            out: vec![Edge {
                label: "knows".to_string(),
                node: bob,
            }],
            incoming: vec![],
        },
        vector: None,
        blob: None,
        meta: RecordMeta::default(),
    };
    legacy.meta.created_tx = database.last_tx_id();
    database.insert_record("person", legacy).expect("legacy");
    let rows = database
        .execute("SELECT ->knows{payload, ts_unix}->person AS e FROM person WHERE name = 'legacy'")
        .expect("legacy path");
    let Some(ScalarValue::Text(text)) = rows[0].get("e") else {
        panic!("se esperaba JSON legacy");
    };
    let value: Value = serde_json::from_str(text).expect("json");
    assert_eq!(value["payload"], Value::Null);
    assert_eq!(value["ts_unix"], Value::Null);
}

/// AC-0193-05 — persistencia y recovery del EdgeMetaStore.
#[test] // @spec AC-0193-05
fn test_ac_0193_05_persistence_recovery() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("edge_recovery.db");
    {
        let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
        database
            .create_table(
                "person",
                vec![ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                }],
            )
            .expect("create");
        let alice = database
            .insert(
                "person",
                ScalarMap::from([("name".to_string(), ScalarValue::Text("alice".to_string()))]),
            )
            .expect("alice");
        let bob = database
            .insert(
                "person",
                ScalarMap::from([("name".to_string(), ScalarValue::Text("bob".to_string()))]),
            )
            .expect("bob");
        database
            .execute(&format!(
                "RELATE person:{alice}->knows->person:{bob} SET {{\"since\":2020}} AT 1700000000"
            ))
            .expect("relate");
    }
    let mut reopened = Database::open(DbConfig::new(&path, 64)).expect("reopen");
    let rows = reopened
        .execute("SELECT ->knows{payload, ts_unix}->person AS e FROM person WHERE name = 'alice'")
        .expect("path tras reopen");
    assert_eq!(rows.len(), 1);
    let Some(ScalarValue::Text(text)) = rows[0].get("e") else {
        panic!("se esperaba JSON tras reopen");
    };
    let value: Value = serde_json::from_str(text).expect("json");
    assert_eq!(value["payload"]["since"], Value::from(2020));
    assert_eq!(value["ts_unix"], Value::from(1_700_000_000));
}

/// AC-0193-06 — rollback de transacción no deja metadatos.
#[test] // @spec AC-0193-06
fn test_ac_0193_06_txn_rollback() {
    let (_dir, mut database, alice, bob) = seed("edge_rollback");
    database.begin();
    database
        .execute(&format!(
            "RELATE person:{alice}->knows->person:{bob} SET {{\"since\":2020}}"
        ))
        .expect("relate en tx");
    database.rollback().expect("rollback");
    let rows = database
        .execute("SELECT ->knows{payload, ts_unix}->person AS e FROM person WHERE name = 'alice'")
        .expect("path tras rollback");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("e"), Some(&ScalarValue::Null));
}

/// AC-0193-07 — el formato postcard de `Edge` no cambia.
#[test] // @spec AC-0193-07
fn test_ac_0193_07_edge_format_unchanged() {
    let edge = Edge {
        label: "knows".to_string(),
        node: RecordId::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("id"),
    };
    let bytes = postcard::to_allocvec(&edge).expect("postcard");
    let expected =
        hex::decode("056b6e6f77731a303141525a334e44454b54535634525246465136394735464156")
            .expect("hex");
    assert_eq!(
        bytes,
        expected,
        "el postcard de Edge cambió: {}",
        hex::encode(&bytes)
    );
}
