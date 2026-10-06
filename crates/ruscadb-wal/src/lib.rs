//! # ruscadb-wal
//!
//! Write-Ahead Log global de RuscaDB: frames con CRC32C, group commit
//! (coalescing fsync) y recovery por truncado del tail rasgado. MVCC con
//! snapshot isolation (visibilidad por `tx_id`).
//!
//! Implementa los puertos `Wal` y `TxnManager` de `ruscadb-core`.
//! Diseño: ADR-006 y `docs/RuscaDB-roadmap.md` §5.3.
//! Especificación: `specs/wal_durability.md` (SPEC-0002). Fase: F1.

#![forbid(unsafe_code)]
