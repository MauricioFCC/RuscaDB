//! API relacional de la fachada (SPEC-0012, FR-0012-05/06).
//!
//! Extiende [`Database`] con `create_table`, `insert` (con **auto-commit**
//! documentado: cada inserción valida el esquema, mantiene el índice,
//! persiste el catálogo y confirma vía WAL-first), `create_index` y
//! `execute` (texto RQL → filas). El catálogo se carga de forma diferida
//! desde la página 0 en cada operación: reabrir la base recupera tablas,
//! filas e índices sin estado en memoria.

use ruscadb_core::{EdgeSet, Record, RecordId, RecordMeta, RuscaError, ScalarMap, ScalarValue};
use ruscadb_query::parse;
use ruscadb_txn::Snapshot;

use crate::catalog::{Catalog, ColumnDef, ColumnType, TableDef};
use crate::database::Database;
use crate::executor::{Row, execute_select_at};
use crate::heap::{blank_slotted_page, heap_insert};
use crate::index::index_insert;
use crate::indexes::TableIndexes;

impl Database {
    /// Crea una tabla con su esquema y reserva su primera página de heap.
    ///
    /// Confirma la operación (WAL-first) antes de retornar.
    ///
    /// Args:
    ///     name: Nombre de la tabla (no vacío, único).
    ///     columns: Esquema (nombres únicos y no vacíos).
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el nombre/columnas son inválidos
    ///     o la tabla ya existe.
    pub fn create_table(&mut self, name: &str, columns: Vec<ColumnDef>) -> Result<(), RuscaError> {
        validate_schema(name, &columns)?;
        let mut catalog = Catalog::load(self)?;
        catalog.register_table(name, columns)?;
        let heap_start = catalog.get(name)?.heap_start;
        self.write_page(&blank_slotted_page(heap_start))?;
        catalog.save(self)?;
        self.indexes
            .insert(name.to_string(), TableIndexes::default());
        Ok(())
    }

    /// Inserta una fila con solo escalares (azúcar sobre [`Database::insert_record`]).
    ///
    /// Crea un [`Record`] nuevo (id ULID, sin vector/aristas) y delega: valida
    /// el esquema (con coerción `Int→Float`), persiste en el heap, mantiene el
    /// índice secundario y los índices derivados, y confirma (WAL-first).
    ///
    /// Args:
    ///     table: Tabla destino.
    ///     scalars: Valores por columna.
    ///
    /// Returns:
    ///     El [`RecordId`] generado para la fila.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::ColumnNotFound`] si falta/sobra una columna;
    ///     [`RuscaError::TypeMismatch`] si un valor no pertenece a su columna.
    pub fn insert(&mut self, table: &str, scalars: ScalarMap) -> Result<RecordId, RuscaError> {
        let record = Record {
            id: RecordId::new(),
            scalars,
            doc: None,
            edges: EdgeSet::default(),
            vector: None,
            blob: None,
            meta: RecordMeta::default(),
        };
        self.insert_record(table, record)
    }

    /// Inserta un [`Record`] completo y alimenta los índices derivados.
    ///
    /// Persiste en el heap los escalares, el embedding (`vector`) y las aristas
    /// (`edges`) **tal como vienen**, conservando el `record.id` (necesario
    /// para que `edges.out` referencie a otros registros). Además de mantener
    /// el índice secundario de escalares (SPEC-0012), actualiza los índices en
    /// memoria de la tabla: HNSW (vectores), CSR (aristas salientes) e inverso
    /// (columnas `TEXT`). Confirma con auto-commit WAL-first.
    ///
    /// MVCC (SPEC-0019): estampa `meta.created_tx` con la transacción activa
    /// ([`Database::begin`]) o con una transacción de auto-commit que se publica
    /// tras el commit; además estampa `meta.lsn` con el próximo LSN del WAL.
    ///
    /// Args:
    ///     table: Tabla destino.
    ///     record: Registro completo (scalars + vector + edges).
    ///
    /// Returns:
    ///     El [`RecordId`] del registro insertado (el mismo de `record`).
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::ColumnNotFound`] si falta/sobra una columna escalar;
    ///     [`RuscaError::TypeMismatch`] si un escalar no pertenece a su columna;
    ///     [`RuscaError::DimensionMismatch`] si un vector rompe la dimensión
    ///     del índice de la tabla.
    pub fn insert_record(
        &mut self,
        table: &str,
        mut record: Record,
    ) -> Result<RecordId, RuscaError> {
        let (tx, is_auto_commit) = match self.active_tx {
            Some(tx) => (tx, false),
            None => (self.txn.begin(), true),
        };
        record.meta.created_tx = tx;
        record.meta.lsn = self.wal.next_lsn();
        let mut catalog = Catalog::load(self)?;
        let definition = catalog.get(table)?.clone();
        record.scalars = validate_scalars(&definition, table, record.scalars)?;
        let locator = heap_insert(self, &mut catalog, table, &record)?;
        self.primary_insert(table, record.id, locator)?;
        maintain_index(self, &mut catalog, table, &record, locator)?;
        let entry = self.indexes.entry(table.to_string()).or_default();
        entry.index_record(&record, &definition)?;
        entry.finalize();
        catalog.save(self)?;
        if is_auto_commit {
            self.txn.commit(tx)?;
        }
        Ok(record.id)
    }

    /// Crea el índice secundario de una columna y lo construye desde el heap.
    ///
    /// Confirma la operación (WAL-first) antes de retornar.
    ///
    /// Args:
    ///     table: Tabla destino.
    ///     column: Columna a indexar (una por tabla).
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::ColumnNotFound`] si la columna no existe;
    ///     [`RuscaError::InvalidConfig`] si ya hay un índice.
    pub fn create_index(&mut self, table: &str, column: &str) -> Result<(), RuscaError> {
        let mut catalog = Catalog::load(self)?;
        {
            let definition = catalog.get(table)?;
            definition.column_type(column)?;
            if definition.index.is_some() {
                return Err(RuscaError::InvalidConfig(format!(
                    "la tabla '{table}' ya tiene un índice (solo se admite una columna indexada)"
                )));
            }
        }
        let indexed = crate::catalog::IndexDef {
            column: column.to_string(),
            pages: Vec::new(),
        };
        catalog.get_mut(table)?.index = Some(indexed);
        crate::index::index_build(self, &mut catalog, table)?;
        catalog.save(self)
    }

    /// Ejecuta una consulta RQL y devuelve las filas proyectadas.
    ///
    /// Usa el snapshot más reciente (todos los commits visibles).
    ///
    /// Args:
    ///     query: Texto RQL (`SELECT ... FROM ... WHERE ... LIMIT ...`).
    ///
    /// Returns:
    ///     Filas como mapas columna → escalar.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si el texto no parsea;
    ///     errores de esquema del ejecutor en otro caso.
    pub fn execute(&mut self, query: &str) -> Result<Vec<Row>, RuscaError> {
        let snapshot = self.snapshot();
        self.execute_at(query, &snapshot)
    }

    /// Ejecuta una consulta RQL con visibilidad "as of" un snapshot MVCC.
    ///
    /// Args:
    ///     query: Texto RQL (`SELECT ... FROM ... WHERE ... LIMIT ...`).
    ///     snapshot: Vista fija de visibilidad ([`Database::snapshot`]).
    ///
    /// Returns:
    ///     Filas visibles para `snapshot`, como mapas columna → escalar.
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si el texto no parsea;
    ///     errores de esquema del ejecutor en otro caso.
    pub fn execute_at(&mut self, query: &str, snapshot: &Snapshot) -> Result<Vec<Row>, RuscaError> {
        let select = parse(query)?;
        execute_select_at(self, &select, snapshot)
    }

    /// Carga el catálogo actual (instantánea para planificar e inspeccionar).
    ///
    /// Returns:
    ///     El catálogo persistido (vacío si la base es nueva).
    pub fn catalog(&mut self) -> Result<Catalog, RuscaError> {
        Catalog::load(self)
    }
}

/// Valida el nombre y las columnas de una tabla nueva.
fn validate_schema(name: &str, columns: &[ColumnDef]) -> Result<(), RuscaError> {
    if name.is_empty() {
        return Err(RuscaError::InvalidConfig(
            "el nombre de la tabla no puede estar vacío".to_string(),
        ));
    }
    if columns.is_empty() {
        return Err(RuscaError::InvalidConfig(format!(
            "la tabla '{name}' debe tener al menos una columna"
        )));
    }
    let mut seen: Vec<&str> = Vec::with_capacity(columns.len());
    for column in columns {
        if column.name.is_empty() {
            return Err(RuscaError::InvalidConfig(format!(
                "la tabla '{name}' tiene una columna sin nombre"
            )));
        }
        if seen.contains(&column.name.as_str()) {
            return Err(RuscaError::InvalidConfig(format!(
                "la columna '{}' está duplicada en la tabla '{name}'",
                column.name
            )));
        }
        seen.push(&column.name);
    }
    Ok(())
}

/// Valida los escalares contra el esquema (con coerción `Int→Float`).
pub(crate) fn validate_scalars(
    definition: &TableDef,
    table: &str,
    scalars: ScalarMap,
) -> Result<ScalarMap, RuscaError> {
    for key in scalars.keys() {
        if definition.column_type(key).is_err() {
            return Err(RuscaError::ColumnNotFound {
                column: key.clone(),
            });
        }
    }
    let mut validated = ScalarMap::new();
    for column in &definition.columns {
        let value = scalars
            .get(&column.name)
            .ok_or_else(|| RuscaError::ColumnNotFound {
                column: column.name.clone(),
            })?;
        validated.insert(
            column.name.clone(),
            coerce_value(table, &column.name, column.col_type, value)?,
        );
    }
    Ok(validated)
}

/// Ajusta un valor al tipo de su columna o falla con `TypeMismatch`.
fn coerce_value(
    table: &str,
    column: &str,
    expected: ColumnType,
    value: &ScalarValue,
) -> Result<ScalarValue, RuscaError> {
    match (expected, value) {
        (_, ScalarValue::Null) => Ok(ScalarValue::Null),
        (ColumnType::Bool, ScalarValue::Bool(_))
        | (ColumnType::Int, ScalarValue::Int(_))
        | (ColumnType::Float, ScalarValue::Float(_))
        | (ColumnType::Text, ScalarValue::Text(_)) => Ok(value.clone()),
        (ColumnType::Float, ScalarValue::Int(number)) => Ok(ScalarValue::Float(*number as f64)),
        _ => Err(RuscaError::TypeMismatch {
            message: format!(
                "la columna '{column}' de '{table}' es {expected:?} pero recibió {value:?} (ajusta el valor al esquema)"
            ),
        }),
    }
}

/// Actualiza el índice de la tabla con la fila recién insertada.
pub(crate) fn maintain_index(
    database: &mut Database,
    catalog: &mut Catalog,
    table: &str,
    record: &Record,
    locator: crate::heap::RowLocator,
) -> Result<(), RuscaError> {
    let indexed = catalog.get(table)?.index.clone();
    let Some(definition) = indexed else {
        return Ok(());
    };
    let value = record
        .scalars
        .get(&definition.column)
        .cloned()
        .unwrap_or(ScalarValue::Null);
    index_insert(database, catalog, table, &value, locator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::{Plan, execute_with_plan, plan_for};
    use crate::{ColumnType, ScalarValue};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use ruscadb_query::Projection;

    /// Abre una base temporal de pruebas con pool amplio (sin backpressure).
    fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join(format!("{tag}.db"));
        let database =
            Database::open(crate::DbConfig::new(&path, 64)).expect("apertura de la base");
        (dir, database)
    }

    /// Crea `t(a INT, b TEXT)` con tres filas de ejemplo.
    fn seed_table(database: &mut Database) {
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
        for (value, label) in [(1_i64, "x"), (2, "x"), (3, "y")] {
            let mut scalars = ScalarMap::new();
            scalars.insert("a".to_string(), ScalarValue::Int(value));
            scalars.insert("b".to_string(), ScalarValue::Text(label.to_string()));
            database.insert("t", scalars).expect("insert");
        }
    }

    /// AC-0012-01 — crear → insertar 3 filas → `SELECT *` las devuelve.
    #[test]
    fn test_ac_0012_01_create_insert_select_all() {
        let (_dir, mut database) = open_test_db("ac01");
        seed_table(&mut database);

        let rows = database.execute("SELECT * FROM t").expect("select");
        assert_eq!(rows.len(), 3);
        let first = rows[0].get("a").expect("columna a");
        assert_eq!(first, &ScalarValue::Int(1));
        assert_eq!(
            rows[0].get("b").expect("columna b"),
            &ScalarValue::Text("x".to_string())
        );
        assert_eq!(rows[2].get("a").expect("columna a"), &ScalarValue::Int(3));
    }

    /// AC-0012-02 — filtro con `AND`, proyección y `LIMIT`.
    #[test]
    fn test_ac_0012_02_where_projection_limit() {
        let (_dir, mut database) = open_test_db("ac02");
        seed_table(&mut database);

        let rows = database
            .execute("SELECT a FROM t WHERE b = 'x' AND a > 1 LIMIT 1")
            .expect("select");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 1);
        assert_eq!(rows[0].get("a").expect("columna a"), &ScalarValue::Int(2));
    }

    /// AC-0012-03 — el plan usa el índice y coincide con el full scan.
    #[test]
    fn test_ac_0012_03_index_scan_matches_full_scan() {
        let (_dir, mut database) = open_test_db("ac03");
        seed_table(&mut database);
        database.create_index("t", "a").expect("create_index");

        let select = parse("SELECT * FROM t WHERE a = 2").expect("parse");
        let catalog = database.catalog().expect("catálogo");
        let plan = plan_for(&select, &catalog).expect("plan");
        assert_eq!(
            plan,
            Plan::IndexScan {
                column: "a".to_string()
            }
        );

        let via_index = execute_with_plan(&mut database, &select, plan).expect("index scan");
        let via_full =
            execute_with_plan(&mut database, &select, Plan::FullScan).expect("full scan");
        assert_eq!(via_index, via_full);
        assert_eq!(via_index.len(), 1);
        assert_eq!(
            via_index[0].get("a").expect("columna a"),
            &ScalarValue::Int(2)
        );
    }

    /// AC-0012-04 — catálogo, filas e índice sobreviven a cerrar y reabrir.
    #[test]
    fn test_ac_0012_04_catalog_survives_reopen() {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join("reopen.db");
        {
            let mut database = Database::open(crate::DbConfig::new(&path, 64)).expect("apertura");
            seed_table(&mut database);
            database.create_index("t", "a").expect("create_index");
            database.close().expect("cierre");
        }
        let mut database = Database::open(crate::DbConfig::new(&path, 64)).expect("reapertura");
        let rows = database.execute("SELECT * FROM t").expect("select");
        assert_eq!(rows.len(), 3);
        let filtered = database
            .execute("SELECT a FROM t WHERE a = 3")
            .expect("select con índice");
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].get("a").expect("columna a"),
            &ScalarValue::Int(3)
        );
    }

    /// AC-0012-05 — tabla inexistente y tipo incompatible dan errores accionables.
    #[test]
    fn test_ac_0012_05_schema_errors_are_actionable() {
        let (_dir, mut database) = open_test_db("ac05");
        seed_table(&mut database);

        let missing = database
            .execute("SELECT * FROM ausente")
            .expect_err("tabla ausente");
        assert!(
            matches!(missing, RuscaError::TableNotFound { ref table } if table == "ausente"),
            "se esperaba TableNotFound, se obtuvo {missing:?}"
        );
        assert!(missing.to_string().contains("ausente"));

        let mut bad = ScalarMap::new();
        bad.insert("a".to_string(), ScalarValue::Text("no-es-int".to_string()));
        bad.insert("b".to_string(), ScalarValue::Text("x".to_string()));
        let mismatch = database.insert("t", bad).expect_err("tipo incompatible");
        assert!(
            matches!(mismatch, RuscaError::TypeMismatch { .. }),
            "se esperaba TypeMismatch, se obtuvo {mismatch:?}"
        );
        assert!(mismatch.to_string().contains('a'));

        let unknown = database
            .execute("SELECT inexistente FROM t")
            .expect_err("columna ausente");
        assert!(
            matches!(unknown, RuscaError::ColumnNotFound { ref column } if column == "inexistente"),
            "se esperaba ColumnNotFound, se obtuvo {unknown:?}"
        );
    }

    proptest! {
        /// Roundtrip: insertar N filas y `SELECT *` devuelve el mismo
        /// número de filas con el mismo contenido en orden.
        #[test]
        fn prop_insert_select_roundtrip(
            inputs in prop::collection::vec((prop::num::i64::ANY, "[a-z]{1,6}"), 1..30),
        ) {
            let (_dir, mut database) = open_test_db("proptest");
            database
                .create_table(
                    "t",
                    vec![
                        ColumnDef { name: "a".to_string(), col_type: ColumnType::Int },
                        ColumnDef { name: "b".to_string(), col_type: ColumnType::Text },
                    ],
                )
                .expect("create_table");
            let mut expected: Vec<Row> = Vec::new();
            for (value, label) in &inputs {
                let mut scalars = ScalarMap::new();
                scalars.insert("a".to_string(), ScalarValue::Int(*value));
                scalars.insert("b".to_string(), ScalarValue::Text(label.clone()));
                database.insert("t", scalars).expect("insert");
                let mut row = Row::new();
                row.insert("a".to_string(), ScalarValue::Int(*value));
                row.insert("b".to_string(), ScalarValue::Text(label.clone()));
                expected.push(row);
            }
            let rows = database.execute("SELECT * FROM t").expect("select");
            prop_assert_eq!(rows.len(), expected.len());
            prop_assert_eq!(rows, expected);
        }
    }

    /// La proyección respeta el orden pedido y `*` trae todas las columnas.
    #[test]
    fn test_projection_star_returns_all_columns() {
        let (_dir, mut database) = open_test_db("proj");
        seed_table(&mut database);
        let rows = database.execute("SELECT b FROM t LIMIT 2").expect("select");
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row.len(), 1);
            assert!(row.contains_key("b"));
        }
        let parsed = parse("SELECT * FROM t").expect("parse");
        assert_eq!(parsed.projection, Projection::All);
    }
}
