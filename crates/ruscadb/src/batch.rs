//! Escritura por lotes y aborto de transacción (SPEC-0027).
//!
//! `Database::insert_many` inserta N registros con **un único commit**
//! (un frame WAL + un `fsync`), en lugar de N commits como [`Database::insert`].
//! La ruta de una fila ([`Database::insert`]/[`Database::insert_record`]) se
//! mantiene intacta: `insert_many` reutiliza los mismos pasos por registro
//! (validación de esquema, heap, índice primario, índice secundario e índices
//! derivados) pero carga el catálogo una vez y confirma al final.
//!
//! Este módulo también aloja la verificación de [`Database::rollback`], que
//! aborta la transacción activa y descarta las páginas sucias del buffer pool.

use ruscadb_core::{Record, RecordId, RuscaError};

use crate::catalog::Catalog;
use crate::database::Database;
use crate::heap::heap_insert;
use crate::table_api::{maintain_index, validate_scalars};

impl Database {
    /// Inserta un lote de registros con un único commit.
    ///
    /// Carga el catálogo **una sola vez**, valida el esquema de cada registro
    /// (con coerción `Int→Float`) y, si todos son válidos, los persiste en el
    /// heap, mantiene por registro el índice primario, el índice secundario y
    /// los índices derivados (HNSW/CSR/invertido) y persiste el catálogo y
    /// confirma **una sola vez** (un frame WAL + un `fsync`). Por eso es más
    /// rápido que N llamadas a [`Database::insert`] (NF-0027-01) y, si algún
    /// registro viola el esquema, no escribe nada (validación previa).
    ///
    /// [`Database::insert`] y [`Database::insert_record`] siguen igual: delegan
    /// en la ruta de una fila (un commit por inserción).
    ///
    /// Args:
    ///     table: Tabla destino.
    ///     records: Registros completos (scalars + vector + edges) a insertar.
    ///
    /// Returns:
    ///     Los [`RecordId`] insertados, en el orden de entrada.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::ColumnNotFound`] si falta/sobra una columna escalar;
    ///     [`RuscaError::TypeMismatch`] si un escalar no pertenece a su columna;
    ///     [`RuscaError::DimensionMismatch`] si un vector rompe la dimensión
    ///     del índice de la tabla;
    ///     [`RuscaError::Io`] si falla la escritura o la confirmación.
    pub fn insert_many(
        &mut self,
        table: &str,
        records: impl IntoIterator<Item = Record>,
    ) -> Result<Vec<RecordId>, RuscaError> {
        let mut catalog = Catalog::load(self)?;
        let definition = catalog.get(table)?.clone();
        // Fase 1 (sin efectos): valida el esquema de TODOS los registros antes
        // de tocar el heap, de modo que un registro inválido no publique nada.
        let mut validated = Vec::new();
        for mut record in records {
            record.scalars = validate_scalars(&definition, table, record.scalars)?;
            validated.push(record);
        }
        if validated.is_empty() {
            return Ok(Vec::new());
        }
        // Fase 2: un solo pase de escritura + un solo commit al final.
        let (tx, is_auto_commit) = match self.active_tx {
            Some(tx) => (tx, false),
            None => (self.txn.begin(), true),
        };
        let mut ids = Vec::with_capacity(validated.len());
        for mut record in validated {
            record.meta.created_tx = tx;
            record.meta.lsn = self.wal.next_lsn();
            let id = record.id;
            let locator = heap_insert(self, &mut catalog, table, &record)?;
            self.primary_insert(table, id, locator)?;
            maintain_index(self, &mut catalog, table, &record, locator)?;
            self.indexes
                .entry(table.to_string())
                .or_default()
                .index_record(&record, &definition)?;
            ids.push(id);
        }
        if let Some(entry) = self.indexes.get_mut(table) {
            entry.finalize();
        }
        catalog.save(self)?;
        if is_auto_commit {
            self.txn.commit(tx)?;
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{ColumnDef, ColumnType};
    use crate::heap::heap_insert;
    use crate::{DbConfig, EdgeSet, EncryptionConfig, RecordMeta, ScalarMap, ScalarValue};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use ruscadb_wal::RecordKind;
    use std::path::Path;

    /// Abre una base temporal de pruebas con pool amplio (sin backpressure).
    fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join(format!("{tag}.db"));
        let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
        (dir, database)
    }

    /// Crea `t(a INT, b TEXT)`.
    fn create_table(database: &mut Database) {
        database
            .create_table(
                "t",
                vec![
                    ColumnDef {
                        name: "a".to_string(),
                        col_type: ColumnType::Int,
                    },
                    ColumnDef {
                        name: "b".to_string(),
                        col_type: ColumnType::Text,
                    },
                ],
            )
            .expect("create_table");
    }

    /// Construye un `Record` con escalares `a` y `b`.
    fn record(a: i64, b: &str) -> Record {
        let mut scalars = ScalarMap::new();
        scalars.insert("a".to_string(), ScalarValue::Int(a));
        scalars.insert("b".to_string(), ScalarValue::Text(b.to_string()));
        Record {
            id: RecordId::new(),
            scalars,
            doc: None,
            edges: EdgeSet::default(),
            vector: None,
            blob: None,
            meta: RecordMeta::default(),
        }
    }

    /// Cuenta los commit records del WAL (para verificar el número de commits).
    fn count_commit_records(wal_path: &Path) -> usize {
        ruscadb_wal::read_records_with_key(wal_path, None)
            .expect("lee WAL")
            .into_iter()
            .filter(|record| record.kind == RecordKind::Commit)
            .count()
    }

    /// AC-0027-01 — `insert_many` con N registros produce un solo commit.
    #[test]
    // @spec AC-0027-01
    fn test_ac_0027_01_insert_many_single_commit() {
        let (dir, mut database) = open_test_db("ac01");
        create_table(&mut database);
        let wal_path = dir.path().join("ac01.wal");
        let commits_before = count_commit_records(&wal_path);

        let records = vec![record(1, "x"), record(2, "x"), record(3, "y")];
        let ids = database.insert_many("t", records).expect("insert_many");
        assert_eq!(ids.len(), 3, "devuelve un id por registro");

        let commits_after = count_commit_records(&wal_path);
        assert_eq!(
            commits_after - commits_before,
            1,
            "el lote debe generar un único commit (frame WAL)"
        );

        let rows = database.execute("SELECT * FROM t").expect("select");
        assert_eq!(rows.len(), 3, "los N registros son visibles tras el commit");
        assert_eq!(rows[0].get("a"), Some(&ScalarValue::Int(1)));
        assert_eq!(rows[2].get("a"), Some(&ScalarValue::Int(3)));
    }

    /// AC-0027-02 — un registro inválido falla con error accionable y no
    /// publica a medias (tras rollback la tabla sigue vacía).
    #[test]
    // @spec AC-0027-02
    fn test_ac_0027_02_insert_many_error_is_actionable() {
        let (_dir, mut database) = open_test_db("ac02");
        create_table(&mut database);

        let mut bad = record(2, "ok");
        bad.scalars.insert("b".to_string(), ScalarValue::Int(7));
        let error = database
            .insert_many("t", vec![record(1, "ok"), bad])
            .expect_err("registro con esquema inválido");

        assert!(
            matches!(error, RuscaError::TypeMismatch { ref message } if message.contains('b')),
            "se esperaba un TypeMismatch accionable sobre 'b', se obtuvo {error:?}"
        );
        assert!(error.to_string().contains('b'));

        database.rollback().expect("rollback");
        assert_eq!(
            database.execute("SELECT * FROM t").expect("select").len(),
            0,
            "el lote fallido no debe publicar filas a medias"
        );
    }

    /// AC-0027-03 — `rollback` aborta la tx en vuelo y un snapshot posterior no
    /// ve los cambios.
    #[test]
    // @spec AC-0027-03
    fn test_ac_0027_03_rollback_aborts_tx() {
        let (_dir, mut database) = open_test_db("ac03");
        create_table(&mut database);

        let tx = database.begin();
        {
            let mut catalog = Catalog::load(&mut database).expect("catálogo");
            let uncommitted = Record {
                id: RecordId::new(),
                scalars: ScalarMap::from([
                    ("a".to_string(), ScalarValue::Int(99)),
                    (
                        "b".to_string(),
                        ScalarValue::Text("sin-confirmar".to_string()),
                    ),
                ]),
                doc: None,
                edges: EdgeSet::default(),
                vector: None,
                blob: None,
                meta: RecordMeta {
                    created_tx: tx,
                    deleted_tx: None,
                    lsn: database.wal.next_lsn(),
                    embedding_version: None,
                },
            };
            heap_insert(&mut database, &mut catalog, "t", &uncommitted).expect("heap_insert");
        }
        assert!(
            !database.pool.dirty_pages().is_empty(),
            "hay cambios sin confirmar en el pool"
        );

        database.rollback().expect("rollback");

        assert!(database.active_tx().is_none(), "la tx activa se limpia");
        assert!(
            !database.txn.is_committed(tx),
            "la tx abortada no debe quedar confirmada"
        );
        assert_eq!(
            database.execute("SELECT * FROM t").expect("select").len(),
            0,
            "un snapshot posterior no ve los cambios abortados"
        );

        // La base sigue operativa tras el rollback.
        let mut scalars = ScalarMap::new();
        scalars.insert("a".to_string(), ScalarValue::Int(1));
        scalars.insert("b".to_string(), ScalarValue::Text("vivo".to_string()));
        database.insert("t", scalars).expect("insert tras rollback");
        assert_eq!(
            database.execute("SELECT * FROM t").expect("select").len(),
            1
        );
    }

    /// AC-0027-04 — `rollback` descarta las páginas sucias y la página vuelve
    /// al último estado confirmado.
    #[test]
    // @spec AC-0027-04
    fn test_ac_0027_04_rollback_discards_dirty_pages() {
        let (_dir, mut database) = open_test_db("ac04");
        create_table(&mut database);
        let mut scalars = ScalarMap::new();
        scalars.insert("a".to_string(), ScalarValue::Int(1));
        scalars.insert("b".to_string(), ScalarValue::Text("confirmado".to_string()));
        database.insert("t", scalars).expect("insert confirmado");
        assert_eq!(
            database.execute("SELECT * FROM t").expect("select").len(),
            1
        );

        let heap_start = database
            .catalog()
            .expect("catálogo")
            .get("t")
            .expect("tabla")
            .heap_start;
        let baseline = database.read_page(heap_start).expect("baseline");

        database.begin();
        let mut modified = baseline.clone();
        modified.data_mut()[8] ^= 0xFF;
        database.write_page(&modified).expect("write sucio");
        assert_eq!(database.pool.is_dirty(heap_start), Some(true));

        database.rollback().expect("rollback");

        assert!(
            database.pool.dirty_pages().is_empty(),
            "no deben quedar páginas sucias tras el rollback"
        );
        let reverted = database.read_page(heap_start).expect("página revertida");
        assert_eq!(
            reverted.data(),
            baseline.data(),
            "la página vuelve al último estado confirmado"
        );
        assert_eq!(
            database.execute("SELECT * FROM t").expect("select").len(),
            1,
            "la fila confirmada sigue viva y la base operativa"
        );
    }

    /// AC-0027-05 — `rollback` en modo cifrado es rechazado con error accionable.
    #[test]
    // @spec AC-0027-05
    fn test_ac_0027_05_rollback_encrypted_is_error() {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join("vault.db");
        let mut config = DbConfig::new(&path, 64);
        config.encryption = Some(EncryptionConfig::new([0x2A; 32]));
        let mut database = Database::open(config).expect("apertura cifrada");
        create_table(&mut database);

        let error = database
            .rollback()
            .expect_err("rollback en modo cifrado debe fallar");
        assert!(
            matches!(error, RuscaError::InvalidConfig(ref message) if message.contains("cifrado")),
            "se esperaba un InvalidConfig accionable, se obtuvo {error:?}"
        );
        assert!(error.to_string().contains("rollback"));
    }

    proptest! {
        /// NF-0027-01 — `insert_many` de N registros equivale a N `insert`
        /// (mismo conjunto visible, incluido el lote vacío).
        #[test]
        fn prop_insert_many_equals_n_inserts(
            inputs in prop::collection::vec((prop::num::i64::ANY, "[a-z]{1,6}"), 0..20),
        ) {
            let (_dir_batch, mut batch_db) = open_test_db("prop_batch");
            create_table(&mut batch_db);
            let batch_records: Vec<Record> =
                inputs.iter().map(|(a, b)| record(*a, b)).collect();
            batch_db
                .insert_many("t", batch_records)
                .expect("insert_many");
            let batch_rows = batch_db.execute("SELECT * FROM t").expect("select batch");

            let (_dir_row, mut row_db) = open_test_db("prop_row");
            create_table(&mut row_db);
            for (a, b) in &inputs {
                let mut scalars = ScalarMap::new();
                scalars.insert("a".to_string(), ScalarValue::Int(*a));
                scalars.insert("b".to_string(), ScalarValue::Text(b.clone()));
                row_db.insert("t", scalars).expect("insert fila");
            }
            let row_rows = row_db.execute("SELECT * FROM t").expect("select fila");

            prop_assert_eq!(batch_rows.len(), inputs.len());
            prop_assert_eq!(batch_rows, row_rows);
        }
    }
}
