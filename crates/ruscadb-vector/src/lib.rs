//! # ruscadb-vector
//!
//! Índice vectorial ANN de RuscaDB: **HNSW** primario, IVF+PQ como fallback y
//! filtrado vectorial híbrido por selectividad (pre/in/post + iFVS).
//!
//! Implementa el puerto `VectorIndex` de `ruscadb-core`.
//! Diseño: ADR-004/ADR-009 y `docs/RuscaDB-roadmap.md` §5.4. Fase: F3.

#![forbid(unsafe_code)]
