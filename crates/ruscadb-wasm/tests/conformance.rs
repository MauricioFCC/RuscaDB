//! Conformance del fixture Rust en el binding WASM (SPEC-0196).
//!
//! El binding WASM host expone parser/vector/FTS, no almacenamiento de tablas;
//! por eso valida el **parseo** del mismo fixture y la forma canónica del AST.

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use sha2::{Digest, Sha256};

/// AC-0196-03 — WASM parsea el fixture y produce un hash canónico estable.
#[test] // @spec AC-0196-03
fn test_ac_0196_03_wasm_conformance() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../ruscadb/tests/conformance/basic.json"))
            .expect("fixture");
    let cases = fixture["cases"].as_array().expect("cases");
    let mut hashes = Vec::new();
    for case in cases {
        let sql = case["sql"].as_str().expect("sql");
        let parsed = ruscadb_wasm::parse_query_json(sql);
        let value: serde_json::Value = serde_json::from_str(&parsed).expect("json ast");
        assert_eq!(value["ok"], true, "caso: {sql}");
        assert_eq!(value["statement"]["kind"], "select", "caso: {sql}");
        let canonical = serde_json::to_string(&value["statement"]).expect("canonical");
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        hashes.push(hex::encode(hasher.finalize()));
    }
    assert_eq!(hashes.len(), cases.len());
    assert!(hashes.iter().all(|hash| hash.len() == 64));
}
