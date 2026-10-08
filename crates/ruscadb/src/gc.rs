//! Introspección del catálogo y GC MVCC cableado (SPEC-0028).
//!
//! - [`Database::tables`]: devuelve los nombres de tabla del catálogo.
//! - [`Database::reap`]: purga física de filas obsoletas según el
//!   `low_watermark` del [`TxnManager`](ruscadb_txn::TxnManager).
//!   No compacta el grafo/FTS (fuera de alcance): esos índices ya excluyen a
//!   las filas borradas vía tombstones desde el borrado lógico.

use std::collections::BTreeMap;

use ruscadb_core::{RecordId, RuscaError};
use ruscadb_storage::PageId;
use ruscadb_txn::{Version, is_obsolete};

use crate::catalog::Catalog;
use crate::database::Database;
use crate::heap::{RowLocator, heap_remove, heap_scan};

impl Database {
    /// Devuelve los nombres de tabla del catálogo, en orden.
    ///
    /// Carga el catálogo persistido si hace falta (base reabierta).
    ///
    /// Returns:
    ///     Nombres de tabla (vacío si la base es nueva).
    ///
    /// Errors:
    ///     [`RuscaError::CorruptManifest`] si el catálogo es inválido;
    ///     [`RuscaError::Io`] si falla la lectura.
    pub fn tables(&mut self) -> Result<Vec<String>, RuscaError> {
        Ok(Catalog::load(self)?.table_names())
    }

    /// Purga física de filas obsoletas del heap (GC MVCC).
    ///
    /// Calcula `watermark = txn.low_watermark()`, escanea todas las tablas y
    /// purga cada fila con `deleted_tx` donde
    /// `is_obsolete(Version{created, deleted}, watermark)` es `true`:
    /// la elimina de su página, retira su entrada del índice primario,
    /// decrementa `row_count` y persiste el catálogo. Es idempotente y nunca
    /// toca filas vivas ni con `deleted_tx >= watermark`.
    ///
    /// No compacta el grafo/FTS (fuera de alcance, SPEC-0028): esos índices
    /// derivados ya excluían a las filas borradas desde el borrado lógico.
    ///
    /// Returns:
    ///     Número de filas purgadas (`0` si no había obsoletas).
    ///
    /// Errors:
    ///     [`RuscaError::CorruptManifest`] si el heap/catálogo es inválido;
    ///     [`RuscaError::Io`] si falla la escritura o la confirmación.
    pub fn reap(&mut self) -> Result<usize, RuscaError> {
        let watermark = self.txn.low_watermark();
        let targets = collect_obsolete(self, watermark)?;
        if targets.is_empty() {
            return Ok(0);
        }
        purge_targets(self, &targets)?;
        Ok(targets.len())
    }
}

/// Fila obsoleta pendiente de purga física.
struct ReapTarget {
    /// Tabla dueña de la fila.
    table: String,
    /// Localizador físico en el heap.
    locator: RowLocator,
    /// Identificador de la fila (para el índice primario).
    id: RecordId,
}

/// Recolecta las filas obsoletas de todas las tablas del heap.
///
/// Args:
///     database: Base abierta.
///     watermark: Low watermark actual.
///
/// Returns:
///     Objetivos de purga (vacío si no hay obsoletas).
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el heap es inválido.
fn collect_obsolete(
    database: &mut Database,
    watermark: ruscadb_txn::TxId,
) -> Result<Vec<ReapTarget>, RuscaError> {
    let catalog = Catalog::load(database)?;
    let mut targets = Vec::new();
    for name in catalog.table_names() {
        let table = catalog.get(&name)?.clone();
        for (locator, record) in heap_scan(database, &table)? {
            let version = Version {
                created_tx: record.meta.created_tx,
                deleted_tx: record.meta.deleted_tx,
            };
            if is_obsolete(&version, watermark) {
                targets.push(ReapTarget {
                    table: name.clone(),
                    locator,
                    id: record.id,
                });
            }
        }
    }
    Ok(targets)
}

/// Purga los objetivos: elimina del heap, actualiza catálogo y reindexa.
///
/// Agrupa por página y purga en orden descendente de slot (los índices
/// menores siguen válidos tras cada eliminación). Decrementa `row_count`,
/// persiste el catálogo (WAL-first) y reconstruye el índice primario, cuyos
/// localizadores pudieron desplazarse.
///
/// Args:
///     database: Base abierta.
///     targets: Objetivos de `collect_obsolete` (no vacío).
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el heap/catálogo es inválido;
///     [`RuscaError::Io`] si falla la escritura o la confirmación.
fn purge_targets(database: &mut Database, targets: &[ReapTarget]) -> Result<(), RuscaError> {
    let mut by_page: BTreeMap<(String, PageId), Vec<(u32, RecordId)>> = BTreeMap::new();
    for target in targets {
        by_page
            .entry((target.table.clone(), target.locator.0))
            .or_default()
            .push((target.locator.1, target.id));
    }
    let mut purged_per_table: BTreeMap<String, u64> = BTreeMap::new();
    for ((table, page), mut slots) in by_page {
        slots.sort_by_key(|slot| std::cmp::Reverse(slot.0));
        for (slot, id) in slots {
            heap_remove(database, (page, slot))?;
            database.primary_remove(&table, &id);
            *purged_per_table.entry(table.clone()).or_default() += 1;
        }
    }
    let mut catalog = Catalog::load(database)?;
    for (table, purged) in &purged_per_table {
        let definition = catalog.get_mut(table)?;
        definition.row_count = definition.row_count.saturating_sub(*purged);
    }
    catalog.save(database)?;
    database.rebuild_primary_index()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnDef, ColumnType, ScalarMap, ScalarValue};
    use proptest::prelude::*;
    use ruscadb_core::RecordId;

    /// Abre una base temporal con pool amplio.
    fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join(format!("{tag}.db"));
        let database = Database::open(crate::DbConfig::new(&path, 64)).expect("apertura");
        (dir, database)
    }

    /// Crea `t(a INT)` con `count` filas y devuelve sus ids.
    fn seed_rows(database: &mut Database, table: &str, count: usize) -> Vec<RecordId> {
        let mut ids = Vec::with_capacity(count);
        for value in 0..count {
            let mut scalars = ScalarMap::new();
            scalars.insert("a".to_string(), ScalarValue::Int(value as i64));
            ids.push(database.insert(table, scalars).expect("insert"));
        }
        ids
    }

    /// Crea la tabla `t(a INT)`.
    fn create_int_table(database: &mut Database, table: &str) {
        database
            .create_table(
                table,
                vec![ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("create_table");
    }

    /// AC-0028-02 — `tables()` devuelve los nombres exactos creados.
    #[test]
    fn test_ac_0028_02_tables_lists_created_tables() {
        let (_dir, mut database) = open_test_db("tables");
        assert_eq!(database.tables().expect("tablas"), Vec::<String>::new());
        create_int_table(&mut database, "alpha");
        create_int_table(&mut database, "beta");
        assert_eq!(
            database.tables().expect("tablas"),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    /// AC-0028-03 — `reap()` purga las filas obsoletas y cuenta exacto.
    #[test]
    fn test_ac_0028_03_reap_purges_obsolete() {
        let (_dir, mut database) = open_test_db("reap-purge");
        create_int_table(&mut database, "t");
        let ids = seed_rows(&mut database, "t", 3);
        assert_eq!(
            database
                .catalog()
                .expect("catálogo")
                .get("t")
                .expect("tabla")
                .row_count,
            3
        );
        assert!(database.delete("t", &ids[0]).expect("delete"));
        assert!(database.delete("t", &ids[1]).expect("delete"));
        let purged = database.reap().expect("reap");
        assert_eq!(purged, 2);
        assert_eq!(
            database
                .catalog()
                .expect("catálogo")
                .get("t")
                .expect("tabla")
                .row_count,
            1
        );
        let rows = database.execute("SELECT * FROM t").expect("select");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("a").expect("columna a"), &ScalarValue::Int(2));
        assert_eq!(database.reap().expect("segundo reap"), 0);
    }

    /// AC-0028-04 — `reap()` conserva vivas y borradas recientes.
    #[test]
    fn test_ac_0028_04_reap_keeps_live_and_recent() {
        let (_dir, mut database) = open_test_db("reap-keep");
        create_int_table(&mut database, "t");
        let ids = seed_rows(&mut database, "t", 2);
        // Ancla el watermark por debajo del futuro borrador: el primer `begin`
        // queda en vuelo (toda op mutante confirma solo la tx activa, así que
        // la segunda queda libre para el `delete` y la primera sigue viva).
        let _anchor = database.begin();
        let _active = database.begin();
        assert!(database.delete("t", &ids[0]).expect("delete reciente"));
        let purged = database.reap().expect("reap");
        assert_eq!(purged, 0);
        let rows = database.execute("SELECT * FROM t").expect("select");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("a").expect("columna a"), &ScalarValue::Int(1));
    }

    /// BVA — base vacía: `reap()` es `0` sin efectos.
    #[test]
    fn test_bva_0028_reap_empty_database_returns_zero() {
        let (_dir, mut database) = open_test_db("reap-empty");
        assert_eq!(database.reap().expect("reap"), 0);
        assert_eq!(database.tables().expect("tablas"), Vec::<String>::new());
    }

    /// BVA — sin borrados: `reap()` conserva todo.
    #[test]
    fn test_bva_0028_reap_without_deletes_keeps_all() {
        let (_dir, mut database) = open_test_db("reap-nodelete");
        create_int_table(&mut database, "t");
        seed_rows(&mut database, "t", 5);
        assert_eq!(database.reap().expect("reap"), 0);
        let rows = database.execute("SELECT * FROM t").expect("select");
        assert_eq!(rows.len(), 5);
    }

    /// BVA — todo borrado por debajo del watermark: purga total + idempotente.
    #[test]
    fn test_bva_0028_reap_all_deleted_purges_all_and_idempotent() {
        let (_dir, mut database) = open_test_db("reap-all");
        create_int_table(&mut database, "t");
        let ids = seed_rows(&mut database, "t", 4);
        for id in &ids {
            assert!(database.delete("t", id).expect("delete"));
        }
        assert_eq!(database.reap().expect("reap"), 4);
        assert_eq!(database.reap().expect("reap repetido"), 0);
        let rows = database.execute("SELECT * FROM t").expect("select");
        assert!(rows.is_empty());
    }

    proptest::proptest! {
        /// PBT — `reap` nunca elimina una fila visible para un snapshot
        /// tomado antes (invariante de visibilidad MVCC).
        #[test]
        fn prop_reap_never_removes_visible_rows(
            live in 1usize..12,
            deleted in 1usize..12,
        ) {
            let (_dir, mut database) = open_test_db("reap-prop");
            create_int_table(&mut database, "t");
            let live_ids = seed_rows(&mut database, "t", live);
            let doomed = seed_rows(&mut database, "t", deleted);
            for id in &doomed {
                prop_assert!(database.delete("t", id).expect("delete"));
            }
            let snapshot = database.snapshot();
            let before = database.execute("SELECT * FROM t").expect("select");
            prop_assert_eq!(before.len(), live);
            let purged = database.reap().expect("reap");
            prop_assert_eq!(purged, deleted);
            let after = database.execute("SELECT * FROM t").expect("select");
            prop_assert_eq!(after, before);
            for id in &live_ids {
                let visible = database
                    .get_record("t", id)
                    .expect("get_record")
                    .is_some();
                prop_assert!(
                    visible,
                    "la fila viva {id:?} siguió visible tras reap (snapshot {snapshot:?})"
                );
            }
        }
    }
}
