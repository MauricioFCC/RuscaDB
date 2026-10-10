//! Integración end-to-end de MVCC + manifiesto en la fachada (SPEC-0019).
//!
//! Cubre los criterios AC-0019-01..05 y la propiedad PBT de visibilidad: un
//! snapshot tomado antes de un `insert` nunca ve ese `insert`.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{
    CURRENT_SCHEMA_VERSION, ColumnDef, ColumnType, Database, DbConfig, RuscaError, ScalarMap,
    ScalarValue,
};

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.data"));
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `t(a INT, b TEXT)`.
fn create_t(database: &mut Database) {
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

/// Inserta una fila `(a, b)` en `t` (auto-commit).
fn insert_row(database: &mut Database, a: i64, b: &str) {
    let scalars = ScalarMap::from([
        ("a".to_string(), ScalarValue::Int(a)),
        ("b".to_string(), ScalarValue::Text(b.to_string())),
    ]);
    database.insert("t", scalars).expect("insert");
}

/// AC-0019-01 — el manifiesto se crea, el epoch sube y persiste entre aperturas.
#[test]
// @spec AC-0019-01
fn test_ac_0019_01_manifest_epoch_persists() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac19_01.data");
    let manifest_path = path.with_extension("manifest.json");

    {
        let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
        assert_eq!(database.manifest().schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(database.manifest().epoch, 0, "base nueva: epoch neutro");
        create_t(&mut database);
        insert_row(&mut database, 1, "x");
        assert!(
            database.manifest().epoch >= 1,
            "cada commit con páginas sucias incrementa el epoch"
        );
    }

    assert!(
        manifest_path.exists(),
        "el manifiesto debe existir en {}",
        manifest_path.display()
    );

    let reopened = Database::open(DbConfig::new(&path, 64)).expect("reopen");
    assert!(
        reopened.manifest().epoch >= 1,
        "el epoch persiste entre aperturas"
    );
}

/// AC-0019-02 — manifiesto con JSON inválido o schema_version futura → error.
#[test]
// @spec AC-0019-02
fn test_ac_0019_02_corrupt_or_future_manifest_is_error() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac19_02.data");
    let manifest_path = path.with_extension("manifest.json");

    std::fs::write(&manifest_path, b"{ esto no es json valido ").expect("write");
    let corrupt = Database::open(DbConfig::new(&path, 64));
    assert!(
        matches!(corrupt, Err(RuscaError::CorruptManifest(_))),
        "JSON inválido debe dar CorruptManifest"
    );

    let future = format!(
        r#"{{"schema_version":{},"epoch":0,"checkpoint_lsn":0}}"#,
        CURRENT_SCHEMA_VERSION + 1
    );
    std::fs::write(&manifest_path, future).expect("write");
    let unknown = Database::open(DbConfig::new(&path, 64));
    assert!(
        matches!(unknown, Err(RuscaError::CorruptManifest(_))),
        "schema_version desconocida debe dar CorruptManifest"
    );
}

/// AC-0019-03 — una versión de una tx en vuelo no es visible (sin dirty reads).
#[test]
// @spec AC-0019-03
fn test_ac_0019_03_in_flight_tx_not_visible() {
    let (_dir, mut database) = open_test_db("ac19_03");
    create_t(&mut database);

    let tx = database.begin();
    assert_eq!(database.active_tx(), Some(tx), "begin expone la tx activa");
    let in_flight = database.snapshot();
    insert_row(&mut database, 1, "x");

    let rows = database
        .execute_at("SELECT * FROM t", &in_flight)
        .expect("execute_at");
    assert!(
        rows.is_empty(),
        "una tx en vuelo no debe ser visible (sin dirty reads)"
    );
}

/// AC-0019-04 — snapshot previo al insert no lo ve; el nuevo sí.
#[test]
// @spec AC-0019-04
fn test_ac_0019_04_snapshot_visibility_end_to_end() {
    let (_dir, mut database) = open_test_db("ac19_04");
    create_t(&mut database);

    let old = database.snapshot();
    insert_row(&mut database, 1, "x");

    let rows_old = database.execute_at("SELECT * FROM t", &old).expect("old");
    assert!(
        rows_old.is_empty(),
        "el snapshot anterior al insert no debe verlo"
    );

    let rows_new = database.execute("SELECT * FROM t").expect("new");
    assert_eq!(rows_new.len(), 1, "el snapshot nuevo sí ve el insert");
}

/// AC-0019-05 — el checkpoint_lsn sigue al último LSN confirmado y persiste.
#[test]
// @spec AC-0019-05
fn test_ac_0019_05_checkpoint_lsn_tracks_commits() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac19_05.data");

    let last = {
        let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
        create_t(&mut database);
        insert_row(&mut database, 1, "x");
        insert_row(&mut database, 2, "y");
        let checkpoint = database.manifest().checkpoint_lsn;
        assert!(checkpoint > 0, "el checkpoint avanza con los commits");
        assert_eq!(database.last_lsn(), checkpoint, "last_lsn coincide");
        checkpoint
    };

    let reopened = Database::open(DbConfig::new(&path, 64)).expect("reopen");
    assert_eq!(
        reopened.manifest().checkpoint_lsn,
        last,
        "el checkpoint persiste y coincide con el último LSN confirmado"
    );
}

proptest! {
    /// Un snapshot tomado antes de un `insert` nunca ve ese `insert`.
    #[test]
    fn prop_snapshot_before_insert_never_sees_it(count in 1usize..=6) {
        let (_dir, mut database) = open_test_db("ac19_prop");
        create_t(&mut database);
        let snapshot = database.snapshot();

        for index in 0..count {
            insert_row(&mut database, index as i64, "x");
        }

        let old_rows = database
            .execute_at("SELECT * FROM t", &snapshot)
            .expect("snapshot viejo");
        prop_assert!(old_rows.is_empty());

        let new_rows = database.execute("SELECT * FROM t").expect("snapshot nuevo");
        prop_assert_eq!(new_rows.len(), count);
    }
}
