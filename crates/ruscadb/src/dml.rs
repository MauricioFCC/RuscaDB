//! Ejecución de sentencias DML (SPEC-0043): `INSERT`, `UPDATE` y `DELETE`.
//!
//! `INSERT` multi-fila usa [`Database::insert_many`] (un solo commit); `UPDATE`
//! reescribe las filas visibles que cumplen el `WHERE`; `DELETE` usa el borrado
//! lógico MVCC ([`Database::delete`]). Las tres sentencias devuelven una fila
//! `{"affected": N}` (documentado en [`Database::execute`]).

use ruscadb_core::{EdgeSet, Record, RecordId, RecordMeta, RuscaError, ScalarMap, ScalarValue};
use ruscadb_query::{Delete, Expr, Insert, Update};
use ruscadb_txn::Snapshot;

use crate::catalog::{Catalog, TableDef};
use crate::database::Database;
use crate::executor::{Row, is_visible, keeps_row, literal_scalar};
use crate::heap::{RowLocator, heap_scan, heap_update};
use crate::index::{index_insert, index_remove};
use crate::table_api::validate_scalars;

impl Database {
    /// Ejecuta un `INSERT` multi-fila con un solo commit.
    ///
    /// Args:
    ///     insert: Sentencia `INSERT` analizada (columnas + filas de literales).
    ///
    /// Returns:
    ///     Una fila `{"affected": N}` con el número de filas insertadas.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::ColumnNotFound`] si falta/sobra una columna;
    ///     [`RuscaError::TypeMismatch`] si un literal no pertenece a su columna.
    pub(crate) fn execute_insert(&mut self, insert: &Insert) -> Result<Vec<Row>, RuscaError> {
        let records = build_insert_records(insert)?;
        let inserted = self.insert_many(&insert.table, records)?;
        Ok(vec![affected_row(inserted.len() as i64)])
    }

    /// Ejecuta un `UPDATE` de las filas visibles que cumplen el `WHERE`.
    ///
    /// Args:
    ///     update: Sentencia `UPDATE` analizada.
    ///     snapshot: Vista de visibilidad MVCC de las filas a actualizar.
    ///
    /// Returns:
    ///     Una fila `{"affected": N}` con el número de filas actualizadas.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::ColumnNotFound`] si una columna asignada no existe;
    ///     [`RuscaError::TypeMismatch`] si un literal no pertenece a su columna.
    pub(crate) fn execute_update(
        &mut self,
        update: &Update,
        snapshot: &Snapshot,
    ) -> Result<Vec<Row>, RuscaError> {
        let mut catalog = Catalog::load(self)?;
        let table = catalog.get(&update.table)?.clone();
        validate_assignments(&table, &update.assignments)?;
        let candidates = heap_scan(self, &table)?;
        let (tx, is_auto_commit) = match self.active_tx {
            Some(tx) => (tx, false),
            None => (self.txn.begin(), true),
        };
        let mut affected = 0_i64;
        for (locator, record) in candidates {
            if record.meta.deleted_tx.is_some() || !is_visible(snapshot, &record) {
                continue;
            }
            if !keeps_row(&table, update.filter.as_ref(), &record)? {
                continue;
            }
            rewrite_row(
                self,
                &mut catalog,
                &table,
                &record,
                &update.assignments,
                locator,
                tx,
            )?;
            affected += 1;
        }
        catalog.save(self)?;
        if is_auto_commit {
            self.txn.commit(tx)?;
        }
        Ok(vec![affected_row(affected)])
    }

    /// Ejecuta un `DELETE` lógico (MVCC) de las filas que cumplen el `WHERE`.
    ///
    /// Args:
    ///     delete: Sentencia `DELETE` analizada.
    ///     snapshot: Vista de visibilidad MVCC de las filas a borrar.
    ///
    /// Returns:
    ///     Una fila `{"affected": N}` con el número de filas borradas.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     errores de heap/índice del borrado lógico.
    pub(crate) fn execute_delete(
        &mut self,
        delete: &Delete,
        snapshot: &Snapshot,
    ) -> Result<Vec<Row>, RuscaError> {
        let table = Catalog::load(self)?.get(&delete.table)?.clone();
        let mut victims = Vec::new();
        for (_, record) in heap_scan(self, &table)? {
            if record.meta.deleted_tx.is_some() || !is_visible(snapshot, &record) {
                continue;
            }
            if keeps_row(&table, delete.filter.as_ref(), &record)? {
                victims.push(record.id);
            }
        }
        let mut affected = 0_i64;
        for id in victims {
            if self.delete(&delete.table, &id)? {
                affected += 1;
            }
        }
        Ok(vec![affected_row(affected)])
    }
}

/// Construye los registros de un `INSERT` a partir de sus literales.
///
/// Args:
///     insert: Sentencia `INSERT` (columnas + filas).
///
/// Returns:
///     Un [`Record`] por fila, con los escalares alineados con las columnas.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si un valor no es un literal escalar.
fn build_insert_records(insert: &Insert) -> Result<Vec<Record>, RuscaError> {
    let mut records = Vec::with_capacity(insert.rows.len());
    for row in &insert.rows {
        let mut scalars = ScalarMap::new();
        for (column, value) in insert.columns.iter().zip(row) {
            scalars.insert(column.clone(), literal_scalar(value)?);
        }
        records.push(Record {
            id: RecordId::new(),
            scalars,
            doc: None,
            edges: EdgeSet::default(),
            vector: None,
            blob: None,
            meta: RecordMeta::default(),
        });
    }
    Ok(records)
}

/// Valida las columnas y literales de un `UPDATE` antes de tocar el heap.
///
/// Args:
///     table: Definición de la tabla.
///     assignments: Asignaciones `columna = literal`.
///
/// Returns:
///     `Ok(())` si todas las columnas existen y los valores son literales.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna no existe;
///     [`RuscaError::TypeMismatch`] si un valor no es un literal.
fn validate_assignments(
    table: &TableDef,
    assignments: &[(String, Expr)],
) -> Result<(), RuscaError> {
    for (column, value) in assignments {
        table.column_type(column)?;
        literal_scalar(value)?;
    }
    Ok(())
}

/// Reescribe una fila que cumple el `WHERE`: aplica asignaciones e índices.
///
/// Args:
///     database: Base abierta.
///     catalog: Catálogo (índice secundario + asignador de páginas).
///     table: Definición de la tabla.
///     record: Versión viva original (inmutable).
///     assignments: Asignaciones `columna = literal`.
///     locator: Localizador de la fila (no cambia).
///     tx: Transacción que estampa la nueva versión.
///
/// Returns:
///     `Ok(())` tras persistir la versión y mantener los índices.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si un valor no pertenece a su columna;
///     [`RuscaError::CorruptManifest`] si la fila actualizada no cabe.
fn rewrite_row(
    database: &mut Database,
    catalog: &mut Catalog,
    table: &TableDef,
    record: &Record,
    assignments: &[(String, Expr)],
    locator: RowLocator,
    tx: u64,
) -> Result<(), RuscaError> {
    if let Some(old_value) = indexed_value(table, record) {
        index_remove(database, catalog, &table.name, &old_value, locator)?;
    }
    let updated = apply_assignments(database, table, record, assignments, tx)?;
    heap_update(database, locator, &updated)?;
    if let Some(new_value) = indexed_value(table, &updated) {
        index_insert(database, catalog, &table.name, &new_value, locator)?;
    }
    if let Some(indexes) = database.indexes.get_mut(&table.name) {
        indexes.refresh_text(&updated, table);
    }
    Ok(())
}

/// Construye la versión actualizada de un registro (escalares + metadata).
///
/// Args:
///     database: Base abierta (para el próximo LSN).
///     table: Definición de la tabla (coerción de tipos).
///     record: Versión original.
///     assignments: Asignaciones `columna = literal`.
///     tx: Transacción que estampa `created_tx`.
///
/// Returns:
///     El registro con los escalares reescritos y el `lsn` al día.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si un valor no pertenece a su columna.
fn apply_assignments(
    database: &Database,
    table: &TableDef,
    record: &Record,
    assignments: &[(String, Expr)],
    tx: u64,
) -> Result<Record, RuscaError> {
    let mut scalars = record.scalars.clone();
    for (column, value) in assignments {
        scalars.insert(column.clone(), literal_scalar(value)?);
    }
    let mut updated = record.clone();
    updated.scalars = validate_scalars(table, &table.name, scalars)?;
    updated.meta.created_tx = tx;
    updated.meta.deleted_tx = None;
    updated.meta.lsn = database.wal.next_lsn();
    Ok(updated)
}

/// Valor de la columna indexada de un registro, si la tabla tiene índice.
///
/// Args:
///     table: Definición de la tabla.
///     record: Registro consultado.
///
/// Returns:
///     `Some(valor)` (posiblemente `NULL`) si hay índice; `None` si no.
fn indexed_value(table: &TableDef, record: &Record) -> Option<ScalarValue> {
    let column = table.index.as_ref().map(|index| index.column.clone())?;
    Some(
        record
            .scalars
            .get(&column)
            .cloned()
            .unwrap_or(ScalarValue::Null),
    )
}

/// Construye la fila de resultado `{"affected": N}` de una sentencia DML.
///
/// Args:
///     affected: Número de filas afectadas.
///
/// Returns:
///     Una fila con la única columna `affected`.
fn affected_row(affected: i64) -> Row {
    Row::from([("affected".to_string(), ScalarValue::Int(affected))])
}
