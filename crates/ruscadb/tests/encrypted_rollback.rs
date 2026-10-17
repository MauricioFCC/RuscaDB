//! Tests de aceptación de **rollback en modo cifrado** (SPEC-0105).
//!
//! En modo cifrado el WAL es la única fuente de verdad (SPEC-0013): `commit()`
//! no publica páginas a `.data`, así que `rollback()` restaura el último estado
//! confirmado reaplicando los commit records **sin tocar disco**. Cubre
//! AC-0105-01 (restaura el estado confirmado), AC-0105-02 (las páginas creadas
//! sin commit desaparecen), AC-0105-03 (el rollback no publica nada), AC-0105-04
//! (reanudación consistente tras `close`+reopen) y AC-0105-05 (idempotente sin
//! cambios). El AC-0105-06 (reconstrucción de los índices en memoria) vive en
//! las pruebas unitarias de `src/encryption.rs`.

#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EncryptionConfig, Page, PageId, RuscaError,
    ScalarMap, ScalarValue,
};
use std::path::{Path, PathBuf};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Clave fija de pruebas (32 B).
const TEST_KEY: [u8; 32] = [0x42; 32];

/// Marcador que jamás debe aparecer en claro en una base cifrada.
const MARKER: &str = "RUSCADB_AC0105_ROLLBACK_SIN_CLARO";

/// Abre una base cifrada temporal `tag.db` (WAL derivado `tag.wal`).
///
/// Returns:
///     El directorio temporal (que mantiene los ficheros vivos), la ruta de
///     datos y la base abierta con la clave de pruebas.
fn open_encrypted(tag: &str) -> (tempfile::TempDir, PathBuf, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(encrypted_config(&path)).expect("apertura cifrada");
    (dir, path, database)
}

/// Reabre la base de `path` con la clave de pruebas (cierre + reapertura).
fn reopen(path: &Path) -> Database {
    Database::open(encrypted_config(path)).expect("reapertura cifrada")
}

/// Configuración de prueba con cifrado activado y pool amplio.
fn encrypted_config(path: &Path) -> DbConfig {
    let mut config = DbConfig::new(path, POOL_CAPACITY);
    config.encryption = Some(EncryptionConfig::new(TEST_KEY));
    config
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

/// Escalares de la fila `(a, b)`.
fn scalars(a: i64, b: &str) -> ScalarMap {
    ScalarMap::from([
        ("a".to_string(), ScalarValue::Int(a)),
        ("b".to_string(), ScalarValue::Text(b.to_string())),
    ])
}

/// `true` si `haystack` contiene la subcadena `needle`.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Primera página del heap de `table` según el catálogo confirmado.
fn heap_start(database: &mut Database, table: &str) -> PageId {
    database
        .catalog()
        .expect("catálogo")
        .get(table)
        .expect("tabla")
        .heap_start
}

/// Aplica un cambio **sin commit** sobre la página `id` y devuelve la baseline.
///
/// Empieza una transacción explícita y muta un byte de la página: el estado en
/// el pool deja de coincidir con el último commit, que es lo que `rollback`
/// debe deshacer.
fn mutate_without_commit(database: &mut Database, id: PageId) -> Page {
    let baseline = database.read_page(id).expect("baseline confirmada");
    let tx = database.begin();
    let mut modified = baseline.clone();
    modified.data_mut()[5] ^= 0xFF;
    database
        .write_page(&modified)
        .expect("escritura sin commit");
    assert_eq!(
        database.active_tx(),
        Some(tx),
        "la transacción debe seguir en vuelo"
    );
    baseline
}

/// AC-0105-01 — el rollback cifrado restaura el último estado confirmado.
#[test]
// @spec AC-0105-01
fn test_ac_0105_01_encrypted_rollback_restores_confirmed_state() {
    let (_dir, _path, mut database) = open_encrypted("ac010501");
    create_t(&mut database);
    let confirmed = database
        .insert("t", scalars(1, "confirmado"))
        .expect("insert confirmado");
    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        1
    );

    let page_id = heap_start(&mut database, "t");
    let baseline = mutate_without_commit(&mut database, page_id);
    let inflight = database.read_page(page_id).expect("página en vuelo");
    assert_ne!(
        inflight.data(),
        baseline.data(),
        "el cambio sin commit debe vivir en el pool antes del rollback"
    );

    database.rollback().expect("rollback cifrado");

    assert_eq!(database.active_tx(), None, "la tx activa queda abortada");
    let reverted = database.read_page(page_id).expect("página restaurada");
    assert_eq!(
        reverted.data(),
        baseline.data(),
        "la página vuelve byte a byte al último estado confirmado"
    );
    let rows = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 1, "la fila confirmada sigue visible");
    assert!(
        database
            .get_record("t", &confirmed)
            .expect("point lookup")
            .is_some(),
        "el id confirmado sigue localizable"
    );
    let hits = database
        .execute("SELECT * FROM t WHERE b = 'confirmado'")
        .expect("select filtrado");
    assert_eq!(
        hits.len(),
        1,
        "el filtro por columna indexada sigue acertando"
    );

    // La base sigue operativa: acepta nuevas escrituras confirmadas.
    let nueva = database
        .insert("t", scalars(2, "post-rollback"))
        .expect("insert tras rollback");
    assert!(
        database
            .get_record("t", &nueva)
            .expect("insert posterior")
            .is_some(),
        "la fila escrita tras el rollback es visible"
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t")
            .expect("select final")
            .len(),
        2
    );
}

/// AC-0105-02 — las páginas creadas sin commit desaparecen (no están en el WAL).
#[test]
// @spec AC-0105-02
fn test_ac_0105_02_encrypted_rollback_drops_uncommitted_pages() {
    let (_dir, _path, mut database) = open_encrypted("ac010502");
    create_t(&mut database);
    database
        .insert("t", scalars(1, "confirmado"))
        .expect("insert confirmado");
    let heap = heap_start(&mut database, "t");

    // Página nueva escrita sin commit: existe solo en el pool, nunca en el WAL.
    let probe = PageId(heap.0 + 900);
    let mut probe_page = Page::new(probe);
    probe_page.data_mut()[0] = 0xEE;
    let _tx = database.begin();
    database.write_page(&probe_page).expect("página nueva");
    let inflight = database
        .read_page(probe)
        .expect("la página vive en el pool");
    assert_eq!(
        inflight.data(),
        probe_page.data(),
        "la página nueva debe estar cacheada antes del rollback"
    );

    database.rollback().expect("rollback cifrado");

    let error = database
        .read_page(probe)
        .expect_err("la página sin commit debe desaparecer");
    assert!(
        matches!(error, RuscaError::PageOutOfRange { .. }),
        "se esperaba PageOutOfRange, se obtuvo {error:?}"
    );
    let rows = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(
        rows.len(),
        1,
        "las filas confirmadas siguen intactas tras descartar la página"
    );
}

/// AC-0105-03 — el rollback cifrado no publica nada a disco.
#[test]
// @spec AC-0105-03
fn test_ac_0105_03_encrypted_rollback_writes_nothing() {
    let (_dir, path, mut database) = open_encrypted("ac010503");
    let wal_path = path.with_extension("wal");
    create_t(&mut database);
    database
        .insert("t", scalars(7, MARKER))
        .expect("insert con marcador");

    // Cambio sin commit que debe quedarse en el pool (no llegar a disco).
    let page_id = heap_start(&mut database, "t");
    mutate_without_commit(&mut database, page_id);

    let data_before = std::fs::read(&path).expect("lee .data");
    let wal_before = std::fs::read(&wal_path).expect("lee .wal");

    database.rollback().expect("rollback cifrado");

    let data_after = std::fs::read(&path).expect("lee .data tras rollback");
    let wal_after = std::fs::read(&wal_path).expect("lee .wal tras rollback");
    assert_eq!(
        data_before, data_after,
        "el .data queda byte-idéntico (el rollback no publica)"
    );
    assert_eq!(
        wal_before, wal_after,
        "el .wal queda byte-idéntico (el rollback no añade frames)"
    );
    assert!(
        !contains_bytes(&data_after, MARKER.as_bytes()),
        "el .data no expone el marcador en claro"
    );
    assert!(
        !contains_bytes(&wal_after, MARKER.as_bytes()),
        "el .wal no expone el marcador en claro"
    );
}

/// AC-0105-04 — la reanudación tras rollback es consistente (close + reopen).
#[test]
// @spec AC-0105-04
fn test_ac_0105_04_encrypted_rollback_resumes_consistently() {
    let (_dir, path, mut database) = open_encrypted("ac010504");
    create_t(&mut database);
    let first = database
        .insert("t", scalars(1, "confirmada"))
        .expect("insert confirmado");

    let page_id = heap_start(&mut database, "t");
    mutate_without_commit(&mut database, page_id);
    database.rollback().expect("rollback cifrado");
    database.close().expect("close tras rollback");

    // Reapertura 1: solo lo confirmado, sin duplicados ni pérdida.
    let mut reopened = reopen(&path);
    let rows = reopened.execute("SELECT * FROM t").expect("select 1");
    assert_eq!(rows.len(), 1, "solo la fila confirmada sobrevive al reopen");
    assert!(
        reopened
            .get_record("t", &first)
            .expect("point lookup 1")
            .is_some(),
        "la fila original se localiza tras reabrir"
    );

    // Nueva escritura confirmada tras el rollback...
    let second = reopened
        .insert("t", scalars(2, "post-reopen"))
        .expect("insert post-reopen");
    reopened.close().expect("close 2");

    // ...que sobrevive a un segundo reopen.
    let mut twice = reopen(&path);
    let rows = twice.execute("SELECT * FROM t").expect("select 2");
    assert_eq!(
        rows.len(),
        2,
        "sin pérdida ni duplicados en el segundo reopen"
    );
    assert!(
        twice
            .get_record("t", &first)
            .expect("point lookup 2a")
            .is_some()
    );
    assert!(
        twice
            .get_record("t", &second)
            .expect("point lookup 2b")
            .is_some()
    );
    twice.close().expect("close final");
}

/// AC-0105-05 — el rollback cifrado es idempotente y no toca disco sin cambios.
#[test]
// @spec AC-0105-05
fn test_ac_0105_05_encrypted_rollback_is_idempotent_noop() {
    let (_dir, path, mut database) = open_encrypted("ac010505");
    let wal_path = path.with_extension("wal");

    // (a) Base recién creada sin commits: dos rollbacks = no-op sin tocar disco.
    let data_vacia = std::fs::read(&path).expect(".data sin cambios");
    let wal_vacio = std::fs::read(&wal_path).expect(".wal sin cambios");
    database.rollback().expect("rollback 1 sin cambios");
    database.rollback().expect("rollback 2 sin cambios");
    assert_eq!(
        std::fs::read(&path).expect(".data tras no-ops"),
        data_vacia,
        "sin cambios, el .data queda intacto"
    );
    assert_eq!(
        std::fs::read(&wal_path).expect(".wal tras no-ops"),
        wal_vacio,
        "sin cambios, el .wal queda intacto"
    );
    assert_eq!(database.active_tx(), None, "sin tx en vuelo");

    // La base sigue operativa: el DDL posterior a los no-ops confirma y se ve.
    create_t(&mut database);

    // (b) Con estado confirmado y un cambio sin commit: doble rollback.
    let confirmed = database
        .insert("t", scalars(1, "confirmado"))
        .expect("insert confirmado");
    let page_id = heap_start(&mut database, "t");
    let baseline = mutate_without_commit(&mut database, page_id);
    let data_before = std::fs::read(&path).expect(".data con estado");
    let wal_before = std::fs::read(&wal_path).expect(".wal con estado");

    database.rollback().expect("rollback 1 con cambios");
    database.rollback().expect("rollback 2 idempotente");

    assert_eq!(
        std::fs::read(&path).expect(".data final"),
        data_before,
        "los rollbacks no tocan el .data"
    );
    assert_eq!(
        std::fs::read(&wal_path).expect(".wal final"),
        wal_before,
        "los rollbacks no tocan el .wal"
    );
    let reverted = database
        .read_page(page_id)
        .expect("página tras doble rollback");
    assert_eq!(
        reverted.data(),
        baseline.data(),
        "el estado confirmado queda intacto"
    );
    assert!(
        database
            .get_record("t", &confirmed)
            .expect("point lookup")
            .is_some(),
        "la fila confirmada sigue localizable"
    );
    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        1,
        "solo la fila confirmada es visible"
    );

    // La base sigue operativa tras los no-ops.
    database
        .insert("t", scalars(2, "sigue-operativa"))
        .expect("insert tras no-ops");
}
