//! Lock cross-proceso de la base (SPEC-0076).
//!
//! Verifica los tres criterios de aceptación: un segundo proceso real es
//! rechazado, una segunda apertura en el mismo proceso también falla, y
//! dropear la `Database` libera el lock dejando la base reabrible y operativa.
//!
//! El proceso hijo se lanza con el mismo binario de test (`current_exe`)
//! siguiendo el patrón de `ruscadb-wal::crash_tests`: el hijo abre la base,
//! señala `LOCKED` por stdout y se mantiene vivo hasta que el padre lo mata.

// El oráculo de integración puede fallar ruidosamente (equivalente a
// `allow-expect-in-tests` del workspace, que no alcanza a targets de
// integración).
#![allow(clippy::expect_used)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, RuscaError, ScalarMap, ScalarValue};

/// Cota de espera a que el hijo señale `LOCKED` (anti-cuelgue en CI).
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// Crea la tabla `t(a INT)` en la base.
///
/// Args:
///     database: Base abierta donde registrar la tabla.
fn create_t(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
}

/// Inserta una fila con el entero `a` en la tabla `t`.
///
/// Args:
///     database: Base abierta.
///     a: Valor de la única columna.
fn insert_a(database: &mut Database, a: i64) {
    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(a))]),
        )
        .expect("insert");
}

/// Hijo holder: abre la base, señala `LOCKED` por stdout y permanece vivo.
///
/// Solo actúa si `RUSCADB_LOCK_DIR` está definido (el padre lo lanza vía
/// `current_exe`); mantiene la `Database` —y por tanto el lock— viva hasta que
/// el padre mate el proceso. Sin la variable de entorno es un no-op.
#[test]
fn lock_holder_child() {
    let Ok(dir) = std::env::var("RUSCADB_LOCK_DIR") else {
        return;
    };
    let path = Path::new(&dir).join("db.data");
    let database = Database::open(DbConfig::new(&path, 64)).expect("open del hijo");
    println!("LOCKED");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    // Mantiene la base (y su lock) viva hasta que el padre mate el proceso.
    let _keep_alive = database;
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Lanza el hijo holder contra `dir` con stdout capturado.
///
/// Args:
///     dir: Directorio de trabajo del escenario (la base vive en `db.data`).
///
/// Returns:
///     El hijo en ejecución (el padre debe matarlo al terminar).
fn spawn_lock_holder(dir: &Path) -> Child {
    let exe = std::env::current_exe().expect("exe del test");
    Command::new(exe)
        .arg("--exact")
        .arg("lock_holder_child")
        .arg("--nocapture")
        .env("RUSCADB_LOCK_DIR", dir)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn del hijo holder")
}

/// Espera (con cota) a que el hijo señale `LOCKED` por stdout.
///
/// Args:
///     child: Hijo holder con stdout capturado.
fn wait_locked(child: &mut Child) {
    let stdout = child.stdout.take().expect("stdout del hijo");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(text) => {
                    if tx.send(text).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("el hijo no señaló LOCKED en {READY_TIMEOUT:?}");
        }
        match rx.recv_timeout(remaining) {
            Ok(line) if line.contains("LOCKED") => return,
            Ok(_) => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("timeout esperando LOCKED del hijo")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("el hijo terminó sin señalar LOCKED")
            }
        }
    }
}

/// AC-0076-01 — un segundo proceso real recibe `DatabaseInUse` con la ruta.
#[test]
// @spec AC-0076-01
fn test_ac_0076_01_second_process_fails_in_use() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("db.data");

    let mut child = spawn_lock_holder(dir.path());
    wait_locked(&mut child);

    // El hijo mantiene el lock: la apertura del padre debe fallar.
    match Database::open(DbConfig::new(&path, 64)) {
        Err(RuscaError::DatabaseInUse { path: reported }) => {
            assert!(
                reported.contains("db.data"),
                "la ruta reportada debe mencionar el fichero de datos: {reported}"
            );
        }
        Err(other) => {
            let _ = child.kill();
            panic!("se esperaba DatabaseInUse, se obtuvo {other:?}");
        }
        Ok(_) => {
            let _ = child.kill();
            panic!("el padre no debe abrir mientras el hijo tiene el lock");
        }
    }

    // Al terminar el hijo, el SO libera el lock: la reapertura tiene éxito.
    child.kill().expect("kill del hijo");
    child.wait().expect("wait del hijo");
    let reopened = Database::open(DbConfig::new(&path, 64)).expect("reapertura tras el kill");
    drop(reopened);
}

/// AC-0076-02 — una segunda `Database::open` en el mismo proceso falla.
#[test]
// @spec AC-0076-02
fn test_ac_0076_02_second_open_same_process_fails() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("db.data");
    let _first = Database::open(DbConfig::new(&path, 64)).expect("primer open");

    match Database::open(DbConfig::new(&path, 64)) {
        Err(RuscaError::DatabaseInUse { path: reported }) => {
            assert!(
                reported.contains("db.data"),
                "la ruta reportada debe mencionar el fichero de datos: {reported}"
            );
        }
        Err(other) => panic!("se esperaba DatabaseInUse, se obtuvo {other:?}"),
        Ok(_) => panic!("la segunda apertura en el mismo proceso debe fallar"),
    }
}

/// AC-0076-03 — dropear la `Database` libera el lock y permite reabrir.
#[test]
// @spec AC-0076-03
fn test_ac_0076_03_drop_releases_lock_for_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("db.data");
    {
        let mut database = Database::open(DbConfig::new(&path, 64)).expect("open");
        create_t(&mut database);
        insert_a(&mut database, 1);
        database.commit().expect("commit");
        // El drop de la `Database` libera el lock vía `LockGuard::drop`.
        drop(database);
    }

    let mut database = Database::open(DbConfig::new(&path, 64)).expect("reopen tras drop");
    insert_a(&mut database, 2);
    let rows = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(
        rows.len(),
        2,
        "la base reabierta sigue operativa (insert+select)"
    );
    database.close().expect("cierre limpio");
}
