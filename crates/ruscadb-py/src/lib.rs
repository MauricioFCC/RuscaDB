//! # ruscadb-py
//!
//! Bindings Python de RuscaDB sobre el C ABI (`ruscadb-ffi`) mediante **PyO3**.
//! Driver fino: no reimplementa lógica, solo adapta la API al lenguaje.
//!
//! Diseño: ADR-011. Fase: F5. La dependencia `pyo3` se añade cuando se
//! implemente el binding (F0 no la compila para mantener el workspace liviano).

#![forbid(unsafe_code)]
