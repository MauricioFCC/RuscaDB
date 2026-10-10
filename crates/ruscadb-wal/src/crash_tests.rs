//! Harness crash-recovery con fault injection (SPEC-0056, I1/FF-12).
//!
//! Estrategia honesta a nivel WAL:
//! - El hijo (`crash_writer_child`) escribe N registros y, según `SYNC`, hace
//!   `sync` o no; el padre lo mata (`kill`, portable) tras el sentinel READY.
//! - Pre-sync solo garantiza **nunca rasgado-válido** (la durabilidad ante
//!   pérdida de energía no es observable matando el proceso; la atomicidad de
//!   visibilidad vive en el manifiesto de la fachada, no en el WAL).
//! - Post-sync garantiza **durable exacto** (SI-1 a nivel WAL).
//! - La tail rasgada determinista se inyecta truncando a mitad del último
//!   frame (fold de torn-write real).
//!
//! Todo el módulo es `cfg(test)`: coste cero en release.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use super::{RecordKind, RecoveryOutcome, Wal, recover};

/// Registros escritos por el hijo en cada escenario de crash.
const CRASH_RECORDS: usize = 8;
/// Espera máxima del sentinel READY (anti-cuelgue en CI).
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Hijo escritor: solo actúa invocado con `RUSCADB_CRASH_DIR` (si no, no-op).
///
/// Escribe `CRASH_RECORDS` commits `rec-{i}`, hace `sync` si
/// `RUSCADB_CRASH_SYNC=1`, crea el sentinel READY y se queda aparcado hasta
/// que el padre lo mate (simula el crash en el punto elegido).
#[test]
fn crash_writer_child() {
    let Ok(dir) = std::env::var("RUSCADB_CRASH_DIR") else {
        return;
    };
    let sync = std::env::var("RUSCADB_CRASH_SYNC").as_deref() == Ok("1");
    let mut wal = Wal::open(Path::new(&dir).join("wal.log")).expect("open hijo");
    for index in 0..CRASH_RECORDS {
        wal.append(
            index as u64,
            RecordKind::Commit,
            format!("rec-{index}").as_bytes(),
        )
        .expect("append hijo");
    }
    if sync {
        wal.sync().expect("sync hijo");
    }
    std::fs::write(Path::new(&dir).join("READY"), b"ok").expect("sentinel");
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Ejecuta el hijo, espera READY, lo mata y recupera (hook de fault, SPEC-0056).
///
/// El kill es portable (`Child::kill`: SIGKILL en Unix, TerminateProcess en
/// Windows). Tras `wait` los handles del hijo están liberados y el WAL puede
/// reabrirse sin bloqueos.
///
/// Args:
///     dir: Directorio de trabajo del escenario (el WAL vive en `wal.log`).
///     sync: Si es `true`, el hijo hace `sync` antes del sentinel.
///
/// Returns:
///     El resultado del `recover` tras el kill.
fn run_crash_child(dir: &Path, sync: bool) -> RecoveryOutcome {
    let mut child = spawn_writer(dir, sync);
    wait_ready(dir);
    child.kill().expect("kill del hijo");
    child.wait().expect("wait del hijo");
    recover(dir.join("wal.log")).expect("recover tras el kill")
}

/// AC-0056-01 — kill pre-sync: nunca rasgado-válido.
///
/// Given: hijo que escribe sin `sync` y muere por kill.
/// When: se recupera.
/// Then: `recover` es `Ok` y lo recuperado es un prefijo válido (0..=N
///     registros `rec-{i}` en orden); jamás un frame a medias como válido.
#[test]
fn test_ac_0056_01_kill_before_fsync_invisible() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_crash_child(dir.path(), false);
    assert!(
        outcome.records.len() <= CRASH_RECORDS,
        "como mucho lo escrito: {}",
        outcome.records.len()
    );
    for (index, record) in outcome.records.iter().enumerate() {
        assert_eq!(record.kind, RecordKind::Commit);
        assert_eq!(record.payload, format!("rec-{index}").as_bytes());
    }
    // Re-recover idéntico (nada a medias que cambie entre pasadas).
    let again = recover(dir.path().join("wal.log")).expect("segundo recover");
    assert_eq!(again.records, outcome.records);
}

/// AC-0056-02 — kill post-sync: durable exacto (SI-1 a nivel WAL).
///
/// Given: hijo que escribe + `sync` y muere por kill.
/// When: se recupera.
/// Then: los N registros exactos en orden con LSNs 0..N.
#[test]
fn test_ac_0056_02_kill_after_fsync_durable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_crash_child(dir.path(), true);
    assert_eq!(outcome.records.len(), CRASH_RECORDS);
    for (index, record) in outcome.records.iter().enumerate() {
        assert_eq!(record.lsn, index as u64);
        assert_eq!(record.kind, RecordKind::Commit);
        assert_eq!(record.payload, format!("rec-{index}").as_bytes());
    }
    assert_eq!(outcome.truncated_bytes, 0, "nada que truncar post-sync");
}

/// AC-0056-03 — tail truncada a mitad de frame se descarta.
///
/// Given: WAL con 3 commits + sync, cortado 2 B antes del final.
/// When: se recupera.
/// Then: sobreviven los 2 primeros exactos y `truncated_bytes > 0`.
#[test]
fn test_ac_0056_03_truncated_tail_discarded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wal.log");
    {
        let mut wal = Wal::open(&path).expect("open");
        for index in 0..3u64 {
            wal.append(index, RecordKind::Commit, format!("rec-{index}").as_bytes())
                .expect("append");
        }
        wal.sync().expect("sync");
    }
    let len = std::fs::metadata(&path).expect("metadata").len();
    // Corta dentro del último frame (sus 2 últimos bytes: mitad del CRC).
    truncate_to(&path, len - 2);

    let outcome = recover(&path).expect("recover");
    assert_eq!(outcome.records.len(), 2, "sobrevive el prefijo válido");
    assert!(outcome.truncated_bytes > 0, "la tail rasgada se descuenta");
    assert_eq!(outcome.records[1].payload, b"rec-1");
}

/// AC-0056-04 — el replay es idempotente (I3).
///
/// Given: un WAL recuperado una vez.
/// When: se repite el recovery.
/// Then: `recover(recover(w)) == recover(w)` (mismos registros y offsets).
#[test]
fn test_ac_0056_04_replay_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wal.log");
    {
        let mut wal = Wal::open(&path).expect("open");
        for index in 0..5u64 {
            wal.append(index, RecordKind::Commit, format!("rec-{index}").as_bytes())
                .expect("append");
        }
        wal.sync().expect("sync");
    }
    let first = recover(&path).expect("primer recover");
    let second = recover(&path).expect("segundo recover");
    assert_eq!(first, second, "replay idempotente");
}

/// Trunca el fichero a `len` bytes (inyección determinista de torn-write).
fn truncate_to(path: &Path, len: u64) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open para truncar");
    file.set_len(len).expect("set_len");
}

/// Espera el sentinel READY con cota (anti-cuelgue).
fn wait_ready(dir: &Path) {
    let sentinel: PathBuf = dir.join("READY");
    let start = Instant::now();
    while !sentinel.exists() {
        assert!(
            start.elapsed() < READY_TIMEOUT,
            "el hijo no llegó a READY en {READY_TIMEOUT:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Genera el hijo escritor con el escenario dado.
fn spawn_writer(dir: &Path, sync: bool) -> Child {
    let exe = std::env::current_exe().expect("exe del test");
    Command::new(exe)
        .arg("--exact")
        .arg("crash_tests::crash_writer_child")
        .arg("--nocapture")
        .env("RUSCADB_CRASH_DIR", dir)
        .env("RUSCADB_CRASH_SYNC", if sync { "1" } else { "0" })
        .spawn()
        .expect("spawn del hijo")
}
