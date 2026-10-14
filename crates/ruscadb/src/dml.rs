//! Ejecución de sentencias DML (SPEC-0043): `INSERT`, `UPDATE` y `DELETE`.
//!
//! `INSERT` multi-fila usa [`Database::insert_many`] (un solo commit); `UPDATE`
//! reescribe las filas visibles que cumplen el `WHERE`; `DELETE` usa el borrado
//! lógico MVCC ([`Database::delete`]). Las tres sentencias devuelven una fila
//! `{"affected": N}` (documentado en [`Database::execute`]).

use ruscadb_core::{EdgeSet, Record, RecordId, RecordMeta, RuscaError, ScalarMap, ScalarValue};
use ruscadb_query::{Delete, Expr, Insert, Projection, Update};
use ruscadb_txn::Snapshot;

use crate::catalog::{Catalog, TableDef};
use crate::database::Database;
use crate::executor::{Row, is_visible, keeps_row, literal_scalar, project_row};
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
        let ids = self.insert_many(&insert.table, records)?;
        if insert.returning.is_none() {
            return Ok(vec![affected_row(ids.len() as i64)]);
        }
        // `RETURNING` (SPEC-0102): relee las filas insertadas y las proyecta.
        let table = Catalog::load(self)?.get(&insert.table)?.clone();
        let projection = returning_projection(insert.returning.as_deref());
        let mut rows = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(record) = self.get_record(&insert.table, &id)? {
                rows.push(project_row(&table, &projection, &record.scalars)?);
            }
        }
        Ok(rows)
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
        let mut returned: Vec<Record> = Vec::new();
        for (locator, record) in candidates {
            if record.meta.deleted_tx.is_some() || !is_visible(snapshot, &record) {
                continue;
            }
            if !keeps_row(&table, update.filter.as_ref(), &record)? {
                continue;
            }
            let updated = rewrite_row(
                self,
                &mut catalog,
                &table,
                &record,
                &update.assignments,
                locator,
                tx,
            )?;
            if update.returning.is_some() {
                returned.push(updated);
            }
            affected += 1;
        }
        catalog.save(self)?;
        if is_auto_commit {
            self.txn.commit(tx)?;
        }
        // Coherencia del caché (SPEC-0069): un UPDATE invalida entradas.
        self.invalidate_semantic_cache();
        if let Some(columns) = &update.returning {
            return project_records(&table, columns, &returned);
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
        let mut candidates: Vec<Record> = Vec::new();
        for (_, record) in heap_scan(self, &table)? {
            if record.meta.deleted_tx.is_some() || !is_visible(snapshot, &record) {
                continue;
            }
            if keeps_row(&table, delete.filter.as_ref(), &record)? {
                candidates.push(record);
            }
        }
        let mut affected = 0_i64;
        let mut deleted: Vec<Record> = Vec::new();
        for record in candidates {
            if self.delete(&delete.table, &record.id)? {
                affected += 1;
                if delete.returning.is_some() {
                    deleted.push(record);
                }
            }
        }
        if let Some(columns) = &delete.returning {
            return project_records(&table, columns, &deleted);
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
///     El [`Record`] actualizado tras persistir la versión y mantener los
///     índices (lo consume `RETURNING`, SPEC-0102).
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
) -> Result<Record, RuscaError> {
    let updated = apply_assignments(database, table, record, assignments, tx)?;
    persist_row_update(database, catalog, table, record, &updated, locator)?;
    Ok(updated)
}

/// Persiste una versión actualizada de una fila manteniendo los índices.
///
/// Punto único de escritura de una actualización (SPEC-0043 y SPEC-0091):
/// retira la entrada del índice secundario del valor **anterior**, reescribe la
/// versión en su localizador del heap, inserta la entrada del valor **nuevo** y
/// refresca el índice invertido. Se separa de [`rewrite_row`] para que la API
/// estructurada por `RecordId` ([`Database::update`]) reuse exactamente la
/// misma ruta de persistencia que el `UPDATE ... WHERE` en RQL.
///
/// Args:
///     database: Base abierta (asignador de páginas + índices en memoria).
///     catalog: Catálogo cargado (definición de la tabla).
///     table: Definición de la tabla objetivo.
///     previous: Versión viva original (inmutable) usada para retirar índices.
///     updated: Nueva versión a persistir (`created_tx`/`lsn` ya estampados).
///     locator: Localizador de la fila (no cambia con la actualización).
///
/// Returns:
///     `Ok(())` tras persistir la versión y mantener los índices.
///
/// Errors:
///     [`RuscaError::TypeMismatch`] si el valor indexado no pertenece a su
///     columna; [`RuscaError::CorruptManifest`] si la fila actualizada no cabe.
pub(crate) fn persist_row_update(
    database: &mut Database,
    catalog: &mut Catalog,
    table: &TableDef,
    previous: &Record,
    updated: &Record,
    locator: RowLocator,
) -> Result<(), RuscaError> {
    if let Some(old_value) = indexed_value(table, previous) {
        index_remove(database, catalog, &table.name, &old_value, locator)?;
    }
    heap_update(database, catalog, &table.name, locator, updated)?;
    if let Some(new_value) = indexed_value(table, updated) {
        index_insert(database, catalog, &table.name, &new_value, locator)?;
    }
    if let Some(indexes) = database.indexes.get_mut(&table.name) {
        indexes.refresh_text(updated, table);
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

/// Convierte las columnas de `RETURNING` en una [`Projection`].
///
/// Args:
///     columns: Columnas pedidas (`["*"]` = todas), o `None` (todas).
///
/// Returns:
///     La proyección equivalente.
fn returning_projection(columns: Option<&[String]>) -> Projection {
    match columns {
        Some(columns) if !columns.iter().any(|column| column == "*") => {
            Projection::Columns(columns.to_vec())
        }
        _ => Projection::All,
    }
}

/// Proyecta los registros devueltos por `RETURNING` (SPEC-0102).
///
/// Args:
///     table: Definición de la tabla (valida columnas y ordena `*`).
///     columns: Columnas pedidas (`["*"]` = todas).
///     records: Registros afectados (insertados/actualizados/borrados).
///
/// Returns:
///     Una fila por registro con las columnas pedidas.
///
/// Errors:
///     [`RuscaError::ColumnNotFound`] si una columna pedida no existe.
fn project_records(
    table: &TableDef,
    columns: &[String],
    records: &[Record],
) -> Result<Vec<Row>, RuscaError> {
    let projection = returning_projection(Some(columns));
    let mut rows = Vec::with_capacity(records.len());
    for record in records {
        rows.push(project_row(table, &projection, &record.scalars)?);
    }
    Ok(rows)
}
