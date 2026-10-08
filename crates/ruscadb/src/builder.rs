//! Constructor fluido del composition root (SPEC-0028, FR-0028-01).
//!
//! [`DatabaseBuilder`] es azúcar sobre [`DbConfig`]: acumula `data_path`,
//! `pool_capacity` y `encryption` con métodos encadenables y culmina en
//! [`DatabaseBuilder::open`], que construye el [`DbConfig`] equivalente y
//! delega en [`Database::open`]. Sin estados inválidos: `open` falla con el
//! mismo error que `Database::open` si la configuración es inválida
//! (p. ej. `pool_capacity == 0` → [`RuscaError::InvalidConfig`]).

use std::path::PathBuf;

use ruscadb_core::RuscaError;

use crate::database::{Database, DbConfig};
use crate::encryption::EncryptionConfig;

/// Capacidad del pool por defecto (marcos de 4 KiB).
const DEFAULT_POOL_CAPACITY: usize = 64;

/// Constructor fluido de [`Database`], equivalente a [`DbConfig`].
///
/// Ejemplo: `Database::builder().data_path("db.data").pool_capacity(64).open()`
/// equivale a `Database::open(DbConfig::new("db.data", 64))`.
#[derive(Clone, Debug)]
pub struct DatabaseBuilder {
    /// Ruta del archivo de páginas (`None` hasta llamar `data_path`).
    data_path: Option<PathBuf>,
    /// Marcos del buffer pool (por defecto [`DEFAULT_POOL_CAPACITY`]).
    pool_capacity: usize,
    /// Cifrado en reposo (`None` = modo claro).
    encryption: Option<EncryptionConfig>,
}

impl DatabaseBuilder {
    /// Crea un builder vacío (solo lo usa [`Database::builder`]).
    ///
    /// Returns:
    ///     Builder sin ruta y con capacidad por defecto.
    pub(crate) fn new() -> Self {
        Self {
            data_path: None,
            pool_capacity: DEFAULT_POOL_CAPACITY,
            encryption: None,
        }
    }

    /// Fija la ruta del archivo de páginas.
    ///
    /// Args:
    ///     path: Ruta del archivo de páginas.
    ///
    /// Returns:
    ///     El builder encadenable.
    pub fn data_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_path = Some(path.into());
        self
    }

    /// Fija el número de marcos del buffer pool.
    ///
    /// Args:
    ///     capacity: Marcos del pool (debe ser >= 1, se valida en `open`).
    ///
    /// Returns:
    ///     El builder encadenable.
    pub fn pool_capacity(mut self, capacity: usize) -> Self {
        self.pool_capacity = capacity;
        self
    }

    /// Fija el cifrado en reposo (`None` = modo claro).
    ///
    /// Args:
    ///     encryption: Configuración de cifrado o `None`.
    ///
    /// Returns:
    ///     El builder encadenable.
    pub fn encryption(mut self, encryption: Option<EncryptionConfig>) -> Self {
        self.encryption = encryption;
        self
    }

    /// Construye el [`DbConfig`] equivalente y abre la base.
    ///
    /// Returns:
    ///     La base lista para operar.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si falta `data_path` o si
    ///     `pool_capacity == 0` (igual que [`Database::open`]);
    ///     [`RuscaError::Io`] si falla el acceso a disco.
    pub fn open(self) -> Result<Database, RuscaError> {
        let Some(data_path) = self.data_path else {
            return Err(RuscaError::InvalidConfig(
                "falta data_path: fija la ruta con Database::builder().data_path(..)".to_string(),
            ));
        };
        let mut config = DbConfig::new(data_path, self.pool_capacity);
        config.encryption = self.encryption;
        Database::open(config)
    }
}

impl Database {
    /// Crea un [`DatabaseBuilder`] fluido.
    ///
    /// Equivalencia: `Database::builder().data_path(p).pool_capacity(n).open()`
    /// equivale a `Database::open(DbConfig { data_path: p, pool_capacity: n,
    /// encryption: None })`; con `.encryption(..)` equivale al `DbConfig` con
    /// ese cifrado.
    ///
    /// Returns:
    ///     Builder sin ruta y con capacidad por defecto.
    pub fn builder() -> DatabaseBuilder {
        DatabaseBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnDef, ColumnType, ScalarMap, ScalarValue};

    /// Abre una base de referencia con `DbConfig` directo.
    fn open_with_config(path: &std::path::Path) -> Database {
        Database::open(DbConfig::new(path, 64)).expect("apertura directa")
    }

    /// AC-0028-01 — el builder abre una base equivalente a `DbConfig`.
    #[test]
    fn test_ac_0028_01_builder_opens_equivalent_database() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("builder.db");
        let mut via_builder = Database::builder()
            .data_path(&path)
            .pool_capacity(64)
            .open()
            .expect("open del builder");
        via_builder
            .create_table(
                "t",
                vec![ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("create_table");
        let mut scalars = ScalarMap::new();
        scalars.insert("a".to_string(), ScalarValue::Int(7));
        via_builder.insert("t", scalars).expect("insert");
        via_builder.close().expect("close");

        let mut via_config = open_with_config(&path);
        let rows = via_config.execute("SELECT * FROM t").expect("select");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("a").expect("columna a"), &ScalarValue::Int(7));
    }

    /// AC-0028-05 — `pool_capacity(0)` falla igual que `DbConfig`.
    #[test]
    fn test_ac_0028_05_builder_rejects_invalid_config() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("invalida.db");
        let via_builder = match Database::builder().data_path(&path).pool_capacity(0).open() {
            Ok(_) => panic!("capacidad 0 debe fallar"),
            Err(error) => error,
        };
        let via_config = match Database::open(DbConfig::new(&path, 0)) {
            Ok(_) => panic!("DbConfig con 0 debe fallar"),
            Err(error) => error,
        };
        assert_eq!(
            std::mem::discriminant(&via_builder),
            std::mem::discriminant(&via_config),
            "el builder debe devolver el mismo error que DbConfig"
        );
        assert!(
            matches!(
                via_builder,
                RuscaError::InvalidConfig(_) | RuscaError::BufferPoolFull { .. }
            ),
            "se esperaba InvalidConfig o BufferPoolFull, se obtuvo {via_builder:?}"
        );
    }
}
