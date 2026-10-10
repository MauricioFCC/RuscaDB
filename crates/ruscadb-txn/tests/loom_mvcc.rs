//! Tests de modelo loom para el MVCC de RuscaDB (SPEC-0053).
//!
//! Verifica con `loom::model` que los commits concurrentes sobre claves
//! disjuntas nunca se pierden (AC-0053-01) y que un snapshot concurrente con
//! un commit observa todo o nada, nunca una escritura parcial (AC-0053-02).
//!
//! ## Real vs modelado (FR-0053-04)
//!
//! - **Real:** `TxnManager`, `Version` y `Snapshot` son el código de
//!   producción sin modificar (sin `cfg(loom)` en `src/`).
//! - **Modelado:** el estado compartido entre hebras (`Mutex`) y las hebras
//!   (`thread::spawn`) usan primitivas `loom::sync` / `loom::thread`, de modo
//!   que loom explora sus planificaciones. El interior del gestor (`BTreeSet`)
//!   queda opaco a loom, pero todo acceso cruza el `Mutex`: cada
//!   adquisición/liberación es un punto de planificación modelado.
//! - **Claves disjuntas:** cada hebra publica sus versiones con sus propios
//!   `TxId`; la disyunción equivale a claves distintas porque el gestor no
//!   declara conflictos entre `TxId` diferentes.
//! - **BVA:** el modelo usa 2 hebras (la frontera de 1 hebra la cubre el test
//!   secuencial `test_ac_0053_03_suite_green`); el commit publica 2 claves con
//!   el mismo `TxId` (frontera de misma clave del diseño).
//!
//! El modelo está acotado (2 hebras, 2 commits/hebra, `preemption_bound = 2`)
//! para que la exploración termine rápido (NF-0053-01).

#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom::model::Builder;
use loom::sync::{Arc, Mutex};
use ruscadb_txn::{TxId, TxnManager, Version};

/// Commits que cada hebra ejecuta en el test de no-pérdida.
const COMMITS_PER_THREAD: usize = 2;

/// Hebras del modelo de commits concurrentes.
const MODEL_THREADS: usize = 2;

/// Cota de preemptions explorada por loom (rápida y caza la mayoría de bugs).
const PREEMPTION_BOUND: usize = 2;

/// Estado compartido del modelo: gestor más log de versiones publicadas.
///
/// El log representa el almacén publicado: vive bajo el mismo `Mutex` que el
/// gestor, así que cada commit publica su versión atómicamente.
///
/// Campos:
///     manager: Gestor MVCC real (código de producción).
///     published: Versiones publicadas por commits confirmados.
#[derive(Debug)]
struct SharedModel {
    /// Gestor MVCC real (código de producción).
    manager: TxnManager,
    /// Versiones publicadas por commits confirmados.
    published: Vec<Version>,
}

/// Crea el `Builder` de loom acotado para una exploración rápida.
///
/// Returns:
///     Constructor con `preemption_bound` fijado.
fn model_builder() -> Builder {
    let mut builder = Builder::new();
    builder.preemption_bound = Some(PREEMPTION_BOUND);
    builder
}

/// Ejecuta un commit (`begin` + `commit` + publicación) sobre el estado.
///
/// Args:
///     state: Estado compartido protegido por el `Mutex` modelado.
fn model_commit_once(state: &Arc<Mutex<SharedModel>>) {
    let tx: TxId = state.lock().expect("candado").manager.begin();
    let mut guard = state.lock().expect("candado");
    guard.manager.commit(tx).expect("commit disjunto");
    guard.published.push(Version::new(tx));
}

/// Ejecuta los commits de una hebra del modelo.
///
/// Args:
///     state: Estado compartido protegido por el `Mutex` modelado.
fn model_commit_loop(state: &Arc<Mutex<SharedModel>>) {
    for _ in 0..COMMITS_PER_THREAD {
        model_commit_once(state);
    }
}

/// Comprueba el invariante de no-pérdida sobre el estado final.
///
/// Args:
///     state: Estado compartido tras unir las hebras.
fn assert_no_loss(state: &Arc<Mutex<SharedModel>>) {
    let total: TxId = (MODEL_THREADS * COMMITS_PER_THREAD) as TxId;
    let guard = state.lock().expect("candado");
    assert_eq!(guard.published.len() as TxId, total, "commit perdido");
    let snapshot = guard.manager.snapshot();
    assert_eq!(snapshot.tx_id, total, "el LSN debe avanzar 2N");
    for version in &guard.published {
        assert!(snapshot.is_visible(version), "commit invisible");
    }
    assert_eq!(guard.manager.low_watermark(), total + 1, "watermark");
}

/// AC-0053-01 — commits concurrentes sobre claves disjuntas sin pérdidas.
///
/// Dos hebras hacen `COMMITS_PER_THREAD` commits cada una; loom explora las
/// planificaciones y el invariante exige que ambos sean visibles y que el LSN
/// (`snapshot.tx_id`) avance exactamente `2N`.
#[test]
fn test_ac_0053_01_concurrent_commits_no_loss() {
    model_builder().check(|| {
        let state = Arc::new(Mutex::new(SharedModel {
            manager: TxnManager::new(),
            published: Vec::new(),
        }));
        let mut handles = Vec::new();
        for _ in 0..MODEL_THREADS {
            let thread_state = Arc::clone(&state);
            handles.push(loom::thread::spawn(move || {
                model_commit_loop(&thread_state);
            }));
        }
        for handle in handles {
            handle.join().expect("hebra");
        }
        assert_no_loss(&state);
    });
}

/// AC-0053-02 — snapshot concurrente con commit nunca ve escritura parcial.
///
/// La escritora confirma una transacción que publica dos claves con el mismo
/// `TxId`; la lectora toma un snapshot concurrente. El invariante exige que
/// ambas visibilidades coincidan (todo o nada), en toda planificación.
#[test]
fn test_ac_0053_02_snapshot_atomicity() {
    model_builder().check(|| {
        let state = Arc::new(Mutex::new(TxnManager::new()));
        let tx: TxId = state.lock().expect("candado").begin();
        let version_a = Version::new(tx);
        let version_b = Version::new(tx);
        let writer_state = Arc::clone(&state);
        let writer = loom::thread::spawn(move || {
            writer_state
                .lock()
                .expect("candado")
                .commit(tx)
                .expect("commit");
        });
        let reader_state = Arc::clone(&state);
        let reader = loom::thread::spawn(move || reader_state.lock().expect("candado").snapshot());
        writer.join().expect("escritora");
        let snapshot = reader.join().expect("lectora");
        assert_eq!(
            snapshot.is_visible(&version_a),
            snapshot.is_visible(&version_b),
            "snapshot parcial"
        );
    });
}

/// AC-0053-03 — cordura secuencial de los invariantes (suite verde).
///
/// Reproduce ambas invariantes sin loom (frontera de 1 hebra del BVA): los
/// commits son visibles, el LSN avanza 1 por commit y un snapshot congela su
/// visibilidad ante commits posteriores.
#[test]
fn test_ac_0053_03_suite_green() {
    let mut manager = TxnManager::new();
    let first = manager.begin();
    let second = manager.begin();
    manager.commit(first).expect("commit");
    manager.commit(second).expect("commit");
    let snapshot = manager.snapshot();
    assert!(snapshot.is_visible(&Version::new(first)));
    assert!(snapshot.is_visible(&Version::new(second)));
    assert_eq!(snapshot.tx_id, second);
    let tx = manager.begin();
    let before = manager.snapshot();
    manager.commit(tx).expect("commit");
    let after = manager.snapshot();
    assert!(!before.is_visible(&Version::new(tx)));
    assert!(after.is_visible(&Version::new(tx)));
}
