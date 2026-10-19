//! Tests de aceptación de **pool acotado en modo cifrado** (SPEC-0106).
//!
//! En modo cifrado `publish()` no escribe `.data` (SPEC-0013), así que hasta
//! ahora todas las páginas confirmadas vivían sucias en el pool y un conjunto
//! de trabajo mayor que `pool_capacity` fallaba con `BufferPoolFull` — incluso
//! en la apertura, que precargaba el WAL entero como sucio. SPEC-0106 añade el
//! índice overlay `page_id -> (lsn, offset)`: `open` deja de precargar,
//! `commit` marca limpio tras el fsync del WAL y cada miss se materializa
//! desde el commit record. Cubre AC-0106-01 (apertura con conjunto de trabajo
//! sobre la capacidad), AC-0106-02 (lecturas exactas tras desalojo),
//! AC-0106-05 (fallback a `.data` pre-cifrado bajo presión de capacidad) y
//! AC-0106-06 (rollback tras desalojos). Los AC-0106-03/04/07 (unidad: marca
//! limpia, overlay y límite restante) viven en las pruebas unitarias de
//! `src/database.rs`.

#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EncryptionConfig, ScalarMap, ScalarValue,
};
use std::path::Path;

/// Clave fija de pruebas (32 B).
const TEST_KEY: [u8; 32] = [0x44; 32];

/// Capacidad mínima: menor que el conjunto de trabajo de todos los tests
/// (fuerza desalojos y, hoy, `BufferPoolFull`).
const TINY_POOL: usize = 3;

/// Relleno de texto (~650 B por fila): reparte las filas en varias páginas
/// de heap de 4 KiB de modo que el conjunto de trabajo supere `TINY_POOL`.
fn big_text(a: i64) -> String {
    format!("fila-{a:04}-{}", "x".repeat(640))
}

/// Configuración de prueba con cifrado activado y pool de `capacity`.
fn encrypted_config(path: &Path, capacity: usize) -> DbConfig {
    let mut config = DbConfig::new(path, capacity);
    config.encryption = Some(EncryptionConfig::new(TEST_KEY));
    config
}

/// Abre `path` con la clave de pruebas (`encrypted`) o en claro.
fn open_in(path: &Path, capacity: usize, encrypted: bool) -> Database {
    let config = if encrypted {
        encrypted_config(path, capacity)
    } else {
        DbConfig::new(path, capacity)
    };
    Database::open(config).expect("apertura")
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

/// Escalares de la fila `(a, big_text(a))`.
fn scalars(a: i64) -> ScalarMap {
    ScalarMap::from([
        ("a".to_string(), ScalarValue::Int(a)),
        ("b".to_string(), ScalarValue::Text(big_text(a))),
    ])
}

/// Inserta y confirma las filas `[from, to)`.
fn insert_rows(database: &mut Database, from: i64, to: i64) {
    for a in from..to {
        database.insert("t", scalars(a)).expect("insert");
    }
}

/// Verifica que la tabla contiene exactamente `[from, to)` con valores exactos.
fn assert_rows_exact(database: &mut Database, from: i64, to: i64) {
    let rows = database
        .execute("SELECT a, b FROM t ORDER BY a")
        .expect("select completo");
    assert_eq!(
        rows.len() as i64,
        to - from,
        "el numero de filas debe ser exacto"
    );
    for (offset, row) in rows.iter().enumerate() {
        let expected = from + offset as i64;
        assert_eq!(
            row.get("a"),
            Some(&ScalarValue::Int(expected)),
            "valor de `a` exacto en la fila {offset}"
        );
        assert_eq!(
            row.get("b"),
            Some(&ScalarValue::Text(big_text(expected))),
            "valor de `b` exacto en la fila {offset}"
        );
    }
}

/// Primera página del heap de `table` según el catálogo confirmado.
fn heap_start(database: &mut Database, table: &str) -> ruscadb::PageId {
    database
        .catalog()
        .expect("catálogo")
        .get(table)
        .expect("tabla")
        .heap_start
}

/// AC-0106-01 — abrir con un conjunto de trabajo confirmado sobre la capacidad.
#[test]
// @spec AC-0106-01
fn test_ac_0106_01_encrypted_open_with_working_set_above_pool() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac010601.db");

    // Fase 1: una sesión con pool amplio confirma ~8 páginas de heap.
    {
        let mut database = open_in(&path, 64, true);
        create_t(&mut database);
        insert_rows(&mut database, 0, 40);
        database.close().expect("close fase 1");
    }

    // Fase 2: reapertura con pool tiny. Hoy la precarga del WAL (sucia)
    // desborda el pool y la apertura falla con BufferPoolFull.
    let mut database = open_in(&path, TINY_POOL, true);
    assert_rows_exact(&mut database, 0, 40);
    database.close().expect("close final");
}

/// AC-0106-02 — lecturas exactas tras desalojar todas las páginas confirmadas.
#[test]
// @spec AC-0106-02
fn test_ac_0106_02_encrypted_reads_after_eviction_are_fresh() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac010602.db");

    // Pool de 2 marcos con ~6 páginas de heap confirmadas: cada commit deja la
    // página desalojable y el barrido posterior materializa cada miss.
    let mut database = open_in(&path, 2, true);
    create_t(&mut database);
    insert_rows(&mut database, 0, 30);
    assert_rows_exact(&mut database, 0, 30);
    database.close().expect("close");
}

/// AC-0106-05 — las páginas pre-cifrado se sirven desde `.data` bajo presión.
#[test]
// @spec AC-0106-05
fn test_ac_0106_05_encrypted_reads_fall_back_to_data_file() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac010605.db");

    // Fase clara: filas publicadas a `.data`. El WAL en claro no admite la
    // clave (límite documentado de SPEC-0013), así que la activación del
    // cifrado parte de un WAL nuevo: las páginas `.data` en claro siguen
    // siendo la base de lectura hasta su reescritura.
    {
        let mut database = open_in(&path, 64, false);
        create_t(&mut database);
        insert_rows(&mut database, 0, 8);
        database.close().expect("close fase clara");
    }
    std::fs::remove_file(path.with_extension("wal")).expect("WAL nuevo");

    // Fase cifrada: 16 páginas-confirmadas vía commits en el WAL.
    {
        let mut database = open_in(&path, 64, true);
        insert_rows(&mut database, 8, 24);
        database.close().expect("close fase cifrada");
    }

    // Reapertura tiny: 8 filas pre-cifrado (solo `.data`) + 16 post-cifrado
    // (solo overlay) superan la capacidad del pool.
    let mut database = open_in(&path, TINY_POOL, true);
    assert_rows_exact(&mut database, 0, 24);
    database.close().expect("close final");
}

/// AC-0106-06 — el rollback devuelve el último estado confirmado tras desalojos.
#[test]
// @spec AC-0106-06
fn test_ac_0106_06_encrypted_rollback_after_eviction() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac010606.db");
    let wal_path = path.with_extension("wal");

    let mut database = open_in(&path, 3, true);
    create_t(&mut database);
    insert_rows(&mut database, 0, 25);
    // El barrido obliga a desalojar y reconstruir desde el overlay.
    assert_rows_exact(&mut database, 0, 25);

    let data_before = std::fs::read(&path).expect("lee .data");
    let wal_before = std::fs::read(&wal_path).expect("lee .wal");

    // Cambio sin commit sobre una página de heap (patrón de SPEC-0105).
    let page_id = heap_start(&mut database, "t");
    let mut modified = database.read_page(page_id).expect("baseline");
    modified.data_mut()[5] ^= 0xFF;
    let _tx = database.begin();
    database.write_page(&modified).expect("dirty sin commit");

    database.rollback().expect("rollback tras desalojos");

    assert_eq!(database.active_tx(), None, "la tx queda abortada");
    assert_eq!(
        std::fs::read(&path).expect(".data tras rollback"),
        data_before,
        "el .data queda byte-idéntico"
    );
    assert_eq!(
        std::fs::read(&wal_path).expect(".wal tras rollback"),
        wal_before,
        "el .wal queda byte-idéntico"
    );
    // Las páginas desalojadas se materializan de nuevo desde el overlay.
    assert_rows_exact(&mut database, 0, 25);

    // La base sigue operativa tras el rollback.
    insert_rows(&mut database, 25, 27);
    assert_rows_exact(&mut database, 0, 27);
    database.close().expect("close final");
}
