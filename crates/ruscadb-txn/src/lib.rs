//! # ruscadb-txn
//!
//! Gestión transaccional de RuscaDB: **MVCC snapshot isolation** y
//! **manifiesto versionado** (entrypoint atómico del directorio de base,
//! ADR-010). Especificación: `specs/mvcc_manifest.md` (SPEC-0018). Diseño:
//! `docs/RuscaDB-roadmap.md` §5.3.
//!
//! ## Invariantes
//!
//! - **SI-1 (un commit ack'd es visible):** toda transacción cuyo
//!   [`TxnManager::commit`] devuelve `Ok` es visible para cualquier
//!   [`Snapshot`] tomado *después* del commit, y no lo es para snapshots
//!   anteriores.
//! - **SI-2 (sin dirty reads):** una transacción en vuelo nunca es visible para
//!   otra transacción; su versión solo se publica tras el commit.
//! - **Monotonía de [`TxId`]:** los identificadores se asignan estrictamente
//!   crecientes y nunca se reutilizan; `0` ([`NO_TX`]) está reservado para
//!   «sin transacción».
//! - **Manifiesto atómico:** [`Manifest::store`] escribe `tmp` + `fsync` +
//!   `rename`; un lector nunca observa un manifiesto a medias.

#![forbid(unsafe_code)]

pub mod manifest;
pub mod mvcc;

pub use manifest::{CURRENT_SCHEMA_VERSION, Manifest};
pub use mvcc::{NO_TX, Snapshot, TxId, TxnManager, Version, gc, is_obsolete};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use ruscadb_core::RuscaError;

    /// Ruta `MANIFEST.json` dentro de un directorio temporal vivo.
    fn manifest_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join("MANIFEST.json")
    }

    /// AC-0018-01 — el manifiesto sobrevive un roundtrip store→load.
    #[test] // @spec AC-0018-01
    fn test_ac_0018_01_manifest_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = manifest_path(&dir);
        let original = Manifest {
            schema_version: 1,
            epoch: 7,
            checkpoint_lsn: 42,
        };

        original.store(&path).expect("store");

        let loaded = Manifest::load(&path).expect("load");
        assert_eq!(loaded, original);
    }

    /// AC-0018-02 — `bump_epoch` incrementa en 1 y persiste en disco.
    #[test] // @spec AC-0018-02
    fn test_ac_0018_02_manifest_epoch_bump() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = manifest_path(&dir);
        let mut manifest = Manifest::new();
        assert_eq!(manifest.epoch, 0);

        manifest.bump_epoch();
        manifest.store(&path).expect("store");

        let reloaded = Manifest::load(&path).expect("load");
        assert_eq!(reloaded.epoch, 1);

        // Un segundo bump sobre lo cargado también persiste.
        let mut again = reloaded;
        again.bump_epoch();
        assert_eq!(again.epoch, 2);
    }

    /// AC-0018-03 — una versión de una tx en vuelo no es visible (sin dirty reads).
    #[test] // @spec AC-0018-03
    fn test_ac_0018_03_snapshot_hides_in_flight() {
        let mut manager = TxnManager::new();
        let writer = manager.begin();
        let _reader = manager.begin();
        let snapshot = manager.snapshot();

        let version = Version::new(writer);
        assert!(!snapshot.is_visible(&version));
    }

    /// AC-0018-04 — el commit publica la versión y el borrado la retira.
    #[test] // @spec AC-0018-04
    fn test_ac_0018_04_commit_makes_visible() {
        let mut manager = TxnManager::new();
        let writer = manager.begin();
        let version = Version::new(writer);

        // Antes del commit: invisible para el nuevo snapshot.
        let before = manager.snapshot();
        assert!(!before.is_visible(&version));

        manager.commit(writer).expect("commit");

        // Tras el commit: visible.
        let after = manager.snapshot();
        assert!(after.is_visible(&version));

        // Una versión borrada por una tx ya confirmada deja de ser visible.
        let deleter = manager.begin();
        manager.commit(deleter).expect("commit");
        let deleted = Version {
            created_tx: writer,
            deleted_tx: Some(deleter),
        };
        let later = manager.snapshot();
        assert!(!later.is_visible(&deleted));
    }

    /// AC-0018-05 — un manifiesto corrupto devuelve error, sin panics.
    #[test] // @spec AC-0018-05
    fn test_ac_0018_05_corrupt_manifest_is_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = manifest_path(&dir);

        std::fs::write(&path, b"{ esto no es json valido ").expect("write");
        let err = Manifest::load(&path).expect_err("JSON inválido debe fallar");
        assert!(matches!(err, RuscaError::CorruptManifest(_)));

        // Campos ausentes ⇒ también CorruptManifest (nunca panic).
        std::fs::write(&path, br#"{"epoch":1,"checkpoint_lsn":2}"#).expect("write");
        let err = Manifest::load(&path).expect_err("campo ausente debe fallar");
        assert!(matches!(err, RuscaError::CorruptManifest(_)));
    }

    // PBT — cualquier combinación de campos sobrevive el roundtrip.
    proptest! {
        #[test]
        fn proptest_manifest_roundtrip(
            schema_version in any::<u32>(),
            epoch in any::<u64>(),
            checkpoint_lsn in any::<u64>(),
        ) {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = manifest_path(&dir);
            let manifest = Manifest { schema_version, epoch, checkpoint_lsn };

            manifest.store(&path).expect("store");
            let loaded = Manifest::load(&path).expect("load");

            prop_assert_eq!(loaded, manifest);
        }
    }

    // PBT — ninguna tx en vuelo es visible a un snapshot (sin dirty reads).
    proptest! {
        #[test]
        fn proptest_is_visible_never_dirty_reads(count in 1usize..=8) {
            let mut manager = TxnManager::new();
            let in_flight: Vec<TxId> = (0..count).map(|_| manager.begin()).collect();
            let snapshot = manager.snapshot();

            for &tx in &in_flight {
                prop_assert!(!snapshot.is_visible(&Version::new(tx)));
            }
        }

        #[test]
        fn proptest_committed_versions_are_visible(count in 1usize..=8) {
            let mut manager = TxnManager::new();
            let txs: Vec<TxId> = (0..count).map(|_| manager.begin()).collect();
            for &tx in &txs {
                manager.commit(tx).expect("commit");
            }
            let snapshot = manager.snapshot();

            for &tx in &txs {
                prop_assert!(snapshot.is_visible(&Version::new(tx)));
            }
        }
    }

    // ── BVA (boundary value analysis) ────────────────────────────────────────

    /// `Manifest::new` fija los valores por defecto exactos.
    #[test]
    fn test_manifest_new_defaults() {
        let manifest = Manifest::new();
        assert_eq!(manifest.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.epoch, 0);
        assert_eq!(manifest.checkpoint_lsn, 0);
    }

    /// BVA epoch: mínimo (0) y máximo (`u64::MAX`) hacen roundtrip exacto.
    #[test]
    fn test_bva_manifest_epoch_min_and_max() {
        for epoch in [0u64, 1u64, u64::MAX - 1, u64::MAX] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = manifest_path(&dir);
            let manifest = Manifest {
                schema_version: 1,
                epoch,
                checkpoint_lsn: u64::MAX,
            };

            manifest.store(&path).expect("store");

            assert_eq!(Manifest::load(&path).expect("load"), manifest);
        }
    }

    /// `store` no deja el fichero temporal tras el rename atómico.
    #[test]
    fn test_manifest_store_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = manifest_path(&dir);

        Manifest::new().store(&path).expect("store");

        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(names, vec!["MANIFEST.json".to_string()]);
    }

    /// `store` puede sobrescribir un manifiesto existente (rename atómico).
    #[test]
    fn test_manifest_store_overwrites_existing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = manifest_path(&dir);
        let first = Manifest {
            schema_version: 1,
            epoch: 1,
            checkpoint_lsn: 10,
        };
        let second = Manifest {
            schema_version: 1,
            epoch: 2,
            checkpoint_lsn: 20,
        };

        first.store(&path).expect("store 1");
        second.store(&path).expect("store 2");

        assert_eq!(Manifest::load(&path).expect("load"), second);
    }

    /// BVA TxId: `begin` es monótono y arranca en 1 (0 reservado en `NO_TX`).
    #[test]
    fn test_bva_begin_is_monotonic_from_first_tx() {
        let mut manager = TxnManager::new();
        let first = manager.begin();
        let second = manager.begin();
        let third = manager.begin();

        assert_eq!(first, NO_TX + 1);
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(third, 3);
        assert!(first < second && second < third);
    }

    /// Un gestor recién creado no tiene transacciones: snapshot en `NO_TX`.
    #[test]
    fn test_bva_snapshot_of_empty_manager_is_no_tx() {
        let manager = TxnManager::new();
        assert_eq!(manager.snapshot().tx_id, NO_TX);
    }

    /// BVA `begin` + `snapshot`: `tx_id` es el último asignado.
    #[test]
    fn test_bva_snapshot_tx_id_is_last_assigned() {
        let mut manager = TxnManager::new();
        let first = manager.begin();
        assert_eq!(manager.snapshot().tx_id, first);
        let second = manager.begin();
        assert_eq!(manager.snapshot().tx_id, second);
    }

    /// Commit de una tx inexistente es error accionable.
    #[test]
    fn test_bva_commit_unknown_tx_is_error() {
        let mut manager = TxnManager::new();
        let err = manager.commit(42).expect_err("tx inexistente debe fallar");
        assert!(matches!(err, RuscaError::InvalidConfig(_)));
    }

    /// Commit duplicado de la misma tx es error (ya no está en vuelo).
    #[test]
    fn test_bva_commit_twice_is_error() {
        let mut manager = TxnManager::new();
        let tx = manager.begin();
        manager.commit(tx).expect("primer commit");

        let err = manager.commit(tx).expect_err("segundo commit debe fallar");
        assert!(matches!(err, RuscaError::InvalidConfig(_)));
    }

    /// SPEC-0027 — `rollback` aborta: la tx sale de vuelo y nunca se confirma.
    #[test]
    fn test_txn_manager_rollback_aborts_in_flight() {
        let mut manager = TxnManager::new();
        let tx = manager.begin();
        assert_eq!(manager.low_watermark(), tx, "la tx bloquea el watermark");

        manager.rollback(tx).expect("rollback de tx en vuelo");

        assert!(!manager.is_committed(tx), "una tx abortada no se publica");
        assert_eq!(
            manager.low_watermark(),
            tx + 1,
            "al salir de vuelo deja de bloquear el watermark"
        );
        let err = manager
            .rollback(tx)
            .expect_err("doble rollback debe fallar");
        assert!(matches!(err, RuscaError::InvalidConfig(_)));
    }

    /// SPEC-0027 — `rollback` de una tx inexistente es error accionable.
    #[test]
    fn test_txn_manager_rollback_unknown_tx_is_error() {
        let mut manager = TxnManager::new();
        let err = manager
            .rollback(42)
            .expect_err("tx inexistente debe fallar");
        assert!(matches!(err, RuscaError::InvalidConfig(_)));
        assert!(err.to_string().contains("42"));
    }

    /// SPEC-0027 — tras `commit`, `rollback` de la misma tx falla (ya no está
    /// en vuelo) y no la despublica.
    #[test]
    fn test_txn_manager_rollback_after_commit_is_error() {
        let mut manager = TxnManager::new();
        let tx = manager.begin();
        manager.commit(tx).expect("commit");

        let err = manager
            .rollback(tx)
            .expect_err("rollback de tx confirmada debe fallar");
        assert!(matches!(err, RuscaError::InvalidConfig(_)));
        assert!(manager.is_committed(tx), "el commit previo se conserva");
    }

    /// BVA `is_committed`: antes, durante y después; 0 y desconocidos dan `false`.
    #[test]
    fn test_bva_is_committed_boundaries() {
        let mut manager = TxnManager::new();
        assert!(!manager.is_committed(NO_TX));

        let tx = manager.begin();
        assert!(!manager.is_committed(tx));
        assert!(!manager.is_committed(u64::MAX));

        manager.commit(tx).expect("commit");

        assert!(manager.is_committed(tx));
        assert!(!manager.is_committed(NO_TX));
    }

    /// `Version::new` no marca borrado.
    #[test]
    fn test_version_new_has_no_deleter() {
        let version = Version::new(5);
        assert_eq!(version.created_tx, 5);
        assert_eq!(version.deleted_tx, None);
    }

    /// `Version` serializa/deserializa (contrato serde de SPEC-0018).
    #[test]
    fn test_version_serde_roundtrip() {
        let version = Version {
            created_tx: 3,
            deleted_tx: Some(9),
        };
        let json = serde_json::to_string(&version).expect("serializa");
        let back: Version = serde_json::from_str(&json).expect("deserializa");
        assert_eq!(back, version);
    }

    /// BVA visibilidad: `created_tx == tx_id` es visible (frontera inclusiva).
    #[test]
    fn test_bva_version_created_at_snapshot_is_visible() {
        let mut manager = TxnManager::new();
        let tx = manager.begin();
        manager.commit(tx).expect("commit");
        let snapshot = manager.snapshot();

        assert_eq!(snapshot.tx_id, tx);
        assert!(snapshot.is_visible(&Version::new(tx)));
    }

    /// BVA visibilidad: `created_tx == NO_TX` (bootstrap) es visible.
    #[test]
    fn test_bva_version_created_at_no_tx_is_visible() {
        let snapshot = TxnManager::new().snapshot();
        assert!(snapshot.is_visible(&Version::new(NO_TX)));
    }

    /// BVA visibilidad: una versión de una tx posterior no es visible.
    #[test]
    fn test_bva_version_above_snapshot_is_invisible() {
        let mut manager = TxnManager::new();
        let tx = manager.begin();
        manager.commit(tx).expect("commit");
        let snapshot = manager.snapshot();

        assert!(!snapshot.is_visible(&Version::new(tx + 1)));
    }

    /// BVA borrado: borrador en vuelo no oculta la versión.
    #[test]
    fn test_bva_deleter_in_flight_keeps_visible() {
        let mut manager = TxnManager::new();
        let creator = manager.begin();
        manager.commit(creator).expect("commit");
        let deleter = manager.begin(); // en vuelo
        let snapshot = manager.snapshot();

        let version = Version {
            created_tx: creator,
            deleted_tx: Some(deleter),
        };
        assert!(snapshot.is_visible(&version));
    }

    /// BVA borrado: borrador posterior al snapshot no oculta la versión.
    #[test]
    fn test_bva_deleter_above_snapshot_keeps_visible() {
        let mut manager = TxnManager::new();
        let creator = manager.begin();
        manager.commit(creator).expect("commit");
        let snapshot = manager.snapshot();

        let version = Version {
            created_tx: creator,
            deleted_tx: Some(snapshot.tx_id + 1),
        };
        assert!(snapshot.is_visible(&version));
    }

    /// BVA borrado: `deleted_tx == tx_id` confirmado oculta la versión.
    #[test]
    fn test_bva_deleter_at_snapshot_hides_version() {
        let mut manager = TxnManager::new();
        let creator = manager.begin();
        manager.commit(creator).expect("commit");
        let deleter = manager.begin();
        manager.commit(deleter).expect("commit");
        let snapshot = manager.snapshot();

        assert_eq!(snapshot.tx_id, deleter);
        let version = Version {
            created_tx: creator,
            deleted_tx: Some(deleter),
        };
        assert!(!snapshot.is_visible(&version));
    }

    /// SI: un snapshot congela su visibilidad; commits posteriores no la cambian.
    #[test]
    fn test_snapshot_is_frozen_after_later_commit() {
        let mut manager = TxnManager::new();
        let writer = manager.begin();
        let snapshot = manager.snapshot();

        manager.commit(writer).expect("commit");

        assert!(!snapshot.is_visible(&Version::new(writer)));
    }

    /// `Manifest::load` de una ruta inexistente propaga el error de E/S.
    #[test]
    fn test_manifest_load_missing_file_is_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-existe.json");

        let err = Manifest::load(&path).expect_err("fichero ausente debe fallar");
        assert!(matches!(err, RuscaError::Io(_)));
    }

    // ── SPEC-0025 — reaper MVCC (low watermark + GC de versiones) ────────────

    /// AC-0025-01 — sin transacciones en vuelo, el watermark es el próximo `TxId`.
    #[test] // @spec AC-0025-01
    fn test_ac_0025_01_watermark_without_in_flight() {
        let mut manager = TxnManager::new();
        // Gestor recién creado: no hay nada en vuelo ⇒ primer TxId asignable.
        assert_eq!(manager.low_watermark(), NO_TX + 1);

        let first = manager.begin();
        let second = manager.begin();
        manager.commit(first).expect("commit 1");
        manager.commit(second).expect("commit 2");

        // Todas confirmadas ⇒ watermark = siguiente TxId a asignar.
        assert_eq!(manager.low_watermark(), second + 1);
        assert_eq!(manager.low_watermark(), manager.snapshot().tx_id + 1);
    }

    /// AC-0025-02 — con transacciones en vuelo, devuelve la menor de ellas.
    #[test] // @spec AC-0025-02
    fn test_ac_0025_02_watermark_with_in_flight() {
        let mut manager = TxnManager::new();
        let first = manager.begin();
        let second = manager.begin();
        let third = manager.begin();

        assert_eq!(manager.low_watermark(), first);

        manager.commit(first).expect("commit first");
        assert_eq!(manager.low_watermark(), second);

        manager.commit(second).expect("commit second");
        assert_eq!(manager.low_watermark(), third);
    }

    /// AC-0025-03 — `gc` purga las versiones borradas por debajo del watermark.
    #[test] // @spec AC-0025-03
    fn test_ac_0025_03_gc_purges_obsolete() {
        let mut manager = TxnManager::new();
        let creator = manager.begin();
        manager.commit(creator).expect("commit creator");
        let deleter = manager.begin();
        manager.commit(deleter).expect("commit deleter");
        let live_tx = manager.begin(); // queda en vuelo
        let watermark = manager.low_watermark();
        assert_eq!(watermark, live_tx);

        // Ambas borradas por txs confirmadas por debajo del watermark.
        let obsolete_a = Version {
            created_tx: creator,
            deleted_tx: Some(deleter),
        };
        let obsolete_b = Version {
            created_tx: NO_TX,
            deleted_tx: Some(creator),
        };
        let mut versions = vec![obsolete_a, obsolete_b];

        let removed = gc(&mut versions, watermark);

        assert_eq!(removed, 2);
        assert!(versions.is_empty());
    }

    /// AC-0025-04 — `gc` conserva las vivas y las borradas a/por encima.
    #[test] // @spec AC-0025-04
    fn test_ac_0025_04_gc_keeps_live_and_recent() {
        let watermark = 5;
        let live = Version::new(3);
        let deleted_at = Version {
            created_tx: 2,
            deleted_tx: Some(watermark), // frontera: aún visible
        };
        let deleted_above = Version {
            created_tx: 2,
            deleted_tx: Some(watermark + 1),
        };
        let mut versions = vec![live, deleted_at, deleted_above];
        let original = versions.clone();

        let removed = gc(&mut versions, watermark);

        assert_eq!(removed, 0);
        assert_eq!(versions, original);
    }

    /// AC-0025-05 — `gc` es no-op con vacío e idempotente (sin pánicos).
    #[test] // @spec AC-0025-05
    fn test_ac_0025_05_gc_is_idempotent() {
        // Conjunto vacío: siempre 0, sin pánicos en los extremos del watermark.
        let mut empty: Vec<Version> = Vec::new();
        assert_eq!(gc(&mut empty, NO_TX), 0);
        assert_eq!(gc(&mut empty, u64::MAX), 0);

        // Segunda pasada sobre un conjunto ya purgado no elimina nada.
        let mut manager = TxnManager::new();
        let creator = manager.begin();
        manager.commit(creator).expect("commit creator");
        let deleter = manager.begin();
        manager.commit(deleter).expect("commit deleter");
        let live_tx = manager.begin();
        let watermark = manager.low_watermark();

        let mut versions = vec![
            Version {
                created_tx: creator,
                deleted_tx: Some(deleter),
            },
            Version::new(live_tx),
        ];

        assert_eq!(gc(&mut versions, watermark), 1);
        assert_eq!(gc(&mut versions, watermark), 0);
    }

    // ── BVA `is_obsolete` (fronteras del watermark) ──────────────────────────

    /// BVA: `watermark = 0` nunca hace obsoleta ninguna versión (unsigned).
    #[test]
    fn test_bva_is_obsolete_watermark_zero_never_obsolete() {
        let version = Version {
            created_tx: NO_TX,
            deleted_tx: Some(NO_TX),
        };
        assert!(!is_obsolete(&version, NO_TX));
    }

    /// BVA: `deleted_tx == watermark` se conserva; por debajo sí es obsoleta.
    #[test]
    fn test_bva_is_obsolete_at_watermark_is_kept() {
        let version = Version {
            created_tx: 1,
            deleted_tx: Some(7),
        };
        assert!(!is_obsolete(&version, 7)); // igual: aún visible
        assert!(is_obsolete(&version, 8)); // por debajo: obsoleta
    }

    /// BVA: una versión viva (`deleted_tx == None`) nunca es obsoleta.
    #[test]
    fn test_bva_is_obsolete_live_is_never_obsolete() {
        assert!(!is_obsolete(&Version::new(3), u64::MAX));
    }

    // PBT — NF-0025-01: `gc` nunca purga una versión visible a un snapshot
    // cuyo `tx_id >= watermark`.
    proptest! {
        #[test]
        fn proptest_ac_0025_gc_never_purges_visible(
            committed in 0usize..=6,
            in_flight in 1usize..=4,
            deletions in proptest::collection::vec(any::<Option<usize>>(), 0..=8),
        ) {
            let mut manager = TxnManager::new();
            let mut committed_txs: Vec<TxId> = Vec::new();
            for _ in 0..committed {
                let tx = manager.begin();
                manager.commit(tx).expect("commit");
                committed_txs.push(tx);
            }
            let mut flight_txs: Vec<TxId> = Vec::new();
            for _ in 0..in_flight {
                flight_txs.push(manager.begin());
            }

            let watermark = manager.low_watermark();
            let snapshot = manager.snapshot();
            // Con transacciones en vuelo, el snapshot es >= watermark por construcción.
            prop_assert!(snapshot.tx_id >= watermark);

            let candidates: Vec<TxId> = committed_txs
                .iter()
                .chain(flight_txs.iter())
                .copied()
                .collect();

            let mut versions: Vec<Version> = committed_txs
                .iter()
                .copied()
                .map(Version::new)
                .collect();
            for (index, deletion) in deletions.iter().enumerate() {
                let creator = candidates[index % candidates.len()];
                let deleter = candidates[deletion.unwrap_or(0) % candidates.len()];
                versions.push(Version {
                    created_tx: creator,
                    deleted_tx: Some(deleter),
                });
            }

            let before = versions.clone();
            let removed = gc(&mut versions, watermark);
            prop_assert_eq!(removed, before.len() - versions.len());

            for version in &before {
                if snapshot.is_visible(version) {
                    prop_assert!(
                        versions.contains(version),
                        "gc purgó una versión visible a un snapshot >= watermark"
                    );
                }
            }
        }
    }
}
