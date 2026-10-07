//! Borrado lógico MVCC de filas (SPEC-0022).
//!
//! `Database::delete` no elimina físicamente la fila: estampa
//! `record.meta.deleted_tx` con la transacción activa (o una de auto-commit),
//! reescribe la versión en su localizador y la retira de los índices derivados.
//! Las versiones son **inmutables** y la visibilidad la decide el snapshot
//! ([`ruscadb_txn::Snapshot::is_visible`]), por lo que un snapshot anterior al
//! borrado sigue viendo la fila. La **purga física** de versiones muertas queda
//! fuera de alcance (no hay GC/reaper en SPEC-0022).
//!
//! ## Localización del locator
//!
//! El borrado es por [`RecordId`], no por una columna indexada. No existe un
//! índice primario sobre `RecordId` (el índice secundario de `SPEC-0012` indexa
//! una columna escalar elegida por el usuario), así que la fila se localiza con
//! un **scan del heap** que compara `record.id`; el diseño de SPEC-0022 admite
//! este fallback cuando no hay índice primario.

use ruscadb_core::{Record, RecordId, RuscaError, ScalarValue};
use ruscadb_txn::TxId;
use ruscadb_wal::Lsn;

use crate::catalog::{Catalog, TableDef};
use crate::database::Database;
use crate::heap::{RowLocator, heap_scan, heap_update};
use crate::index::index_remove;

impl Database {
    /// Marca una fila como borrada lógicamente (MVCC) por su identificador.
    ///
    /// Localiza la fila (scan del heap por `RecordId`), estampa
    /// `meta.deleted_tx` con la transacción activa ([`Database::begin`]) o con
    /// una de auto-commit, fija `meta.lsn`, reescribe la versión en su
    /// localizador y la retira de los índices derivados (HNSW/CSR/invertido) y
    /// del índice secundario de escalares. En auto-commit confirma con WAL-first.
    ///
    /// Es idempotente: si el id no existe o ya está borrado devuelve `Ok(false)`
    /// sin efectos ni pánicos.
    ///
    /// Args:
    ///     table: Nombre de la tabla (debe existir).
    ///     id: Identificador de la fila a borrar.
    ///
    /// Returns:
    ///     `Ok(true)` si la fila existía viva y quedó borrada; `Ok(false)` si no
    ///     existe o ya estaba borrada.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::CorruptManifest`] si el heap es inválido;
    ///     [`RuscaError::Io`] si falla la escritura o la confirmación.
    pub fn delete(&mut self, table: &str, id: &RecordId) -> Result<bool, RuscaError> {
        let mut catalog = Catalog::load(self)?;
        let definition = catalog.get(table)?.clone();
        let Some((locator, record)) = locate_row(self, &definition, id)? else {
            return Ok(false);
        };
        if record.meta.deleted_tx.is_some() {
            return Ok(false);
        }
        let (tx, is_auto_commit) = match self.active_tx {
            Some(tx) => (tx, false),
            None => (self.txn.begin(), true),
        };
        let deleted = stamp_deleted(record, tx, self.wal.next_lsn());
        heap_update(self, locator, &deleted)?;
        remove_secondary_entry(self, &mut catalog, table, &deleted, locator)?;
        if let Some(indexes) = self.indexes.get_mut(table) {
            indexes.remove_record(&deleted);
        }
        catalog.save(self)?;
        if is_auto_commit {
            self.txn.commit(tx)?;
        }
        Ok(true)
    }
}

/// Busca por `RecordId` la fila viva o borrada y devuelve su localizador.
///
/// Args:
///     database: Base abierta.
///     table: Definición de la tabla a escanear.
///     id: Identificador buscado.
///
/// Returns:
///     `Some((localizador, registro))` si el id existe; `None` en otro caso.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si una página del heap es inválida.
fn locate_row(
    database: &mut Database,
    table: &TableDef,
    id: &RecordId,
) -> Result<Option<(RowLocator, Record)>, RuscaError> {
    for (locator, record) in heap_scan(database, table)? {
        if record.id == *id {
            return Ok(Some((locator, record)));
        }
    }
    Ok(None)
}

/// Estampa la metadata de borrado (`deleted_tx` + `lsn`) en una versión nueva.
///
/// Args:
///     record: Registro vivo original (inmutable).
///     tx: Transacción que borra la versión.
///     lsn: LSN del WAL asociado.
///
/// Returns:
///     El registro con `meta.deleted_tx = Some(tx)`.
fn stamp_deleted(mut record: Record, tx: TxId, lsn: Lsn) -> Record {
    record.meta.deleted_tx = Some(tx);
    record.meta.lsn = lsn;
    record
}

/// Retira la entrada del índice secundario de escalares de la fila borrada.
///
/// Args:
///     database: Base abierta.
///     catalog: Catálogo (asignador *bump* + definición de la tabla).
///     table: Tabla dueña del índice.
///     record: Registro borrado (para leer el valor de la columna indexada).
///     locator: Localizador de la fila.
///
/// Errors:
///     [`RuscaError::TableNotFound`] / [`RuscaError::TypeMismatch`] si el valor
///     no pertenece a la columna indexada.
fn remove_secondary_entry(
    database: &mut Database,
    catalog: &mut Catalog,
    table: &str,
    record: &Record,
    locator: RowLocator,
) -> Result<(), RuscaError> {
    let Some(column) = catalog
        .get(table)?
        .index
        .as_ref()
        .map(|index| index.column.clone())
    else {
        return Ok(());
    };
    let value = record
        .scalars
        .get(&column)
        .cloned()
        .unwrap_or(ScalarValue::Null);
    index_remove(database, catalog, table, &value, locator)
}
