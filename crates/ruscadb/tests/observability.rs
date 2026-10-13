//! Observabilidad de frontera (SPEC-0080): spans de `tracing` y métricas
//! opcionales.
//!
//! Verifica los tres criterios de aceptación: los cuatro spans se emiten con
//! sus nombres exactos (`commit`, `checkpoint`, `page_fault`, `lock_wait`), la
//! feature `metrics` (ON) registra la latencia de commit y el camino por
//! defecto (OFF) corre limpio. El collector es un [`tracing::Subscriber`]
//! mínimo escrito a mano (sin `tracing-subscriber`): solo captura nombres de
//! span, que es justo lo que exige el AC.

// El oráculo de integración puede fallar ruidosamente (equivalente a
// `allow-expect-in-tests` del workspace, que no alcanza a targets de
// integración).
#![allow(clippy::expect_used)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ruscadb::{Database, DbConfig, Page, PageId};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::with_default;
use tracing::{Event, Metadata};

/// Collector local que registra el nombre de cada span creado.
///
/// Implementación mínima de [`tracing::Subscriber`]: solo `new_span` hace
/// trabajo (guardar el nombre); el resto de la API es no-op. Se comparte por
/// `Arc` para inspeccionarlo una vez terminado el bloque `with_default`.
#[derive(Clone, Default)]
struct SpanNames {
    /// Nombres de los spans creados, en orden de aparición.
    names: Arc<Mutex<Vec<String>>>,
    /// Contador monótono de ids de span (`Id::from_u64` exige id > 0).
    next_id: Arc<AtomicU64>,
}

impl SpanNames {
    /// Devuelve una copia de los nombres de span capturados.
    ///
    /// Returns:
    ///     Los nombres en orden de creación; vacío si el mutex quedó envenenado.
    fn captured(&self) -> Vec<String> {
        self.names
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl tracing::Subscriber for SpanNames {
    /// Habilita todos los niveles: el collector de test captura todo.
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    /// Registra el nombre del span y devuelve un id único no nulo.
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        if let Ok(mut names) = self.names.lock() {
            names.push(attributes.metadata().name().to_string());
        }
        Id::from_u64(self.next_id.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// No-op: el collector no captura campos del span.
    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    /// No-op: el collector no modela relaciones causales.
    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    /// No-op: el collector no captura eventos.
    fn event(&self, _event: &Event<'_>) {}

    /// No-op: la entrada al span no aporta al nombre.
    fn enter(&self, _span: &Id) {}

    /// No-op: la salida del span no aporta al nombre.
    fn exit(&self, _span: &Id) {}
}

/// Escribe la página 0 con un marcador y confirma (commit real).
///
/// Args:
///     database: Base abierta donde escribir.
///
/// Returns:
///     El `Lsn` del commit.
fn write_and_commit(database: &mut Database) -> ruscadb_wal::Lsn {
    let mut page = Page::new(PageId(0));
    page.data_mut()[0] = 0x42;
    database.write_page(&page).expect("write_page");
    database.commit().expect("commit")
}

/// AC-0080-01 — los cuatro spans de frontera se emiten con sus nombres.
///
/// Ejecuta commit, checkpoint del WAL, un page fault (lectura no cacheada) y
/// una adquisición de lock bajo un collector local, y comprueba que aparecen
/// los 4 nombres exactos.
#[test]
// @spec AC-0080-01
fn test_ac_0080_01_core_spans_are_emitted() {
    let directory = tempfile::tempdir().expect("directorio temporal");
    let data_path = directory.path().join("db.data");
    let collector = SpanNames::default();

    with_default(collector.clone(), || {
        // lock_wait: `Database::open` adquiere el lock cross-proceso.
        let mut database = Database::open(DbConfig::new(&data_path, 4)).expect("open");
        // commit: operación de commit real con una página sucia.
        write_and_commit(&mut database);
        drop(database); // libera el lock para poder reabrir
        // checkpoint: checkpoint real del WAL (registro + fsync).
        let mut wal = ruscadb_wal::Wal::open(data_path.with_extension("wal")).expect("wal");
        wal.checkpoint().expect("checkpoint");
        drop(wal);
        // page_fault: al reabrir el pool está vacío y la página se relee.
        let mut database = Database::open(DbConfig::new(&data_path, 4)).expect("reopen");
        database.read_page(PageId(0)).expect("read");
    });

    let names = collector.captured();
    for expected in ["commit", "checkpoint", "page_fault", "lock_wait"] {
        assert!(
            names.iter().any(|name| name == expected),
            "falta el span '{expected}'; capturados: {names:?}"
        );
    }
}

/// AC-0080-02 — con `metrics` ON, un commit registra su latencia.
///
/// Instala un `DebuggingRecorder` local, ejecuta un commit y comprueba que la
/// métrica `ruscadb_commit_latency_seconds` quedó registrada con muestras.
#[cfg(feature = "metrics")]
#[test]
// @spec AC-0080-02
fn test_ac_0080_02_metrics_on_records_commit_latency() {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let directory = tempfile::tempdir().expect("directorio temporal");
    let data_path = directory.path().join("db.data");
    // El collector local también evita que otra prueba registre los callsites
    // sin subscriber (el interés de `tracing` se cachea globalmente).
    let collector = SpanNames::default();

    metrics::with_local_recorder(&recorder, || {
        with_default(collector, || {
            let mut database = Database::open(DbConfig::new(&data_path, 4)).expect("open");
            write_and_commit(&mut database);
        });
    });

    let recorded =
        snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .any(|(key, _unit, _description, value)| {
                key.key().name() == "ruscadb_commit_latency_seconds"
                    && matches!(value, DebugValue::Histogram(ref samples) if !samples.is_empty())
            });
    assert!(
        recorded,
        "la métrica de latencia de commit debe quedar registrada con muestras"
    );
}

/// AC-0080-03 — por defecto (`metrics` OFF) la misma operación corre limpia.
///
/// Sin la feature no hay registro de métricas: el commit avanza el LSN y la
/// página persiste sin instrumentación.
#[cfg(not(feature = "metrics"))]
#[test]
// @spec AC-0080-03
fn test_ac_0080_03_metrics_off_is_clean_default() {
    let directory = tempfile::tempdir().expect("directorio temporal");
    let data_path = directory.path().join("db.data");
    // El collector local evita que otra prueba registre los callsites sin
    // subscriber (el interés de `tracing` se cachea globalmente).
    let collector = SpanNames::default();
    with_default(collector, || {
        let mut database = Database::open(DbConfig::new(&data_path, 4)).expect("open");
        let lsn = write_and_commit(&mut database);
        assert_eq!(
            database.last_lsn(),
            lsn,
            "el commit publica su LSN sin métricas"
        );
        let page = database.read_page(PageId(0)).expect("read");
        assert_eq!(
            page.data()[0],
            0x42,
            "la página persiste sin instrumentación"
        );
    });
}
