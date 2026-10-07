//! Fuzz target del recovery del WAL (`ruscadb_wal::recover`).
//!
//! Invariante: ante bytes arbitrarios (frames truncados, CRC inválido,
//! tampering) `recover` nunca entra en pánico; descarta la cola inválida o
//! devuelve `RuscaError` (STRIDE: Tampering / DoS del recovery, roadmap §6.2).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // WAL temporal por iteración: aísla el estado (SBX) y evita carreras
    // entre workers del fuzzer.
    let Ok(dir) = tempfile::tempdir() else {
        return;
    };
    let path = dir.path().join("wal.log");
    if std::fs::write(&path, data).is_err() {
        return;
    }

    // `recover` trunca in-place: debe tolerar cualquier contenido.
    let _ = ruscadb_wal::recover(&path);
});
