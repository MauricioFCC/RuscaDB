//! Integración del blob store en la fachada (SPEC-0038, F4).
//!
//! Cubre los AC de `specs/blob_integration.md` a través de la API pública:
//! `Database::{put_blob, get_blob, gc_blobs}`, `DbConfig::blob_path` y
//! `DatabaseBuilder::blob_path`.

// El oráculo de tests puede fallar ruidosamente (`allow-expect-in-tests` del
// workspace no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use std::path::Path;

use proptest::prelude::*;
use ruscadb::{BlobHash, Database, DbConfig, RuscaError};

/// Abre una base con blob store usando `DbConfig` directo.
fn open_with_blobs(data_path: &Path, blob_path: &Path) -> Database {
    let mut config = DbConfig::new(data_path, 64);
    config.blob_path = Some(blob_path.to_path_buf());
    Database::open(config).expect("apertura con blob store")
}

/// Abre una base con blob store usando el builder fluido.
fn open_with_blobs_via_builder(data_path: &Path, blob_path: &Path) -> Database {
    Database::builder()
        .data_path(data_path)
        .pool_capacity(64)
        .blob_path(blob_path)
        .open()
        .expect("apertura del builder con blob store")
}

/// Cuenta recursivamente los ficheros bajo `dir` (blobs en disco).
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() { count_files(&path) } else { 1 }
        })
        .sum()
}

/// AC-0038-01 — roundtrip exacto y hash estable, con persistencia.
///
/// Usa el builder para ejercer `DatabaseBuilder::blob_path`, verifica bytes
/// exactos, hash content-addressed estable, BVA de bytes vacíos y lectura tras
/// reabrir la base.
#[test] // @spec AC-0038-01
fn test_ac_0038_01_blob_roundtrip() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let data_path = dir.path().join("db.data");
    let blob_path = dir.path().join("blobs-root");
    let content = b"contenido multimodal 0038";
    let hash = {
        let mut database = open_with_blobs_via_builder(&data_path, &blob_path);
        let hash = database.put_blob(content).expect("put_blob");
        assert_eq!(database.get_blob(&hash).expect("get_blob"), content);
        // Hash content-addressed estable: mismo contenido ⇒ mismo hash.
        assert_eq!(database.put_blob(content).expect("put repetido"), hash);
        // BVA: bytes vacíos hacen roundtrip.
        let empty = database.put_blob(b"").expect("put vacío");
        assert_eq!(database.get_blob(&empty).expect("get vacío"), b"");
        database.close().expect("close");
        hash
    };
    // Persistencia: reabrir y leer el blob exacto.
    let mut reopened = open_with_blobs(&data_path, &blob_path);
    assert_eq!(reopened.get_blob(&hash).expect("get tras reopen"), content);
}

/// AC-0038-02 — sin `blob_path`, las tres operaciones fallan accionablemente.
#[test] // @spec AC-0038-02
fn test_ac_0038_02_blob_not_configured() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let data_path = dir.path().join("sin_blobs.data");
    let mut database = Database::open(DbConfig::new(&data_path, 64)).expect("apertura");
    let missing = BlobHash("00".repeat(32));

    let put_error = database.put_blob(b"x").expect_err("put_blob debe fallar");
    let get_error = database
        .get_blob(&missing)
        .expect_err("get_blob debe fallar");
    let gc_error = database.gc_blobs().expect_err("gc_blobs debe fallar");

    for error in [&put_error, &get_error, &gc_error] {
        assert!(
            matches!(error, RuscaError::InvalidConfig(_)),
            "se esperaba InvalidConfig, se obtuvo {error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains("blob store no configurado") && message.contains("blob_path"),
            "el error debe ser accionable (menciona blob_path): {message}"
        );
    }
}

/// AC-0038-03 — `gc_blobs` elimina huérfanos y devuelve el conteo exacto.
///
/// Al reabrir, el índice de refcounts en memoria arranca vacío: los blobs en
/// disco sin referencia viva son huérfanos recolectables.
#[test] // @spec AC-0038-03
fn test_ac_0038_03_gc_blobs_removes_orphans() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let data_path = dir.path().join("gc.data");
    let blob_path = dir.path().join("gc-blobs");
    let orphan = {
        let mut database = open_with_blobs(&data_path, &blob_path);
        let hash = database.put_blob(b"blob huerfano 0038").expect("put_blob");
        database.close().expect("close");
        hash
    };

    let mut reopened = open_with_blobs(&data_path, &blob_path);
    assert_eq!(reopened.gc_blobs().expect("gc_blobs"), 1, "un huérfano");
    assert!(
        matches!(reopened.get_blob(&orphan), Err(RuscaError::Io(_))),
        "el blob recolectado debe dar NotFound"
    );
    assert_eq!(reopened.gc_blobs().expect("gc repetido"), 0, "idempotente");
}

/// AC-0038-04 — el barrier R7 bloquea `gc_blobs` con una transacción activa.
#[test] // @spec AC-0038-04
fn test_ac_0038_04_gc_blobs_blocked_with_active_tx() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let data_path = dir.path().join("barrier.data");
    let blob_path = dir.path().join("barrier-blobs");
    let mut database = open_with_blobs_via_builder(&data_path, &blob_path);

    let _tx = database.begin();
    let error = database
        .gc_blobs()
        .expect_err("gc_blobs con tx activa debe fallar");
    assert!(
        matches!(error, RuscaError::InvalidConfig(_)),
        "se esperaba InvalidConfig, se obtuvo {error:?}"
    );
    assert!(
        error.to_string().contains("transacci"),
        "el error del barrier debe ser accionable: {error}"
    );

    // Sin transacción en vuelo, el GC vuelve a estar permitido.
    database.commit().expect("commit de la tx");
    assert_eq!(
        database.gc_blobs().expect("gc tras commit"),
        0,
        "sin tx activa el GC procede"
    );
}

/// AC-0038-05 — dedup por contenido y seguridad del GC con referencias vivas.
#[test] // @spec AC-0038-05
fn test_ac_0038_05_dedup_and_gc_safety() {
    let dir = tempfile::tempdir().expect("dir temporal");
    let data_path = dir.path().join("dedup.data");
    let blob_path = dir.path().join("dedup-blobs");
    let mut database = open_with_blobs(&data_path, &blob_path);

    let first = database
        .put_blob(b"contenido deduplicado 0038")
        .expect("put 1");
    let second = database
        .put_blob(b"contenido deduplicado 0038")
        .expect("put 2");
    assert_eq!(first, second, "mismo contenido ⇒ mismo hash (dedup)");
    assert_eq!(
        count_files(&blob_path),
        1,
        "la deduplicación escribe un solo fichero"
    );

    // El blob está referenciado (refcount 2): el GC no lo elimina.
    assert_eq!(
        database.gc_blobs().expect("gc_blobs"),
        0,
        "no toca referenciado"
    );
    assert_eq!(
        database.get_blob(&first).expect("get_blob"),
        b"contenido deduplicado 0038"
    );
}

proptest! {
    /// PBT — `get_blob(put_blob(b)) == b` para bytes arbitrarios.
    ///
    /// Se excluye el prefijo `RCE1` con longitud de envelope: en modo claro el
    /// blob store reserva ese magic para el formato cifrado (SPEC-0013).
    #[test]
    fn prop_ac_0038_get_blob_roundtrip(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        prop_assume!(!(bytes.starts_with(b"RCE1") && bytes.len() >= 45));
        let dir = tempfile::tempdir().expect("dir temporal");
        let data_path = dir.path().join("prop.data");
        let blob_path = dir.path().join("prop-blobs");
        let mut database = open_with_blobs(&data_path, &blob_path);
        let hash = database.put_blob(&bytes).expect("put_blob");
        let recovered = database.get_blob(&hash).expect("get_blob");
        prop_assert_eq!(recovered, bytes);
    }
}
