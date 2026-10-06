//! # ruscadb-ai
//!
//! Inferencia local de embeddings de RuscaDB: **Candle** por defecto (puro
//! Rust) con backend `onnx` opcional. Modelos: CLIP (imagen/texto), Whisper
//! (audio→texto), e5/MiniLM (texto).
//!
//! Implementa el puerto `EmbedPort` de `ruscadb-core`.
//! Diseño: ADR-005 y `docs/RuscaDB-roadmap.md` §6. Fase: F4.

#![forbid(unsafe_code)]
