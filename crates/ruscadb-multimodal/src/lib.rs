//! # ruscadb-multimodal
//!
//! Ingesta multimodal de RuscaDB: blobs content-addressed (CAS sha256 +
//! refcount) para imagen/audio/video/texto y orquestación del pipeline
//! blob → embedding → índice.
//!
//! Implementa el puerto `BlobStore` de `ruscadb-core`.
//! Diseño: `docs/RuscaDB-roadmap.md` §4.5 y §5. Fase: F4.

#![forbid(unsafe_code)]
