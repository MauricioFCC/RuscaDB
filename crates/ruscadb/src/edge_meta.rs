//! Metadatos de arista: payload JSON y `ts_unix` (SPEC-0193).
//!
//! Los metadatos viven en una tabla interna `__ruscadb_edge_meta` con una clave
//! sintética `edge_id = SHA-256(from_table, from_id, label, to_table, to_id)`.
//! Así se reutilizan heap, índice B+tree, MVCC, WAL y recovery existentes sin
//! cambiar el formato postcard de [`ruscadb_core::Edge`].
//!
//! Complejidad: lookup/insert `O(log E)` por el índice de `edge_id`; validación
//! del payload `O(P)` con `P <= 64 KiB`.

use sha2::{Digest, Sha256};

use ruscadb_core::{RecordId, RuscaError, ScalarMap, ScalarValue};
use ruscadb_txn::{Snapshot, Version};

use crate::catalog::{Catalog, ColumnDef, ColumnType, TableDef};
use crate::database::Database;
use crate::heap::{RowLocator, heap_read};
use crate::index::index_lookup_eq;

/// Nombre de la tabla interna de metadatos de arista.
pub const EDGE_META_TABLE: &str = "__ruscadb_edge_meta";
/// Columna índice de la clave sintética.
pub const EDGE_META_INDEX: &str = "edge_id";
/// Límite duro del payload serializado.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
/// Claves reservadas que el payload no puede usar.
const RESERVED_KEYS: [&str; 4] = ["from", "to", "label", "ts_unix"];

/// Metadatos públicos de una arista.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EdgeMeta {
    /// Payload JSON (objeto).
    pub payload: serde_json::Value,
    /// Timestamp Unix (segundos) de la arista.
    pub ts_unix: i64,
}

impl Database {
    /// Lee los metadatos de una arista, si existen.
    ///
    /// Args:
    ///     from_table: Tabla del nodo origen.
    ///     from_id: Id del nodo origen.
    ///     label: Etiqueta de la arista.
    ///     to_table: Tabla del nodo destino.
    ///     to_id: Id del nodo destino.
    ///
    /// Returns:
    ///     `Some(EdgeMeta)` si la arista tiene metadatos; `None` si es legacy o
    ///     no existe.
    ///
    /// Errors:
    ///     Errores de catálogo/lectura del heap o payload corrupto.
    pub fn edge_meta(
        &mut self,
        from_table: &str,
        from_id: &RecordId,
        label: &str,
        to_table: &str,
        to_id: &RecordId,
    ) -> Result<Option<EdgeMeta>, RuscaError> {
        let snapshot = self.snapshot();
        self.edge_meta_at(from_table, from_id, label, to_table, to_id, &snapshot)
    }

    /// Igual que [`Database::edge_meta`] con una visibilidad MVCC fija.
    ///
    /// Args:
    ///     from_table: Tabla del nodo origen.
    ///     from_id: Id del nodo origen.
    ///     label: Etiqueta de la arista.
    ///     to_table: Tabla del nodo destino.
    ///     to_id: Id del nodo destino.
    ///     snapshot: Vista de visibilidad de la consulta.
    ///
    /// Returns:
    ///     `Some(EdgeMeta)` si la arista tiene metadatos visibles.
    ///
    /// Errors:
    ///     Errores de catálogo/lectura del heap o payload corrupto.
    pub fn edge_meta_at(
        &mut self,
        from_table: &str,
        from_id: &RecordId,
        label: &str,
        to_table: &str,
        to_id: &RecordId,
        snapshot: &Snapshot,
    ) -> Result<Option<EdgeMeta>, RuscaError> {
        let catalog = Catalog::load(self)?;
        if !catalog.contains(EDGE_META_TABLE) {
            return Ok(None);
        }
        let table = catalog.get(EDGE_META_TABLE)?.clone();
        let key = edge_key(from_table, from_id, label, to_table, to_id);
        let Some((_, _, meta)) = lookup_edge_meta(self, &table, &key, snapshot)? else {
            return Ok(None);
        };
        Ok(Some(meta))
    }

    /// Crea la tabla interna de metadatos si no existe (idempotente).
    ///
    /// Errors:
    ///     Errores de catálogo/DDL.
    pub(crate) fn ensure_edge_meta_table(&mut self) -> Result<(), RuscaError> {
        let catalog = Catalog::load(self)?;
        if catalog.contains(EDGE_META_TABLE) {
            return Ok(());
        }
        drop(catalog);
        self.create_table(
            EDGE_META_TABLE,
            vec![
                ColumnDef {
                    name: EDGE_META_INDEX.to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "from_table".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "from_id".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "label".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "to_table".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "to_id".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "payload".to_string(),
                    col_type: ColumnType::Text,
                },
                ColumnDef {
                    name: "ts_unix".to_string(),
                    col_type: ColumnType::Int,
                },
            ],
        )?;
        self.create_index(EDGE_META_TABLE, EDGE_META_INDEX)
    }
}

/// Clave sintética estable de la arista.
pub(crate) fn edge_key(
    from_table: &str,
    from_id: &RecordId,
    label: &str,
    to_table: &str,
    to_id: &RecordId,
) -> String {
    let from_id = from_id.to_string();
    let to_id = to_id.to_string();
    let mut hasher = Sha256::new();
    for part in [from_table, &from_id, label, to_table, &to_id] {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    hex::encode(hasher.finalize())
}

/// Busca la fila de metadatos por clave.
///
/// Returns:
///     `(locator, record, meta)` si existe y está viva.
pub(crate) fn lookup_edge_meta(
    database: &mut Database,
    table: &TableDef,
    key: &str,
    snapshot: &Snapshot,
) -> Result<Option<(RowLocator, ruscadb_core::Record, EdgeMeta)>, RuscaError> {
    let value = ScalarValue::Text(key.to_string());
    for locator in index_lookup_eq(database, table, EDGE_META_INDEX, &value)? {
        let record = heap_read(database, locator)?;
        let version = Version {
            created_tx: record.meta.created_tx,
            deleted_tx: record.meta.deleted_tx,
        };
        if !snapshot.is_visible(&version) {
            continue;
        }
        let meta = meta_from_record(&record)?;
        return Ok(Some((locator, record, meta)));
    }
    Ok(None)
}

/// Extrae `EdgeMeta` de un registro de la tabla interna.
pub(crate) fn meta_from_record(record: &ruscadb_core::Record) -> Result<EdgeMeta, RuscaError> {
    let payload = match record.scalars.get("payload") {
        Some(ScalarValue::Text(text)) => serde_json::from_str(text).map_err(|error| {
            RuscaError::InvalidConfig(format!("payload de arista corrupto (no es JSON): {error}"))
        })?,
        _ => serde_json::Value::Object(serde_json::Map::new()),
    };
    let ts_unix = match record.scalars.get("ts_unix") {
        Some(ScalarValue::Int(value)) => *value,
        _ => 0,
    };
    Ok(EdgeMeta { payload, ts_unix })
}

/// Valida y canonicaliza un payload JSON de arista.
///
/// Args:
///     raw: Texto JSON crudo.
///
/// Returns:
///     El objeto validado y su serialización compacta.
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si no es un objeto, usa claves reservadas
///     o excede [`MAX_PAYLOAD_BYTES`].
pub(crate) fn parse_payload(raw: &str) -> Result<(serde_json::Value, String), RuscaError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        RuscaError::InvalidConfig(format!("payload de arista no es JSON válido: {error}"))
    })?;
    let serde_json::Value::Object(map) = &value else {
        return Err(RuscaError::InvalidConfig(
            "el payload de arista debe ser un objeto JSON { ... }".to_string(),
        ));
    };
    for key in RESERVED_KEYS {
        if map.contains_key(key) {
            return Err(RuscaError::InvalidConfig(format!(
                "el payload de arista no puede usar la clave reservada '{key}'"
            )));
        }
    }
    let canonical = serde_json::to_string(&value).map_err(|error| {
        RuscaError::InvalidConfig(format!("no se pudo serializar el payload: {error}"))
    })?;
    if canonical.len() > MAX_PAYLOAD_BYTES {
        return Err(RuscaError::InvalidConfig(format!(
            "payload de arista de {} bytes excede el límite de {MAX_PAYLOAD_BYTES}",
            canonical.len()
        )));
    }
    Ok((value, canonical))
}

/// Construye los escalares de una fila de metadatos.
#[allow(clippy::too_many_arguments)] // los 8 campos de la clave + payload/ts
pub(crate) fn edge_meta_scalars(
    key: &str,
    from_table: &str,
    from_id: &RecordId,
    label: &str,
    to_table: &str,
    to_id: &RecordId,
    payload: &str,
    ts_unix: i64,
) -> ScalarMap {
    ScalarMap::from([
        (
            EDGE_META_INDEX.to_string(),
            ScalarValue::Text(key.to_string()),
        ),
        (
            "from_table".to_string(),
            ScalarValue::Text(from_table.to_string()),
        ),
        (
            "from_id".to_string(),
            ScalarValue::Text(from_id.to_string()),
        ),
        ("label".to_string(), ScalarValue::Text(label.to_string())),
        (
            "to_table".to_string(),
            ScalarValue::Text(to_table.to_string()),
        ),
        ("to_id".to_string(), ScalarValue::Text(to_id.to_string())),
        (
            "payload".to_string(),
            ScalarValue::Text(payload.to_string()),
        ),
        ("ts_unix".to_string(), ScalarValue::Int(ts_unix)),
    ])
}
