//! # ruscadb-storage
//!
//! Adapter de almacenamiento columnar de RuscaDB: segmentos **Lance/Arrow**,
//! zero-copy, blobs multimodales y buffer pool con presupuesto duro.
//!
//! Implementa el puerto `StorageEngine` definido en `ruscadb-core`.
//! Diseño: `docs/RuscaDB-roadmap.md` §5. Fase de síntesis: F1.

#![forbid(unsafe_code)]
