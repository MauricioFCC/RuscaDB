//! Aceptación de SPEC-0187 (CRDT bytes, last_tx_id, features, contrato).

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, CrdtSyncReal, Database, DbConfig, ScalarMap, ScalarValue};

/// AC-0187-01: round-trip binario del CRDT y merge tras deserializar.
#[test] // @spec AC-0187-01
fn test_ac_0187_01_crdt_bytes() {
    let mut replica = CrdtSyncReal::with_actor("a");
    replica.set(41);
    let bytes = replica.to_bytes();
    let mut restored = CrdtSyncReal::from_bytes(&bytes).expect("from_bytes");
    assert_eq!(restored.value(), Some(41));
    assert_eq!(restored.vector_clock(), replica.vector_clock());

    let mut other = CrdtSyncReal::with_actor("b");
    other.set(42);
    restored.merge(&other);
    assert_eq!(restored.value(), Some(42));

    assert!(CrdtSyncReal::from_bytes(b"garbage").is_err());
    assert!(CrdtSyncReal::from_bytes(&bytes[..bytes.len() - 1]).is_err());
}

/// AC-0187-02: `last_tx_id` crece y `snapshot_at` ve la version historica.
#[test] // @spec AC-0187-02
fn test_ac_0187_02_last_tx_id() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("tx.db");
    let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "x".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create");
    database
        .insert(
            "t",
            ScalarMap::from([("x".to_string(), ScalarValue::Int(1))]),
        )
        .expect("insert 1");
    let first = database.last_tx_id();
    database
        .insert(
            "t",
            ScalarMap::from([("x".to_string(), ScalarValue::Int(2))]),
        )
        .expect("insert 2");
    let second = database.last_tx_id();
    assert!(second > first);
    let historical = database
        .execute_at_lsn("SELECT * FROM t", first)
        .expect("snapshot historico");
    assert_eq!(historical.len(), 1);
}

/// AC-0187-03: el manifiesto de features coincide con Cargo.toml.
#[test] // @spec AC-0187-03
fn test_ac_0187_03_features_manifest() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("Cargo.toml");
    assert!(!ruscadb::FEATURES.is_empty());
    assert!(ruscadb::FEATURES.contains(&"kms"));
    assert!(ruscadb::FEATURES.contains(&"ssi"));
    for feature in ruscadb::FEATURES {
        assert!(
            manifest.contains(&format!("{feature} =")),
            "feature '{feature}' no aparece en Cargo.toml"
        );
    }
}

/// AC-0187-04: fingerprint determinista y hexadecimal.
#[test] // @spec AC-0187-04
fn test_ac_0187_04_contract_fingerprint() {
    let first = ruscadb::contract::fingerprint();
    let second = ruscadb::contract::fingerprint();
    assert_eq!(first, second);
    assert_ne!(first, 0);
    let hex = ruscadb::contract::fingerprint_hex();
    assert_eq!(hex.len(), 16);
    assert!(hex.chars().all(|character| character.is_ascii_hexdigit()));
}
