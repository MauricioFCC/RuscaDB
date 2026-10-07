//! # ruscadb-py
//!
//! Bindings Python de RuscaDB sobre el C-ABI estable (`ruscadb-ffi`).
//!
//! Placeholder documentado de F5 (SPEC-0010): el binding definitivo sera un
//! driver fino que consuma el C-ABI sin reimplementar logica. Hoy el wrapper
//! funcional vive en el repositorio (Python `ctypes`) y este crate solo apunta
//! a el para no anadir `pyo3` mientras el workspace debe permanecer liviano.
//!
//! Ver `bindings/README.md` para compilar la biblioteca
//! (`cargo build -p ruscadb-ffi`) y usarla.

#![forbid(unsafe_code)]

/// Ruta del wrapper Python (`ctypes`) sobre el C-ABI, relativa a la raiz.
pub const PYTHON_WRAPPER: &str = "bindings/python/ruscadb.py";

/// Ruta del crate que define el C-ABI consumido por el wrapper.
pub const C_ABI_CRATE: &str = "crates/ruscadb-ffi";
