//! API relacional de la fachada (SPEC-0012, FR-0012-05/06).
//!
//! Extiende [`Database`] con `create_table`, `insert` (con **auto-commit**
//! documentado: cada inserción valida el esquema, mantiene el índice,
//! persiste el catálogo y confirma vía WAL-first), `create_index` y
//! `execute` (texto RQL → filas). El catálogo se carga de forma diferida
//! desde la página 0 en cada operación: reabrir la base recupera tablas,
//! filas e índices sin estado en memoria.

use ruscadb_core::{
    EdgeSet, Metric, Record, RecordId, RecordMeta, RuscaError, ScalarMap, ScalarValue,
};
use ruscadb_query::{Statement, parse_statement};
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

    /// Registra (o actualiza) un modelo de embedding permitido (SPEC-0032).
    ///
    /// Activa la allowlist de la base: desde este punto `insert_record` valida
    /// el `vector` de cada [`Record`] contra el modelo (`model_id`, `dim`,
    /// `metric`; invariante SI-2) y fija `meta.embedding_version` con `version`.
    /// Con el registro vacío no hay validación (compatibilidad, NF-0032-01).
    ///
    /// El registro es **en memoria**: no se persiste y se pierde al reabrir.
    ///
    /// Args:
    ///     model_id: Identificador del modelo (no vacío).
    ///     dim: Dimensión esperada del vector (`>= 1`).
    ///     metric: Métrica de distancia esperada.
    ///     version: Versión del modelo (ADR-009).
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si `model_id` está vacío o `dim == 0`.
    pub fn register_model(
        &mut self,
        model_id: &str,
        dim: usize,
        metric: Metric,
        version: u32,
    ) -> Result<(), RuscaError> {
        self.registry.register(model_id, dim, metric, version)
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
    /// Allowlist de modelos (SPEC-0032, invariante SI-2): si el registro
    /// (`vector`) trae metadata de modelo y la allowlist está activa
    /// ([`Database::register_model`]), se valida `model_id`/`dim`/`metric` y se
    /// fija `meta.embedding_version` con la versión registrada. Sin modelos
    /// registrados el comportamiento previo se conserva (NF-0032-01).
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
    ///     [`RuscaError::InvalidConfig`] si el vector no cumple la allowlist
    ///     (modelo no registrado o `dim`/`metric` incompatibles);
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
        apply_registry(self, &mut record)?;
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

    /// Ejecuta una sentencia RQL y devuelve las filas resultantes.
    ///
    /// Usa el snapshot más reciente (todos los commits visibles) para `SELECT`,
    /// `UPDATE` y `DELETE`. Las sentencias DML (SPEC-0043) devuelven una fila
    /// `{"affected": N}` con el número de filas insertadas/actualizadas/borradas.
    ///
    /// Args:
    ///     query: Texto RQL (`SELECT`, `INSERT`, `UPDATE` o `DELETE`).
    ///
    /// Returns:
    ///     Filas como mapas columna → escalar (`{"affected": N}` para DML).
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si el texto no parsea;
    ///     errores de esquema del ejecutor en otro caso.
    pub fn execute(&mut self, query: &str) -> Result<Vec<Row>, RuscaError> {
        let snapshot = self.snapshot();
        self.execute_at(query, &snapshot)
    }

    /// Ejecuta una sentencia RQL con visibilidad "as of" un snapshot MVCC.
    ///
    /// Despacha `SELECT`/`INSERT`/`UPDATE`/`DELETE` (SPEC-0043). `EXPLAIN` no es
    /// una sentencia ejecutable y devuelve [`RuscaError::ParseError`] (se
    /// inspecciona con `parse_statement`).
    ///
    /// Args:
    ///     query: Texto RQL.
    ///     snapshot: Vista fija de visibilidad ([`Database::snapshot`]).
    ///
    /// Returns:
    ///     Filas visibles para `snapshot` (o `{"affected": N}` para DML).
    ///
    /// Errors:
    ///     [`RuscaError::ParseError`] si el texto no parsea;
    ///     errores de esquema del ejecutor en otro caso.
    pub fn execute_at(&mut self, query: &str, snapshot: &Snapshot) -> Result<Vec<Row>, RuscaError> {
        match parse_statement(query)? {
            Statement::Select(select) => execute_select_at(self, &select, snapshot),
            Statement::Insert(insert) => self.execute_insert(&insert),
            Statement::Update(update) => self.execute_update(&update, snapshot),
            Statement::Delete(delete) => self.execute_delete(&delete, snapshot),
            Statement::Explain(_) => Err(RuscaError::ParseError {
                message:
                    "EXPLAIN no es una sentencia ejecutable; inspecciónala con parse_statement"
                        .to_string(),
                position: 0,
            }),
        }
    }

    /// Carga el catálogo actual (instantánea para planificar e inspeccionar).
    ///
    /// Returns:
    ///     El catálogo persistido (vacío si la base es nueva).
    pub fn catalog(&mut self) -> Result<Catalog, RuscaError> {
        Catalog::load(self)
    }
}

/// Aplica la allowlist de modelos: valida el vector y fija su versión (SI-2).
///
/// No-op si el registro no trae `vector` o si la allowlist está vacía.
///
/// Args:
///     database: Base cuyo registro de modelos se consulta.
///     record: Registro a validar y enriquecer in situ.
///
/// Errors:
///     [`RuscaError::InvalidConfig`] si el vector no cumple la allowlist.
fn apply_registry(database: &Database, record: &mut Record) -> Result<(), RuscaError> {
    let Some(vector) = record.vector.as_ref() else {
        return Ok(());
    };
    database.registry.validate(vector)?;
    if let Some(version) = database.registry.version_of(&vector.meta.model_id) {
        record.meta.embedding_version = Some(version);
    }
    Ok(())
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
    use crate::{ColumnType, Embedding, EmbeddingMeta, ScalarValue};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use ruscadb_query::{Projection, parse};

    /// Abre una base temporal de pruebas con pool amplio (sin backpressure).
    fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join(format!("{tag}.db"));
        let database =
            Database::open(crate::DbConfig::new(&path, 64)).expect("apertura de la base");
        (dir, database)
    }

    /// Crea la tabla `t(a INT, b TEXT)` vacía.
    fn create_ab_table(database: &mut Database) {
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

    /// Crea `t(a INT, b TEXT)` con tres filas de ejemplo.
    fn seed_table(database: &mut Database) {
        create_ab_table(database);
        for (value, label) in [(1_i64, "x"), (2, "x"), (3, "y")] {
            let mut scalars = ScalarMap::new();
            scalars.insert("a".to_string(), ScalarValue::Int(value));
            scalars.insert("b".to_string(), ScalarValue::Text(label.to_string()));
            database.insert("t", scalars).expect("insert");
        }
    }

    /// Inserta en `t` los pares `(a, b)` dados (`a` admite `None` = `NULL`).
    fn seed_pairs(database: &mut Database, pairs: &[(Option<i64>, &str)]) {
        for (value, label) in pairs {
            let scalar = value.map_or(ScalarValue::Null, ScalarValue::Int);
            let mut scalars = ScalarMap::new();
            scalars.insert("a".to_string(), scalar);
            scalars.insert("b".to_string(), ScalarValue::Text((*label).to_string()));
            database.insert("t", scalars).expect("insert");
        }
    }

    /// Extrae la columna `a` como enteros, tratando `NULL`/ausente como `None`.
    fn column_a(rows: &[Row]) -> Vec<Option<i64>> {
        rows.iter()
            .map(|row| match row.get("a") {
                Some(ScalarValue::Null) | None => None,
                Some(ScalarValue::Int(value)) => Some(*value),
                other => panic!("se esperaba Int/NULL en 'a', se obtuvo {other:?}"),
            })
            .collect()
    }

    /// Extrae la columna `b` como textos.
    fn column_b(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row.get("b") {
                Some(ScalarValue::Text(value)) => value.clone(),
                other => panic!("se esperaba Text en 'b', se obtuvo {other:?}"),
            })
            .collect()
    }

    /// AC-0012-01 — crear → insertar 3 filas → `SELECT *` las devuelve.
    #[test]
    // @spec AC-0012-01
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
    // @spec AC-0012-02
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
    // @spec AC-0012-03
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
    // @spec AC-0012-04
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
    // @spec AC-0012-05
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

    /// Construye un registro con el escalar `a="x"` y un vector de modelo dado.
    ///
    /// Args:
    ///     model_id: Identificador del modelo del vector.
    ///     values: Valores del vector.
    ///     metric: Métrica declarada.
    ///
    /// Returns:
    ///     Un [`Record`] listo para `insert_record`.
    fn record_with_vector(model_id: &str, values: Vec<f32>, metric: Metric) -> Record {
        let mut scalars = ScalarMap::new();
        scalars.insert("a".to_string(), ScalarValue::Text("x".to_string()));
        let meta = EmbeddingMeta {
            model_id: model_id.to_string(),
            dim: values.len(),
            metric,
        };
        Record {
            id: RecordId::new(),
            scalars,
            doc: None,
            edges: EdgeSet::default(),
            vector: Some(Embedding::new(values, meta).expect("embedding válido")),
            blob: None,
            meta: RecordMeta::default(),
        }
    }

    /// AC-0032-05 — la fachada aplica la allowlist en `insert_record`.
    #[test] // @spec AC-0032-05
    fn test_ac_0032_05_database_enforces_registry() {
        let (_dir, mut database) = open_test_db("ac0032_05");
        database
            .create_table(
                "t",
                vec![ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Text,
                }],
            )
            .expect("create_table");
        database
            .register_model("m1", 3, Metric::Cosine, 7)
            .expect("register_model");

        let unknown = record_with_vector("m2", vec![0.1, 0.2, 0.3], Metric::Cosine);
        let error = database
            .insert_record("t", unknown)
            .expect_err("modelo ajeno debe rechazarse");
        assert!(matches!(error, RuscaError::InvalidConfig(_)));
        assert!(error.to_string().contains("m2"));
        assert_eq!(
            database.primary_index_len("t"),
            0,
            "un rechazo no debe escribir"
        );

        let bad_dim = record_with_vector("m1", vec![0.1, 0.2], Metric::Cosine);
        let dim_error = database
            .insert_record("t", bad_dim)
            .expect_err("dimensión incompatible debe rechazarse");
        assert!(matches!(dim_error, RuscaError::InvalidConfig(_)));
        assert_eq!(database.primary_index_len("t"), 0);

        let good = record_with_vector("m1", vec![0.1, 0.2, 0.3], Metric::Cosine);
        let id = database.insert_record("t", good).expect("modelo válido");
        let stored = database
            .get_record("t", &id)
            .expect("get_record")
            .expect("el registro existe");
        assert_eq!(stored.meta.embedding_version, Some(7));
    }

    proptest! {
        /// PBT de compatibilidad (NF-0032-01): con registro vacío, `insert_record`
        /// acepta cualquier vector y deja `embedding_version` sin fijar.
        #[test]
        fn prop_empty_registry_insert_accepts_any_vector(
            model_id in "[a-zA-Z0-9_-]{0,12}",
            values in prop::collection::vec(-100.0f32..100.0f32, 1..12),
            metric in prop::sample::select(vec![
                Metric::L2,
                Metric::Cosine,
                Metric::InnerProduct,
            ]),
        ) {
            let (_dir, mut database) = open_test_db("prop_registry");
            database
                .create_table(
                    "t",
                    vec![ColumnDef { name: "a".to_string(), col_type: ColumnType::Text }],
                )
                .expect("create_table");
            let record = record_with_vector(&model_id, values, metric);
            let id = database
                .insert_record("t", record)
                .expect("registro vacío debe aceptar");
            let stored = database.get_record("t", &id).expect("get_record").expect("existe");
            prop_assert_eq!(stored.meta.embedding_version, None);
        }
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

    /// AC-0036-02 — `ORDER BY a ASC` devuelve las filas en orden ascendente.
    #[test] // @spec AC-0036-02
    fn test_ac_0036_02_execute_order_asc() {
        let (_dir, mut database) = open_test_db("ac0036_02");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[(Some(3), "c"), (Some(1), "a"), (Some(2), "b")],
        );

        let rows = database
            .execute("SELECT a, b FROM t ORDER BY a ASC")
            .expect("select");
        assert_eq!(column_a(&rows), vec![Some(1), Some(2), Some(3)]);
        assert_eq!(column_b(&rows), vec!["a", "b", "c"]);
    }

    /// AC-0036-03 — `ORDER BY a DESC LIMIT 2` devuelve las 2 mayores (desc.).
    #[test] // @spec AC-0036-03
    fn test_ac_0036_03_execute_order_desc_limit() {
        let (_dir, mut database) = open_test_db("ac0036_03");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[(Some(1), "a"), (Some(3), "c"), (Some(2), "b")],
        );

        let rows = database
            .execute("SELECT a FROM t ORDER BY a DESC LIMIT 2")
            .expect("select");
        assert_eq!(column_a(&rows), vec![Some(3), Some(2)]);
    }

    /// AC-0036-04 — los `NULL` van al final en `ASC` y al principio en `DESC`.
    #[test] // @spec AC-0036-04
    fn test_ac_0036_04_nulls_last_asc() {
        let (_dir, mut database) = open_test_db("ac0036_04");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[
                (Some(2), "b"),
                (None, "n1"),
                (Some(1), "a"),
                (None, "n2"),
                (Some(3), "c"),
            ],
        );

        let ascending = database
            .execute("SELECT a FROM t ORDER BY a ASC")
            .expect("asc");
        assert_eq!(
            column_a(&ascending),
            vec![Some(1), Some(2), Some(3), None, None]
        );

        let descending = database
            .execute("SELECT a FROM t ORDER BY a DESC")
            .expect("desc");
        assert_eq!(
            column_a(&descending),
            vec![None, None, Some(3), Some(2), Some(1)]
        );
    }

    /// AC-0036-05 — `ORDER BY` sobre columna inexistente da `ColumnNotFound`.
    #[test] // @spec AC-0036-05
    fn test_ac_0036_05_order_by_unknown_column() {
        let (_dir, mut database) = open_test_db("ac0036_05");
        seed_table(&mut database);

        let error = database
            .execute("SELECT * FROM t ORDER BY ausente")
            .expect_err("columna ausente");
        assert!(
            matches!(error, RuscaError::ColumnNotFound { ref column } if column == "ausente"),
            "se esperaba ColumnNotFound, se obtuvo {error:?}"
        );
        assert!(error.to_string().contains("ausente"));
    }

    /// BVA — ordenar por una columna no proyectada y con valores duplicados
    /// (el orden es estable: empates conservan el orden de inserción).
    #[test]
    fn test_ac_0036_bva_unprojected_and_duplicates() {
        let (_dir, mut database) = open_test_db("ac0036_bva");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[
                (Some(2), "p"),
                (Some(1), "q"),
                (Some(2), "r"),
                (Some(1), "s"),
            ],
        );

        let rows = database
            .execute("SELECT b FROM t ORDER BY a ASC")
            .expect("select");
        assert_eq!(column_b(&rows), vec!["q", "s", "p", "r"]);
    }

    /// Localiza la fila del grupo etiquetado con `label` en la columna `b`.
    ///
    /// Args:
    ///     rows: Filas agregadas.
    ///     label: Etiqueta del grupo.
    ///
    /// Returns:
    ///     La fila del grupo.
    ///
    /// Raises:
    ///     Panic si no existe el grupo (fallo de test).
    fn group_row<'a>(rows: &'a [Row], label: &str) -> &'a Row {
        rows.iter()
            .find(|row| row.get("b") == Some(&ScalarValue::Text(label.to_string())))
            .unwrap_or_else(|| panic!("no hay grupo '{label}' en {rows:?}"))
    }

    /// Devuelve `COUNT(*)` del grupo etiquetado con `label` en `b`.
    ///
    /// Args:
    ///     rows: Filas agregadas.
    ///     label: Etiqueta del grupo.
    ///
    /// Returns:
    ///     El conteo del grupo, o `None` si no existe.
    fn group_count(rows: &[Row], label: &str) -> Option<i64> {
        rows.iter()
            .find(|row| row.get("b") == Some(&ScalarValue::Text(label.to_string())))
            .and_then(|row| match row.get("count") {
                Some(ScalarValue::Int(value)) => Some(*value),
                _ => None,
            })
    }

    /// AC-0040-02 — `COUNT(*)` agrupado cuenta las filas de cada grupo.
    #[test] // @spec AC-0040-02
    fn test_ac_0040_02_count_groups() {
        let (_dir, mut database) = open_test_db("ac0040_02");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[
                (Some(1), "x"),
                (Some(2), "x"),
                (Some(3), "y"),
                (Some(4), "z"),
            ],
        );

        let rows = database
            .execute("SELECT b, COUNT(*) FROM t GROUP BY b")
            .expect("select");
        assert_eq!(rows.len(), 3);
        assert_eq!(group_count(&rows, "x"), Some(2));
        assert_eq!(group_count(&rows, "y"), Some(1));
        assert_eq!(group_count(&rows, "z"), Some(1));
    }

    /// AC-0040-03 — `SUM`/`AVG`/`COUNT(col)` ignoran los `NULL`.
    #[test] // @spec AC-0040-03
    fn test_ac_0040_03_sum_avg_ignore_nulls() {
        let (_dir, mut database) = open_test_db("ac0040_03");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[
                (Some(1), "g"),
                (Some(2), "g"),
                (None, "g"),
                (Some(10), "h"),
                (None, "h"),
            ],
        );

        let rows = database
            .execute("SELECT b, SUM(a), AVG(a), COUNT(a) FROM t GROUP BY b")
            .expect("select");
        let group_g = group_row(&rows, "g");
        assert_eq!(group_g.get("sum_a"), Some(&ScalarValue::Int(3)));
        assert_eq!(group_g.get("avg_a"), Some(&ScalarValue::Float(1.5)));
        assert_eq!(group_g.get("count_a"), Some(&ScalarValue::Int(2)));

        let group_h = group_row(&rows, "h");
        assert_eq!(group_h.get("sum_a"), Some(&ScalarValue::Int(10)));
        assert_eq!(group_h.get("avg_a"), Some(&ScalarValue::Float(10.0)));
        assert_eq!(group_h.get("count_a"), Some(&ScalarValue::Int(1)));
    }

    /// AC-0040-04 — `MIN`/`MAX` devuelven el mínimo y máximo por grupo.
    #[test] // @spec AC-0040-04
    fn test_ac_0040_04_min_max() {
        let (_dir, mut database) = open_test_db("ac0040_04");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[
                (Some(3), "g"),
                (None, "g"),
                (Some(1), "g"),
                (Some(2), "g"),
                (Some(5), "h"),
            ],
        );

        let rows = database
            .execute("SELECT b, MIN(a), MAX(a) FROM t GROUP BY b")
            .expect("select");
        let group_g = group_row(&rows, "g");
        assert_eq!(group_g.get("min_a"), Some(&ScalarValue::Int(1)));
        assert_eq!(group_g.get("max_a"), Some(&ScalarValue::Int(3)));
        let group_h = group_row(&rows, "h");
        assert_eq!(group_h.get("min_a"), Some(&ScalarValue::Int(5)));
        assert_eq!(group_h.get("max_a"), Some(&ScalarValue::Int(5)));
    }

    /// AC-0040-05 — agregados sin `GROUP BY` devuelven una única fila global.
    #[test] // @spec AC-0040-05
    fn test_ac_0040_05_aggregate_without_group_by() {
        let (_dir, mut database) = open_test_db("ac0040_05");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[(Some(1), "x"), (Some(2), "y"), (Some(3), "z")],
        );

        let rows = database
            .execute("SELECT COUNT(*), SUM(a), AVG(a), MIN(a), MAX(a) FROM t")
            .expect("select");
        assert_eq!(rows.len(), 1, "sin GROUP BY debe haber una sola fila");
        let row = &rows[0];
        assert_eq!(row.get("count"), Some(&ScalarValue::Int(3)));
        assert_eq!(row.get("sum_a"), Some(&ScalarValue::Int(6)));
        assert_eq!(row.get("avg_a"), Some(&ScalarValue::Float(2.0)));
        assert_eq!(row.get("min_a"), Some(&ScalarValue::Int(1)));
        assert_eq!(row.get("max_a"), Some(&ScalarValue::Int(3)));

        // BVA: tabla vacía → COUNT(*) = 0 y el resto NULL (semántica SQL).
        let (_empty_dir, mut empty) = open_test_db("ac0040_05_empty");
        create_ab_table(&mut empty);
        let global = empty
            .execute("SELECT COUNT(*), SUM(a), MIN(a) FROM t")
            .expect("select");
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].get("count"), Some(&ScalarValue::Int(0)));
        assert_eq!(global[0].get("sum_a"), Some(&ScalarValue::Null));
        assert_eq!(global[0].get("min_a"), Some(&ScalarValue::Null));
    }

    /// NF-0040-02 — errores accionables de la agregación.
    #[test] // @spec NF-0040-02
    fn test_ac_0040_errors_are_actionable() {
        let (_dir, mut database) = open_test_db("ac0040_err");
        seed_table(&mut database);

        let ungrouped = database
            .execute("SELECT a FROM t GROUP BY b")
            .expect_err("columna a fuera de GROUP BY");
        assert!(
            matches!(ungrouped, RuscaError::TypeMismatch { .. }),
            "se esperaba TypeMismatch, se obtuvo {ungrouped:?}"
        );

        let missing = database
            .execute("SELECT a FROM t GROUP BY ausente")
            .expect_err("GROUP BY inexistente");
        assert!(
            matches!(missing, RuscaError::ColumnNotFound { ref column } if column == "ausente"),
            "se esperaba ColumnNotFound, se obtuvo {missing:?}"
        );

        let bad_type = database
            .execute("SELECT SUM(b) FROM t")
            .expect_err("SUM sobre texto");
        assert!(
            matches!(bad_type, RuscaError::TypeMismatch { .. }),
            "se esperaba TypeMismatch, se obtuvo {bad_type:?}"
        );
    }

    /// BVA — agregados sobre columnas flotantes (`SUM`/`AVG` tipo `Float`).
    #[test] // @spec AC-0040-06
    fn test_ac_0040_06_float_aggregates() {
        let (_dir, mut database) = open_test_db("ac0040_06");
        database
            .create_table(
                "f",
                vec![
                    ColumnDef {
                        name: "x".to_string(),
                        col_type: ColumnType::Float,
                    },
                    ColumnDef {
                        name: "g".to_string(),
                        col_type: ColumnType::Text,
                    },
                ],
            )
            .expect("create_table");
        for (value, group) in [(1.5_f64, "a"), (2.5, "a"), (10.0, "b")] {
            let mut scalars = ScalarMap::new();
            scalars.insert("x".to_string(), ScalarValue::Float(value));
            scalars.insert("g".to_string(), ScalarValue::Text(group.to_string()));
            database.insert("f", scalars).expect("insert");
        }

        let rows = database
            .execute("SELECT g, SUM(x), AVG(x) FROM f GROUP BY g")
            .expect("select");
        let group_a = rows
            .iter()
            .find(|row| row.get("g") == Some(&ScalarValue::Text("a".to_string())))
            .expect("grupo a");
        assert_eq!(group_a.get("sum_x"), Some(&ScalarValue::Float(4.0)));
        assert_eq!(group_a.get("avg_x"), Some(&ScalarValue::Float(2.0)));
        let group_b = rows
            .iter()
            .find(|row| row.get("g") == Some(&ScalarValue::Text("b".to_string())))
            .expect("grupo b");
        assert_eq!(group_b.get("sum_x"), Some(&ScalarValue::Float(10.0)));
    }

    /// BVA — `GROUP BY` con `ORDER BY` de un agregado y `LIMIT`.
    #[test] // @spec AC-0040-07
    fn test_ac_0040_07_group_by_order_limit() {
        let (_dir, mut database) = open_test_db("ac0040_07");
        create_ab_table(&mut database);
        seed_pairs(
            &mut database,
            &[
                (Some(1), "x"),
                (Some(2), "x"),
                (Some(3), "y"),
                (Some(4), "z"),
                (Some(5), "z"),
                (Some(6), "z"),
            ],
        );

        let rows = database
            .execute("SELECT b, COUNT(*) AS total FROM t GROUP BY b ORDER BY total DESC LIMIT 2")
            .expect("select");
        assert_eq!(rows.len(), 2, "LIMIT se aplica a las filas agregadas");
        assert_eq!(rows[0].get("b"), Some(&ScalarValue::Text("z".to_string())));
        assert_eq!(rows[0].get("total"), Some(&ScalarValue::Int(3)));
        assert_eq!(rows[1].get("b"), Some(&ScalarValue::Text("x".to_string())));
        assert_eq!(rows[1].get("total"), Some(&ScalarValue::Int(2)));

        let unknown = database
            .execute("SELECT b, COUNT(*) AS total FROM t GROUP BY b ORDER BY ausente")
            .expect_err("ORDER BY no proyectado");
        assert!(
            matches!(unknown, RuscaError::ColumnNotFound { ref column } if column == "ausente"),
            "se esperaba ColumnNotFound, se obtuvo {unknown:?}"
        );
    }

    proptest! {
        /// PBT — invariantes de agregados sobre datos no nulos:
        /// `COUNT(*) == n`, `SUM == suma manual` y `MIN <= AVG <= MAX`.
        #[test]
        fn prop_aggregate_invariants(
            values in prop::collection::vec(-1000i64..1000, 1..40),
        ) {
            let (_dir, mut database) = open_test_db("prop_agg");
            create_ab_table(&mut database);
            for value in &values {
                let mut scalars = ScalarMap::new();
                scalars.insert("a".to_string(), ScalarValue::Int(*value));
                scalars.insert("b".to_string(), ScalarValue::Text("g".to_string()));
                database.insert("t", scalars).expect("insert");
            }

            let rows = database
                .execute("SELECT b, COUNT(*), SUM(a), AVG(a), MIN(a), MAX(a) FROM t GROUP BY b")
                .expect("select");
            prop_assert_eq!(rows.len(), 1);
            let row = &rows[0];
            let count = match row.get("count") {
                Some(ScalarValue::Int(value)) => *value,
                other => panic!("count inesperado: {other:?}"),
            };
            let sum = match row.get("sum_a") {
                Some(ScalarValue::Int(value)) => *value,
                other => panic!("sum_a inesperado: {other:?}"),
            };
            let avg = match row.get("avg_a") {
                Some(ScalarValue::Float(value)) => *value,
                other => panic!("avg_a inesperado: {other:?}"),
            };
            let min = match row.get("min_a") {
                Some(ScalarValue::Int(value)) => *value,
                other => panic!("min_a inesperado: {other:?}"),
            };
            let max = match row.get("max_a") {
                Some(ScalarValue::Int(value)) => *value,
                other => panic!("max_a inesperado: {other:?}"),
            };
            prop_assert_eq!(count, values.len() as i64);
            prop_assert_eq!(sum, values.iter().sum::<i64>());
            prop_assert!(min <= max);
            prop_assert!(avg >= min as f64 && avg <= max as f64);
        }

        /// PBT — con `NULL` intercalados: `COUNT(*) == n`,
        /// `COUNT(a) == nº de no nulos` y `SUM` ignora los `NULL`.
        #[test]
        fn prop_aggregate_ignores_nulls(
            values in prop::collection::vec(prop::option::of(-1000i64..1000), 1..40),
        ) {
            let (_dir, mut database) = open_test_db("prop_agg_null");
            create_ab_table(&mut database);
            for value in &values {
                let scalar = value.map_or(ScalarValue::Null, ScalarValue::Int);
                let mut scalars = ScalarMap::new();
                scalars.insert("a".to_string(), scalar);
                scalars.insert("b".to_string(), ScalarValue::Text("g".to_string()));
                database.insert("t", scalars).expect("insert");
            }

            let rows = database
                .execute("SELECT COUNT(*), COUNT(a), SUM(a) FROM t GROUP BY b")
                .expect("select");
            prop_assert_eq!(rows.len(), 1);
            let row = &rows[0];
            let non_null: Vec<i64> = values.iter().filter_map(|value| *value).collect();
            let count_star = match row.get("count") {
                Some(ScalarValue::Int(value)) => *value,
                other => panic!("count inesperado: {other:?}"),
            };
            let count_a = match row.get("count_a") {
                Some(ScalarValue::Int(value)) => *value,
                other => panic!("count_a inesperado: {other:?}"),
            };
            prop_assert_eq!(count_star, values.len() as i64);
            prop_assert_eq!(count_a, non_null.len() as i64);
            if non_null.is_empty() {
                prop_assert_eq!(row.get("sum_a"), Some(&ScalarValue::Null));
            } else {
                let sum = match row.get("sum_a") {
                    Some(ScalarValue::Int(value)) => *value,
                    other => panic!("sum_a inesperado: {other:?}"),
                };
                prop_assert_eq!(sum, non_null.iter().sum::<i64>());
            }
        }
    }

    proptest! {
        /// PBT de monotonicidad: `ASC` es no decreciente con `NULL` al final;
        /// `DESC` es no creciente con `NULL` al principio (orden estable).
        #[test]
        fn prop_order_by_is_monotonic(
            inputs in prop::collection::vec(prop::option::of(-50i64..50), 1..40),
        ) {
            let (_dir, mut database) = open_test_db("prop_order");
            create_ab_table(&mut database);
            for (index, value) in inputs.iter().enumerate() {
                let scalar = value.map_or(ScalarValue::Null, ScalarValue::Int);
                let mut scalars = ScalarMap::new();
                scalars.insert("a".to_string(), scalar);
                scalars.insert("b".to_string(), ScalarValue::Text(format!("v{index}")));
                database.insert("t", scalars).expect("insert");
            }

            let ascending = database
                .execute("SELECT a FROM t ORDER BY a ASC")
                .expect("asc");
            let asc_values = column_a(&ascending);
            let first_null = asc_values.iter().take_while(|value| value.is_some()).count();
            prop_assert_eq!(asc_values.len(), inputs.len());
            prop_assert!(asc_values[first_null..].iter().all(Option::is_none));
            for pair in asc_values[..first_null].windows(2) {
                prop_assert!(pair[0] <= pair[1], "ASC no monótono: {pair:?}");
            }

            let descending = database
                .execute("SELECT a FROM t ORDER BY a DESC")
                .expect("desc");
            let desc_values = column_a(&descending);
            let start = desc_values.iter().take_while(|value| value.is_none()).count();
            prop_assert!(desc_values[..start].iter().all(Option::is_none));
            for pair in desc_values[start..].windows(2) {
                prop_assert!(pair[0] >= pair[1], "DESC no antimonótono: {pair:?}");
            }
        }
    }
}
