//! # ruscadb-query
//!
//! Motor de consultas de RuscaDB: **Apache DataFusion** como base SQL más un
//! dialecto extendido tipo SurrealQL (`->`, `@>`, `TRAVERSE`, `KNN(...)`).
//!
//! Diseño: ADR-001/ADR-007 y `docs/RuscaDB-roadmap.md` §4. Fase: F2.

#![forbid(unsafe_code)]
