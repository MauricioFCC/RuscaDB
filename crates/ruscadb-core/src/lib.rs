//! # ruscadb-core
//!
//! Dominio de RuscaDB (hexágono interno): modelo de datos unificado
//! [`Record`], catálogo, tipos puros y **puertos** (traits) agnósticos de
//! infraestructura.
//!
//! ## Reglas de este crate
//!
//! - **No depende de ningún adapter** (`storage`, `query`, `wal`, `ai`, ...).
//! - **Cero `unsafe`**: `#![forbid(unsafe_code)]` (ver
//!   `docs/RuscaDB-roadmap.md` §6.3).
//! - Solo tipos y contratos; ninguna operación de I/O.
//!
//! La implementación del `Record` y el catálogo se especifica en
//! `specs/core_record.md` (SPEC-0001) y se sintetiza en la Fase F1.

#![forbid(unsafe_code)]

/// Versión del dominio, tomada de la del paquete.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::CORE_VERSION;

    /// El crate expone una versión no vacía (smoke test del esqueleto F0).
    #[test]
    fn core_version_is_not_empty() {
        assert!(!CORE_VERSION.is_empty());
    }
}
