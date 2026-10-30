//! Tests de aceptacion de la feature `lance-format` (SPEC-0136).

#![cfg(feature = "lance-format")]
#![allow(clippy::expect_used)]

use ruscadb::LanceTable;

#[test] // @spec AC-0136-01
fn test_ac_0136_01_lance_append_scan() {
    let mut table = LanceTable::new("test");
    table.append();
    table.append();
    assert_eq!(table.len(), 2);
    assert_eq!(
        LanceTable::from_bytes(&table.to_bytes()).expect("roundtrip"),
        table
    );

    // Columnas tipadas con round-trip exacto por el contenedor `RLNC1`.
    let mut typed = LanceTable::new("typed");
    typed.push_int("id", 7).expect("int");
    typed.push_float("score", 0.5).expect("float");
    let restored = LanceTable::from_bytes(&typed.to_bytes()).expect("from_bytes");
    assert_eq!(restored, typed);
    assert_eq!(restored.len(), 1);
}
