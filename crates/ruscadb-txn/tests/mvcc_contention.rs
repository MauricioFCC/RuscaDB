//! Contención MVCC c=64 + retry + bloat (SPEC-0055).
//!
//! Cubre AC-0055-01..04 con hilos reales: no-pérdida bajo contención,
//! conflicto write-write accionable (first-committer-wins), helper
//! `retry_on_conflict` que converge y métrica de bloat con reclamo.

#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use ruscadb_core::RuscaError;
use ruscadb_txn::{NO_TX, Snapshot, TxnManager, Version, dead_versions, gc, retry_on_conflict};

/// Número de committers concurrentes del stress (roadmap R6: c=64).
const CONTENTION_THREADS: usize = 64;

/// Abre un gestor compartido entre hebras.
fn shared_manager() -> Arc<Mutex<TxnManager>> {
    Arc::new(Mutex::new(TxnManager::new()))
}

/// AC-0055-01 — 64 commits concurrentes sin pérdidas.
///
/// Given: 64 hebras con claves disjuntas sobre un gestor compartido.
/// When: todas hacen begin + stage_write + commit.
/// Then: los 64 commits son visibles y el LSN avanza exactamente 64.
#[test]
// @spec AC-0055-01
fn test_ac_0055_01_no_loss_under_contention() {
    let manager = shared_manager();
    let mut handles = Vec::with_capacity(CONTENTION_THREADS);
    for index in 0..CONTENTION_THREADS {
        let shared = Arc::clone(&manager);
        handles.push(std::thread::spawn(move || {
            let key = format!("key-{index:03}");
            let mut guard = shared.lock().expect("mutex del gestor");
            let tx = guard.begin();
            guard.stage_write(tx, key).expect("stage_write");
            guard.commit(tx).expect("commit");
            tx
        }));
    }
    let mut committed: BTreeSet<u64> = BTreeSet::new();
    for handle in handles {
        committed.insert(handle.join().expect("hebra"));
    }
    assert_eq!(committed.len(), CONTENTION_THREADS, "ningún commit perdido");

    let guard = manager.lock().expect("mutex del gestor");
    for tx in &committed {
        assert!(guard.is_committed(*tx), "tx {tx} debe estar confirmada");
    }
    // TxIds 1..=64: el snapshot congela en 64 y el watermark avanza a 65.
    let snapshot: Snapshot = guard.snapshot();
    assert_eq!(snapshot.tx_id, CONTENTION_THREADS as u64);
    assert_eq!(guard.low_watermark(), CONTENTION_THREADS as u64 + 1);
}

/// AC-0055-02 — conflicto write-write accionable.
///
/// Given: dos tx concurrentes escribiendo la misma clave.
/// When: ambas intentan commit.
/// Then: la primera gana y la segunda recibe `WriteConflict` con la clave.
#[test]
// @spec AC-0055-02
fn test_ac_0055_02_write_write_conflict_actionable() {
    let mut manager = TxnManager::new();
    let first = manager.begin();
    let second = manager.begin();
    manager
        .stage_write(first, "hot-key".to_string())
        .expect("stage first");
    manager
        .stage_write(second, "hot-key".to_string())
        .expect("stage second");

    manager.commit(first).expect("gana el primero");
    let conflict = manager
        .commit(second)
        .expect_err("el segundo debe conflictuar");
    assert!(
        matches!(conflict, RuscaError::WriteConflict { ref key } if key == "hot-key"),
        "se esperaba WriteConflict con la clave, se obtuvo {conflict:?}"
    );

    // Claves disjuntas no conflictúan.
    let third = manager.begin();
    let fourth = manager.begin();
    manager
        .stage_write(third, "a".to_string())
        .expect("stage third");
    manager
        .stage_write(fourth, "b".to_string())
        .expect("stage fourth");
    manager.commit(third).expect("commit third");
    manager.commit(fourth).expect("commit fourth sin conflicto");

    // Quien empieza DESPUÉS del commit ajeno no conflictúa (no concurrente).
    let fifth = manager.begin();
    manager
        .stage_write(fifth, "hot-key".to_string())
        .expect("stage fifth");
    manager
        .commit(fifth)
        .expect("sin conflicto: empezó después");
}

/// AC-0055-03 — `retry_on_conflict` converge con intentos acotados.
///
/// Given: una operación que falla 2 veces con conflicto y luego tiene éxito.
/// When: se envuelve en `retry_on_conflict` con 5 intentos.
/// Then: devuelve Ok y se invocó exactamente 3 veces.
#[test]
// @spec AC-0055-03
fn test_ac_0055_03_retry_converges() {
    let mut attempts = 0;
    let result = retry_on_conflict(5, || {
        attempts += 1;
        if attempts < 3 {
            Err(RuscaError::WriteConflict {
                key: "k".to_string(),
            })
        } else {
            Ok(42)
        }
    });
    assert_eq!(result.expect("debe converger"), 42);
    assert_eq!(attempts, 3, "exactamente 3 intentos");

    // Un error NO de conflicto no se reintenta.
    let mut tries = 0;
    let fatal: Result<(), RuscaError> = retry_on_conflict(5, || {
        tries += 1;
        Err(RuscaError::InvalidConfig("fatal".to_string()))
    });
    assert!(fatal.is_err());
    assert_eq!(tries, 1, "los errores fatales no se reintentan");

    // Conflicto persistente agota los intentos y devuelve el conflicto.
    let mut endless = 0;
    let exhausted: Result<(), RuscaError> = retry_on_conflict(3, || {
        endless += 1;
        Err(RuscaError::WriteConflict {
            key: "siempre".to_string(),
        })
    });
    assert!(matches!(exhausted, Err(RuscaError::WriteConflict { .. })));
    assert_eq!(endless, 3, "intentos acotados a 3");
}

/// AC-0055-04 — bloat medido y reclamado por el reaper.
///
/// Given: versiones muertas bajo el watermark.
/// When: se mide `dead_versions` y corre `gc`.
/// Then: la métrica coincide con lo purgado y lo vivo sigue visible.
#[test]
// @spec AC-0055-04
fn test_ac_0055_04_bloat_measured_and_reaped() {
    let mut manager = TxnManager::new();
    let creator = manager.begin();
    manager.commit(creator).expect("commit creator");
    let deleter = manager.begin();
    manager.commit(deleter).expect("commit deleter");
    let live_tx = manager.begin(); // queda en vuelo
    let watermark = manager.low_watermark();
    assert_eq!(watermark, live_tx);

    let mut versions = vec![
        Version {
            created_tx: creator,
            deleted_tx: Some(deleter),
        },
        Version {
            created_tx: NO_TX,
            deleted_tx: Some(creator),
        },
        Version::new(live_tx), // viva: nunca bloat
    ];
    assert_eq!(
        dead_versions(&versions, watermark),
        2,
        "dos muertas medidas"
    );
    assert_eq!(gc(&mut versions, watermark), 2, "dos purgadas");
    assert_eq!(dead_versions(&versions, watermark), 0, "bloat a cero");
    assert_eq!(versions.len(), 1, "la viva sobrevive");
}
