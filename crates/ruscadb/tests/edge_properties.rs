//! Scaffold RED de SPEC-0193 (aristas con payload y `ts_unix`).
//!
//! Los tests están `#[ignore]` a propósito: la spec está `proposed` y la
//! implementación aún no existe. `cargo xtask trace` reconoce los nombres y
//! `cargo xtask audit` no los cuenta como huecos. Al implementar la spec se
//! quita el `#[ignore]` y se reemplaza el `panic!` por la aserción real.

#![allow(missing_docs)]
#![allow(clippy::panic)]

/// AC-0193-01 — RELATE con payload es idempotente y preserva `ts_unix`.
#[test]
#[ignore = "SPEC-0193 propuesta: pendiente de implementación"]
// @spec AC-0193-01
fn test_ac_0193_01_relate_payload_idempotent() {
    panic!("SPEC-0193 pendiente: RELATE SET + idempotencia");
}

/// AC-0193-02 — conflicto, DO NOTHING y DO UPDATE de payload.
#[test]
#[ignore = "SPEC-0193 propuesta: pendiente de implementación"]
// @spec AC-0193-02
fn test_ac_0193_02_relate_conflict_and_update() {
    panic!("SPEC-0193 pendiente: ON CONFLICT");
}

/// AC-0193-03 — validación del payload (objeto, reservadas, 64 KiB).
#[test]
#[ignore = "SPEC-0193 propuesta: pendiente de implementación"]
// @spec AC-0193-03
fn test_ac_0193_03_payload_validation() {
    panic!("SPEC-0193 pendiente: validación de payload");
}

/// AC-0193-04 — proyección `->label{payload, ts_unix}->tabla`.
#[test]
#[ignore = "SPEC-0193 propuesta: pendiente de implementación"]
// @spec AC-0193-04
fn test_ac_0193_04_edge_read_projection() {
    panic!("SPEC-0193 pendiente: lectura de metadatos");
}

/// AC-0193-05 — persistencia y recovery del EdgeMetaStore.
#[test]
#[ignore = "SPEC-0193 propuesta: pendiente de implementación"]
// @spec AC-0193-05
fn test_ac_0193_05_persistence_recovery() {
    panic!("SPEC-0193 pendiente: REDG1 + WAL");
}

/// AC-0193-06 — rollback de transacción no deja metadatos.
#[test]
#[ignore = "SPEC-0193 propuesta: pendiente de implementación"]
// @spec AC-0193-06
fn test_ac_0193_06_txn_rollback() {
    panic!("SPEC-0193 pendiente: atomicidad MVCC");
}

/// AC-0193-07 — el formato postcard de `Edge` no cambia.
#[test]
#[ignore = "SPEC-0193 propuesta: pendiente de implementación"]
// @spec AC-0193-07
fn test_ac_0193_07_edge_format_unchanged() {
    panic!("SPEC-0193 pendiente: golden postcard de Edge");
}
