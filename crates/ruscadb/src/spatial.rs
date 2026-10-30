//! Indice espacial R-tree (SPEC-0114): re-exporta desde `geo` (SPEC-0112).
//!
//! El feature `spatial` implica `geo`, por lo que `RTree`, `Point` y `Rect`
//! estan disponibles via `ruscadb::geo` cuando `geo` esta activo. Este modulo
//! existe para mantener la compatibilidad con codigo que referencia
//! `ruscadb::spatial`.
