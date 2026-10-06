//! # ruscadb-node
//!
//! Bindings Node.js de RuscaDB sobre el C ABI (`ruscadb-ffi`) mediante
//! **napi-rs**. Driver fino: solo adapta la API al lenguaje.
//!
//! Diseño: ADR-011. Fase: F5. La dependencia `napi` se añade al implementar
//! el binding (F0 no la compila para mantener el workspace liviano).

#![forbid(unsafe_code)]
