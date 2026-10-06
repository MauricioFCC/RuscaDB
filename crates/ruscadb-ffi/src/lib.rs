//! # ruscadb-ffi
//!
//! C ABI estable de RuscaDB: contrato único para todos los drivers. Valida
//! `(ptr, len)` antes de `from_raw_parts`, usa handle table con generación
//! (anti use-after-free) y `catch_unwind` en la frontera.
//!
//! Diseño: ADR-008/ADR-011 y `docs/RuscaDB-roadmap.md` §6.2. Fase: F5.

#![forbid(unsafe_code)]
