//! Introspección del catálogo y GC MVCC cableado (SPEC-0028, SPEC-0085).
//!
//! - [`Database::tables`]: devuelve los nombres de tabla del catálogo.
//! - [`Database::reap`]: purga física de filas obsoletas según el
//!   `low_watermark` del [`TxnManager`](ruscadb_txn::TxnManager), **libera**
//!   las páginas que quedan sin filas a la free-list persistida (SPEC-0085) y
//!   **reconstruye** los índices derivados (vectorial/grafo/FTS) desde las
//!   filas vivas, de modo que no queden entradas huérfanas (SPEC-0101).
//!
//! ## Crash-safety de la reclamación (NF-0085-01)
//!
//! La secuencia segura es: **purga en memoria → free-list en memoria → un único
//! commit atómico**. Se libera una página solo cuando *todas* sus filas son
//! obsoletas (`is_obsolete`), es decir, invisibles para todo snapshot (`<=` el
//! watermark); por eso reasignarla nunca pisa una fila viva. La retirada de la
//! página del catálogo (`table.pages`) y su alta en la free-list del manifiesto
//! ocurren antes del commit que las persiste juntas (`catalog.save` →
//! `commit`: WAL-first + `Manifest::store` `tmp+fsync+rename`). Un crash antes
//! del commit no publica ninguna de las dos (páginas en fuga, detectable); un
//! crash después publica ambas (consistente). Nunca se publica una página libre
//! antes que la purga que la vació.

use std::collections::BTreeMap;

use ruscadb_core::{RecordId, RuscaError};
use ruscadb_storage::PageId;
use ruscadb_txn::{Version, is_obsolete};

use crate::catalog::Catalog;
use crate::database::Database;
use crate::heap::{RowLocator, heap_page_is_empty, heap_remove, heap_scan};

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
    /// **SPEC-0085**: además, devuelve a la free-list persistida las páginas que
    /// quedan sin ninguna fila (su purga las vació) y reconstruye los índices
    /// derivados (vectorial/grafo/FTS) desde las filas vivas, eliminando las
    /// entradas huérfanas de las purgadas (SPEC-0101). La secuencia de
    /// crash-safety se documenta en el módulo: purga y liberación se persisten
    /// juntas en un único commit atómico, y solo se liberan páginas cuyas filas
    /// eran todas obsoletas (invisibles), de modo que reasignarlas nunca pisa
    /// una fila viva. Repetir `reap()` sin cambios no altera la free-list
    /// (idempotencia, NF-0085-02).
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

/// Purga los objetivos: elimina del heap, libera páginas, reconstruye derivados.
///
/// Agrupa por página y purga en orden descendente de slot (los índices menores
/// siguen válidos tras cada eliminación). Cuando una página de datos queda sin
/// ninguna fila, se retira de `table.pages` y se encola en la free-list del
/// manifiesto. Luego decrementa `row_count`, **persiste el catálogo y la
/// free-list en un único commit atómico** (WAL-first, `catalog.save`) y
/// reconstruye los índices derivados (vectorial/grafo/FTS) y el índice primario
/// desde las filas vivas ([`Database::compact_derived_indexes`], SPEC-0101):
/// así ninguna fila purgada conserva entradas en memoria.
///
/// La free-list se muta **antes** de `catalog.save` para que su `Manifest::store`
/// la publique junto con la purga (ver crash-safety del módulo): así nunca se
/// publica una página libre sin la purga que la vació.
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
    let mut reclaimed: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for ((table, page), mut slots) in by_page {
        slots.sort_by_key(|slot| std::cmp::Reverse(slot.0));
        for (slot, id) in slots {
            heap_remove(database, (page, slot))?;
            database.primary_remove(&table, &id);
            *purged_per_table.entry(table.clone()).or_default() += 1;
        }
        // Una página sin filas tras la purga es reutilizable (SPEC-0085).
        if heap_page_is_empty(database, page)? {
            reclaimed.entry(table).or_default().push(page.0);
        }
    }
    let mut catalog = Catalog::load(database)?;
    for (table, purged) in &purged_per_table {
        let definition = catalog.get_mut(table)?;
        definition.row_count = definition.row_count.saturating_sub(*purged);
    }
    for (table, pages) in &reclaimed {
        let definition = catalog.get_mut(table)?;
        definition.pages.retain(|raw| !pages.contains(raw));
    }
    // Alta en la free-list ANTES del commit que la persiste con la purga.
    for pages in reclaimed.values() {
        for raw in pages {
            database.release_page(PageId(*raw));
        }
    }
    catalog.save(database)?;
    // Rebuild desde las filas vivas (SPEC-0101): elimina las entradas
    // huérfanas de las purgadas en los índices derivados y recalcula los
    // localizadores del índice primario (el `heap_remove` los desplazó).
    database.compact_derived_indexes()?;
    Ok(())
}

impl Database {
    /// Reserva una página reutilizando la free-list o creciendo el fichero.
    ///
    /// **WHAT**: extrae una página de la free-list persistida si hay alguna
    /// válida (`< next_page`); si no, delega en el asignador *bump* del
    /// catálogo. **WHY**: reutilizar páginas liberadas por el reap mantiene
    /// estable el alto agua del almacén (FR-0085-01). **WHERE**: usado por el
    /// heap y el índice secundario de la fachada.
    ///
    /// Si la free-list contuviera un id fuera de rango (manifiesto corrupto) lo
    /// descarta silenciosamente (página en fuga detectable) y prueba el
    /// siguiente: nunca revienta ni reasigna una página inexistente.
    ///
    /// Args:
    ///     catalog: Catálogo abierto (asignador *bump* y `next_page`).
    ///
    /// Returns:
    ///     El `PageId` reservado, listo para escribir.
    pub(crate) fn allocate_page(&mut self, catalog: &mut Catalog) -> PageId {
        while let Some(raw) = self.manifest().take_free_page() {
            if raw < catalog.next_page() {
                return PageId(raw);
            }
        }
        catalog.alloc_page()
    }

    /// Devuelve una página a la free-list persistida (idempotente).
    ///
    /// La mutación es en memoria y se serializa con el próximo commit (el
    /// `Manifest::store` atómico); un `reap()` repetido no duplica la entrada.
    ///
    /// Args:
    ///     id: Página sin filas que se reutilizará.
    pub(crate) fn release_page(&self, id: PageId) {
        self.manifest().add_free_page(id.0);
    }

    /// Número de páginas en la free-list persistida (introspección).
    ///
    /// Returns:
    ///     La cantidad de páginas reutilizables.
    pub fn free_page_count(&self) -> usize {
        self.manifest().free_page_count()
    }

    /// Número de aristas únicas del grafo derivado de una tabla (SPEC-0085).
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///
    /// Returns:
    ///     La cardinalidad de aristas del CSR (`0` si la tabla no tiene índice
    ///     en memoria).
    pub fn graph_edge_count(&self, table: &str) -> usize {
        self.indexes
            .get(table)
            .map_or(0, crate::indexes::TableIndexes::graph_edge_count)
    }

    /// Número de vectores indexados del índice HNSW de una tabla (SPEC-0101).
    ///
    /// Señal observable de compactación: tras un `reap()` que purga filas
    /// vectoriales, el rebuild deja solo los vectores de las filas vivas, de
    /// modo que este conteo baja al de filas vivas con embedding.
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///
    /// Returns:
    ///     La longitud del corpus vectorial (`0` si la tabla no tiene índice en
    ///     memoria o ningún embedding).
    pub fn vector_index_len(&self, table: &str) -> usize {
        self.indexes
            .get(table)
            .map_or(0, crate::indexes::TableIndexes::vector_len)
    }

    /// Número de documentos vivos del índice invertido de `column`.
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///     column: Columna `TEXT` indexada.
    ///
    /// Returns:
    ///     El número de documentos vivos (`0` si no hay índice).
    pub fn fts_doc_count(&self, table: &str, column: &str) -> usize {
        self.indexes
            .get(table)
            .map_or(0, |indexes| indexes.fts_doc_count(column))
    }

    /// Número total de postings del índice invertido de una columna.
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///     column: Columna `TEXT` indexada.
    ///
    /// Returns:
    ///     La suma de postings (`0` si no hay índice).
    pub fn fts_posting_count(&self, table: &str, column: &str) -> usize {
        self.indexes
            .get(table)
            .map_or(0, |indexes| indexes.fts_posting_count(column))
    }
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
    // @spec AC-0028-02
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
    // @spec AC-0028-03
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
    // @spec AC-0028-04
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
