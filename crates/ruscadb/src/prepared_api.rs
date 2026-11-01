//! API publica de sentencias preparadas (SPEC-0184).
//!
//! `prepare` valida marcadores (`?`/`$n`) y fija el texto; `execute_prepared`
//! solo acepta valores (`ParamValue`), nunca SQL concatenado. El plan se
//! identifica por `plan_cache_key` (FNV-1a) para reutilizarlo.

use ruscadb_core::RuscaError;

use crate::database::Database;
use crate::executor::Row;
use crate::prepared_real::{ParamType, ParamValue, PreparedStmtReal};

/// Handle opaco de una sentencia preparada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedHandle {
    inner: PreparedStmtReal,
}

impl PreparedHandle {
    /// Texto validado de la consulta.
    ///
    /// Returns:
    ///     SQL original (sin bind).
    pub fn query(&self) -> &str {
        self.inner.query()
    }

    /// Numero de parametros esperados.
    ///
    /// Returns:
    ///     Aridad.
    pub fn param_count(&self) -> usize {
        self.inner.param_count()
    }

    /// Clave estable de cache de plan (FNV-1a 64).
    ///
    /// Returns:
    ///     Hash de la consulta normalizada.
    pub fn plan_cache_key(&self) -> u64 {
        self.inner.plan_cache_key()
    }
}

impl Database {
    /// Prepara una consulta sin declarar tipos (cada parametro acepta
    /// `Int`/`Float`/`Text`/`Bool`/`NULL`).
    ///
    /// Args:
    ///     sql: Consulta con marcadores `?` o `$1..$n`.
    ///
    /// Returns:
    ///     Handle reutilizable.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la consulta esta vacia o los
    ///     marcadores no son contiguos.
    pub fn prepare(&self, sql: &str) -> Result<PreparedHandle, RuscaError> {
        let inner = PreparedStmtReal::prepare_any(sql)
            .map_err(|error| RuscaError::InvalidConfig(format!("prepare: {error}")))?;
        Ok(PreparedHandle { inner })
    }

    /// Prepara una consulta con tipos declarados por posicion.
    ///
    /// Args:
    ///     sql: Consulta con marcadores.
    ///     types: Tipos esperados (`ParamType`), uno por marcador.
    ///
    /// Returns:
    ///     Handle con validacion de tipos en cada bind.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la aridad o los marcadores no
    ///     cuadran.
    pub fn prepare_typed(
        &self,
        sql: &str,
        types: Vec<ParamType>,
    ) -> Result<PreparedHandle, RuscaError> {
        let inner = PreparedStmtReal::prepare_typed(sql, types)
            .map_err(|error| RuscaError::InvalidConfig(format!("prepare: {error}")))?;
        Ok(PreparedHandle { inner })
    }

    /// Ejecuta una sentencia preparada enlazando valores.
    ///
    /// Args:
    ///     handle: Handle devuelto por [`Self::prepare`].
    ///     params: Valores en el orden de los marcadores.
    ///
    /// Returns:
    ///     Filas del `SELECT` (o vacio para DML sin `RETURNING`).
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la aridad/tipo no cuadra;
    ///     las del motor al ejecutar la consulta.
    pub fn execute_prepared(
        &mut self,
        handle: &PreparedHandle,
        params: &[ParamValue],
    ) -> Result<Vec<Row>, RuscaError> {
        let sql = handle
            .inner
            .bind_values(params)
            .map_err(|error| RuscaError::InvalidConfig(format!("bind: {error}")))?;
        self.execute(&sql)
    }
}
