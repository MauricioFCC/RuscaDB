//! Group commit real del WAL con crash-harness de proceso hijo (SPEC-0081).
//!
//! Verifica los tres criterios de aceptación con procesos hijos reales
//! (patrón de `tests/process_lock.rs` y `ruscadb-wal::crash_tests`):
//!
//! - AC-0081-01: `FsyncPolicy::Full` — cada ack es durable (`kill` tras N acks).
//! - AC-0081-02: `FsyncPolicy::Normal` — coalescing (fsyncs < commits) y los N
//!   acks persisten tras reapertura.
//! - AC-0081-03: `kill` en mitad de un grupo con `Normal` — recovery limpio
//!   (prefijo acknowledged completo, cola rasgada truncada, sin corrupción).
//!
//! El hijo se lanza con el mismo binario de test (`current_exe`) y el helper
//! `group_commit_child` se despacha por `RUSCADB_GC_MODE`. Sin `RUSCADB_GC_DIR`
//! el helper es un no-op (corre como test normal sin efecto).

// El oráculo de integración puede fallar ruidosamente (equivalente a
// `allow-expect-in-tests` del workspace, que no alcanza a targets de
// integración).
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use ruscadb::{Database, DbConfig, Page, PageId};
use ruscadb_wal::{FsyncPolicy, RecordKind, Wal, read_records, recover, verify};

/// Cota de espera a que el hijo señale `ACKED` (anti-cuelgue en CI).
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Commits confirmados por el hijo en AC-0081-01 (Full).
const FULL_COMMITS: usize = 16;
/// Commits en la ráfaga concurrente de AC-0081-02 (Normal).
const BURST_COMMITS: usize = 32;
/// Acks mínimos exigidos antes del kill en AC-0081-03 (Normal).
const KILL_ACKS: usize = 8;
/// Número de hilos que commitean en el escenario de kill (grupos reales).
const KILL_WORKERS: usize = 4;

/// Helper hijo: se despacha por `RUSCADB_GC_MODE` (no-op sin `RUSCADB_GC_DIR`).
///
/// Ejecutado como proceso separado vía `current_exe --exact group_commit_child`.
#[test]
fn group_commit_child() {
    let Ok(dir) = std::env::var("RUSCADB_GC_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let mode = std::env::var("RUSCADB_GC_MODE").unwrap_or_default();
    let n = env_usize("RUSCADB_GC_N");
    let k = env_usize("RUSCADB_GC_K");
    match mode.as_str() {
        "full_ack" => child_full_ack(&dir, n),
        "normal_burst" => child_normal_burst(&dir, n),
        "normal_kill" => child_normal_kill(&dir, k),
        _ => {}
    }
}

/// Lee un entero positivo de una variable de entorno (`0` si falta o es ilegible).
///
/// Args:
///     name: Nombre de la variable de entorno.
///
/// Returns:
///     El valor parseado o `0`.
fn env_usize(name: &str) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// Hijo de AC-0081-01: `Full`, N commits confirmados, señala `ACKED` y espera.
///
/// Args:
///     dir: Directorio del escenario (la base vive en `db.data`).
///     n: Número de commits confirmados antes de señalar.
fn child_full_ack(dir: &Path, n: usize) {
    let data_path = dir.join("db.data");
    let mut config = DbConfig::new(&data_path, 16);
    config.fsync_policy = FsyncPolicy::Full;
    let mut database = Database::open(config).expect("open Full del hijo");
    for round in 0..n {
        let mut page = Page::new(PageId(0));
        page.data_mut()[..8].copy_from_slice(&(round as u64 + 1).to_le_bytes());
        database.write_page(&page).expect("write_page del hijo");
        database.commit().expect("commit Full del hijo");
    }
    std::fs::write(dir.join("ACKED"), n.to_string()).expect("sentinel ACKED");
    wait_forever();
}

/// Hijo de AC-0081-02: ráfaga concurrente de N commits con `Normal`.
///
/// Cada hilo hace `append` + `sync` (un "commit" lógico); el writer thread
/// coalesce los fsyncs. Al terminar escribe el contador de fsyncs en `SYNCS`.
///
/// Args:
///     dir: Directorio del escenario (el WAL vive en `wal.log`).
///     n: Número de commits concurrentes.
fn child_normal_burst(dir: &Path, n: usize) {
    let wal = Arc::new(
        Wal::open_with(dir.join("wal.log"), FsyncPolicy::Normal).expect("open Normal del hijo"),
    );
    let barrier = Arc::new(Barrier::new(n));
    let mut handles = Vec::with_capacity(n);
    for index in 0..n {
        let wal = Arc::clone(&wal);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            wal.append(
                index as u64,
                RecordKind::Commit,
                format!("c-{index}").as_bytes(),
            )
            .expect("append concurrente");
            wal.sync().expect("sync concurrente");
        }));
    }
    for handle in handles {
        handle.join().expect("join de la ráfaga");
    }
    std::fs::write(dir.join("SYNCS"), wal.sync_count().to_string()).expect("contador de fsyncs");
}

/// Hijo de AC-0081-03: commitea en bucle con `Normal` y espera el kill.
///
/// Varios hilos hacen `append` + `sync` (grupos reales de group commit) hasta
/// que se alcanzan `k` acks acumulados; entonces escribe `ACKED` y prosigue
/// hasta que el padre lo mata.
///
/// Args:
///     dir: Directorio del escenario (el WAL vive en `wal.log`).
///     k: Acks acumulados que disparan el sentinel `ACKED`.
fn child_normal_kill(dir: &Path, k: usize) {
    let wal = Arc::new(
        Wal::open_with(dir.join("wal.log"), FsyncPolicy::Normal).expect("open Normal del hijo"),
    );
    let acked = Arc::new(AtomicUsize::new(0));
    let signaled = Arc::new(AtomicBool::new(false));
    for worker in 0..KILL_WORKERS {
        let wal = Arc::clone(&wal);
        let acked = Arc::clone(&acked);
        let signaled = Arc::clone(&signaled);
        let dir = dir.to_path_buf();
        // Los handles se dropean (detach): los hilos siguen hasta el kill.
        std::thread::spawn(move || {
            let mut sequence = 0u64;
            loop {
                wal.append(
                    worker as u64,
                    RecordKind::Commit,
                    format!("w{worker}-{sequence}").as_bytes(),
                )
                .expect("append en bucle");
                wal.sync().expect("sync en bucle");
                sequence += 1;
                let total = acked.fetch_add(1, Ordering::SeqCst) + 1;
                if total >= k && !signaled.swap(true, Ordering::SeqCst) {
                    std::fs::write(dir.join("ACKED"), total.to_string()).expect("sentinel ACKED");
                }
            }
        });
    }
    // El hilo principal espera el kill del padre (los workers siguen solos).
    wait_forever();
}

/// Bloquea el hilo indefinidamente (el padre lo mata).
fn wait_forever() -> ! {
    loop {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Lanza el hijo helper con el modo y parámetros dados.
///
/// Args:
///     dir: Directorio de trabajo del escenario.
///     mode: Modo del helper (`full_ack`, `normal_burst`, `normal_kill`).
///     n: Parámetro `RUSCADB_GC_N` (commits).
///     k: Parámetro `RUSCADB_GC_K` (acks mínimos).
///
/// Returns:
///     El hijo en ejecución (el padre debe matarlo o esperarlo).
fn spawn_child(dir: &Path, mode: &str, n: usize, k: usize) -> Child {
    let exe = std::env::current_exe().expect("exe del test");
    Command::new(exe)
        .arg("--exact")
        .arg("group_commit_child")
        .arg("--nocapture")
        .env("RUSCADB_GC_DIR", dir)
        .env("RUSCADB_GC_MODE", mode)
        .env("RUSCADB_GC_N", n.to_string())
        .env("RUSCADB_GC_K", k.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn del hijo")
}

/// Espera (con cota) a que el hijo cree el sentinel `name` en `dir`.
///
/// Args:
///     dir: Directorio del escenario.
///     name: Nombre del archivo sentinel.
fn wait_for_sentinel(dir: &Path, name: &str) {
    let sentinel = dir.join(name);
    let deadline = Instant::now() + READY_TIMEOUT;
    while !sentinel.exists() {
        assert!(
            Instant::now() < deadline,
            "el hijo no creó {name} en {READY_TIMEOUT:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Cuenta los `RecordKind::Commit` válidos de un WAL en disco.
///
/// Args:
///     wal_path: Ruta del WAL.
///
/// Returns:
///     El número de commit records recuperables.
fn commit_count(wal_path: &Path) -> usize {
    read_records(wal_path)
        .expect("lectura del WAL")
        .iter()
        .filter(|record| record.kind == RecordKind::Commit)
        .count()
}

/// AC-0081-01 — con `Full`, cada ack es durable (kill tras N acks).
///
/// Given: base `FsyncPolicy::Full` y un hijo que hace N commits confirmados.
/// When: el hijo muere tras recibir los N acks.
/// Then: la reapertura recupera exactamente los N commits.
#[test]
// @spec AC-0081-01
fn test_ac_0081_01_full_policy_every_ack_is_durable() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let mut child = spawn_child(dir.path(), "full_ack", FULL_COMMITS, 0);
    wait_for_sentinel(dir.path(), "ACKED");
    child.kill().expect("kill del hijo");
    child.wait().expect("wait del hijo");

    let wal_path = dir.path().join("db.data").with_extension("wal");
    assert_eq!(
        commit_count(&wal_path),
        FULL_COMMITS,
        "cada ack con Full debe ser durable"
    );
    let database = Database::open(DbConfig::new(dir.path().join("db.data"), 16))
        .expect("reapertura tras el kill");
    assert_eq!(
        database.last_lsn(),
        FULL_COMMITS as u64 - 1,
        "los N commits se recuperan exactamente"
    );
}

/// AC-0081-02 — con `Normal`, una ráfaga coalesce fsyncs y persiste los acks.
///
/// Given: WAL `FsyncPolicy::Normal` y una ráfaga de N commits concurrentes.
/// When: se ejecuta la ráfaga y se consulta el contador de fsyncs.
/// Then: fsyncs < N (coalescing) y tras reapertura los N acks están en el WAL.
#[test]
// @spec AC-0081-02
fn test_ac_0081_02_normal_policy_groups_fsyncs() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let mut child = spawn_child(dir.path(), "normal_burst", BURST_COMMITS, 0);
    let status = child.wait().expect("wait del hijo");
    assert!(status.success(), "el hijo de la ráfaga debe terminar Ok");

    let syncs: u64 = std::fs::read_to_string(dir.path().join("SYNCS"))
        .expect("contador de fsyncs")
        .trim()
        .parse()
        .expect("contador de fsyncs entero");
    assert!(
        syncs < BURST_COMMITS as u64,
        "coalescing: {syncs} fsyncs < {BURST_COMMITS} commits"
    );

    let wal_path = dir.path().join("wal.log");
    assert_eq!(
        commit_count(&wal_path),
        BURST_COMMITS,
        "los N acks persisten en el WAL tras el cierre"
    );
}

/// AC-0081-03 — kill en mitad de un grupo → recovery con prefijo limpio.
///
/// Given: hijo `Normal` que commitea en bucle (varios hilos, grupos reales).
/// When: se le hace kill tras confirmar al menos K acks.
/// Then: recovery limpio (prefijo acknowledged completo, cola truncada, sin
///     corrupción de frames) e idempotente.
#[test]
// @spec AC-0081-03
fn test_ac_0081_03_kill_during_group_recovers_clean_prefix() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let mut child = spawn_child(dir.path(), "normal_kill", 0, KILL_ACKS);
    wait_for_sentinel(dir.path(), "ACKED");
    child.kill().expect("kill en mitad del grupo");
    child.wait().expect("wait del hijo");

    let wal_path = dir.path().join("wal.log");
    let verification = verify(&wal_path, None).expect("verify del WAL");
    assert!(
        !verification.corrupted,
        "el kill no debe corromper frames: {verification:?}"
    );

    let records = read_records(&wal_path).expect("lectura del WAL");
    let commits = records
        .iter()
        .filter(|record| record.kind == RecordKind::Commit)
        .count();
    assert!(
        commits >= KILL_ACKS,
        "prefijo acknowledged completo: {commits} >= {KILL_ACKS}"
    );

    // Recovery: trunca la cola rasgada sin perder frames válidos.
    let outcome = recover(&wal_path).expect("recovery tras el kill");
    assert_eq!(
        outcome.records.len(),
        records.len(),
        "el recovery conserva todo el prefijo válido"
    );
    let again = read_records(&wal_path).expect("segunda lectura");
    assert_eq!(again, outcome.records, "replay idempotente");
}
