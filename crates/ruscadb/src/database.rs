//! Motor de almacenamiento durable: composition root de RuscaDB.
//!
//! Combina `PagedFile` + `BufferPool` + `Wal` con política **WAL-first**:
//! el commit hace `fsync` del WAL antes de publicar páginas, y el arranque
//! ejecuta un replay idempotente de los commit records.
//!
//! Ver `specs/durable_engine.md` (SPEC-0004).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use ruscadb_ai::ModelRegistry;
use ruscadb_btree::BPlusTree;
use ruscadb_core::{RecordId, RuscaError};
use ruscadb_multimodal::BlobStore;
use ruscadb_storage::{BufferPool, PAGE_SIZE, Page, PageId, PagedFile};
use ruscadb_txn::{Manifest, Snapshot, TxId, TxnManager};
use ruscadb_wal::{FsyncPolicy, Lsn, RecordKind, Wal};

use crate::auth::{AuditEntry, Role};
use crate::blob_pipeline::BlobPipeline;
use crate::cache::{CostReport, SemanticCache};
use crate::encryption::EncryptionConfig;
use crate::gates::IngestionPolicy;
use crate::hardening::ShadowReport;
use crate::heap::RowLocator;
use crate::indexes::TableIndexes;
use crate::lock::LockGuard;

const COUNT_SIZE: usize = 4;
const ID_SIZE: usize = 8;

/// Configuración de apertura de una base RuscaDB.
#[derive(Clone, Debug)]
pub struct DbConfig {
    /// Ruta del archivo de páginas (el WAL se deriva con extensión `.wal`).
    pub data_path: PathBuf,
    /// Número de marcos del buffer pool (presupuesto = `pool_capacity * 4 KiB`).
    pub pool_capacity: usize,
    /// Cifrado en reposo (`None` = modo claro). Ver [`EncryptionConfig`].
    pub encryption: Option<EncryptionConfig>,
    /// Directorio raíz del blob store (`None` = sin blobs, SPEC-0038).
    ///
    /// Si es `Some`, [`Database::open`] abre (o crea) un [`BlobStore`] en esa
    /// ruta. El cifrado del blob store queda fuera de alcance en esta iteración:
    /// el store se abre siempre en modo claro.
    pub blob_path: Option<PathBuf>,
    /// Política de fsync del WAL (SPEC-0081). Por defecto [`FsyncPolicy::Full`]
    /// (fsync por commit, comportamiento histórico).
    ///
    /// Con [`FsyncPolicy::Normal`] el commit espera el ack del grupo de
    /// group commit antes de publicar las páginas.
    pub fsync_policy: FsyncPolicy,
}

impl DbConfig {
    /// Crea una configuración de apertura (sin cifrado ni blob store).
    ///
    /// La política de fsync por defecto es [`FsyncPolicy::Full`] (idéntica al
    /// comportamiento histórico, SPEC-0081, NF-0081-03).
    ///
    /// Args:
    ///     data_path: Ruta del archivo de páginas.
    ///     pool_capacity: Número de marcos del buffer pool (>= 1).
    pub fn new(data_path: impl Into<PathBuf>, pool_capacity: usize) -> Self {
        Self {
            data_path: data_path.into(),
            pool_capacity,
            encryption: None,
            blob_path: None,
            fsync_policy: FsyncPolicy::Full,
        }
    }
}

/// Base de datos embebida con durabilidad WAL-first.
pub struct Database {
    file: PagedFile,
    /// Buffer pool de páginas (SPEC-0003). `pub(crate)` para que el rollback de
    /// la fachada y sus tests puedan inspeccionar/descartar marcos sucios.
    pub(crate) pool: BufferPool,
    pub(crate) wal: Wal,
    wal_path: PathBuf,
    encryption: Option<EncryptionConfig>,
    /// Índices derivados por tabla (HNSW/CSR/invertido), SPEC-0017.
    pub(crate) indexes: BTreeMap<String, TableIndexes>,
    /// Índice primario en memoria `RecordId -> RowLocator` por tabla, SPEC-0024.
    ///
    /// Se puebla en cada inserción y se reconstruye desde el heap al abrir la
    /// base; `Database::delete` lo usa para localizar la fila en `O(log n)`.
    pub(crate) primary: BTreeMap<String, BPlusTree<RecordId, RowLocator>>,
    /// Manifiesto versionado (`schema_version`, `epoch`, `checkpoint_lsn`), SPEC-0019.
    manifest: Manifest,
    /// Ruta del fichero `MANIFEST.json` derivada de la ruta de datos.
    manifest_path: PathBuf,
    /// Gestor MVCC de transacciones y snapshots, SPEC-0019.
    pub(crate) txn: TxnManager,
    /// Transacción explícita en vuelo (`Database::begin`), si la hay.
    pub(crate) active_tx: Option<TxId>,
    /// Allowlist en memoria de modelos de embedding (SPEC-0032).
    ///
    /// Vacío por defecto ⇒ compatibilidad total con el comportamiento previo
    /// (NF-0032-01). No se persiste: se reconstruye vacío al reabrir la base
    /// (la persistencia queda fuera de alcance).
    pub(crate) registry: ModelRegistry,
    /// Último LSN confirmado (checkpoint en memoria).
    last_lsn: Lsn,
    /// Overlay WAL en modo cifrado (SPEC-0106): `page_id -> (lsn, offset)`.
    ///
    /// Apunta al commit record que contiene la última imagen confirmada de
    /// cada página. Se construye en [`Database::open`] (recuperación perezosa,
    /// patrón wal-index de SQLite) y se actualiza en cada [`Database::commit`];
    /// vacío en modo claro.
    pub(crate) wal_overlay: HashMap<PageId, (Lsn, u64)>,
    /// Blob store content-addressed abierto junto a la base (SPEC-0038).
    ///
    /// `None` cuando `DbConfig::blob_path` no se fijó: las operaciones de blob
    /// devuelven un error accionable (no configurado).
    pub(crate) blobs: Option<BlobStore>,
    /// Shadow reads habilitados (SPEC-0062): OFF por defecto (coste nulo).
    pub(crate) shadow_enabled: bool,
    /// Acumulador del modo shadow (SPEC-0062): verificación de índices en vivo.
    pub(crate) shadow: ShadowReport,
    /// Política de ingesta configurable (SPEC-0064).
    pub(crate) policy: IngestionPolicy,
    /// Ruta del registro de modelos persistible `<data>.models.json` (SPEC-0064).
    pub(crate) models_path: PathBuf,
    /// Caché semántico de KNN (SPEC-0065): OFF por defecto.
    pub(crate) semantic_cache: SemanticCache,
    /// Coste por query acumulado (SPEC-0065).
    pub(crate) cost_report: CostReport,
    /// Rol de instancia para RBAC (SPEC-0066): Admin por defecto.
    pub(crate) role: Role,
    /// Captura de auditoría habilitada (SPEC-0066): OFF por defecto.
    pub(crate) audit_enabled: bool,
    /// Trail de auditoría en memoria (SPEC-0066).
    pub(crate) audit: Vec<AuditEntry>,
    /// Base de reloj monótono del audit (SPEC-0070).
    pub(crate) audit_epoch: std::time::Instant,
    /// Último timestamp emitido en el audit (SPEC-0070).
    pub(crate) audit_last_nanos: u64,
    /// Ruta del audit persistido `<data>.audit.jsonl` (SPEC-0073).
    pub(crate) audit_path: PathBuf,
    /// Ruta del caché semántico persistido `<data>.cache.json` (SPEC-0093).
    pub(crate) cache_path: PathBuf,
    /// Pipeline de blobs (SPEC-0067): cola FIFO de análisis.
    pub(crate) blob_pipeline: BlobPipeline,
    /// Guard RAII del lock cross-proceso `<data>.lock` (SPEC-0076).
    ///
    /// Mantiene el lock exclusivo no-bloqueante mientras la `Database` viva; al
    /// dropearse la base el lock se libera y la ruta queda reabrible. Se
    /// adquiere en [`Database::open`] **antes** de tocar el fichero de datos.
    _lock: Option<LockGuard>,
    /// Modo solo lectura (SPEC-0109): sin lock exclusivo, escrituras rechazadas.
    read_only: bool,
}

impl Database {
    /// Abre (o crea) la base y ejecuta el recovery antes de servir I/O.
    ///
    /// Si `config.encryption` está presente, el WAL se abre con
    /// `Wal::open_encrypted` y el replay descifra con esa clave (SPEC-0013).
    /// Carga (o crea) el manifiesto `<data>.manifest.json` y valida su
    /// `schema_version` (SPEC-0019).
    ///
    /// Si `config.blob_path` es `Some`, abre (o crea) un [`BlobStore`] en esa
    /// ruta para `put_blob`/`get_blob`/`gc_blobs` (SPEC-0038). El blob store se
    /// abre siempre en modo claro: **el cifrado del blob store queda fuera de
    /// alcance** de esta iteración (se cableará con `open_encrypted` después).
    ///
    /// Adquiere un lock exclusivo no-bloqueante sobre el sidecar `<data>.lock`
    /// (SPEC-0076) **antes** de tocar el fichero de datos; el lock se libera al
    /// dropearse la `Database`.
    ///
    /// La política de fsync del WAL la fija [`DbConfig::fsync_policy`]
    /// (SPEC-0081): [`FsyncPolicy::Full`] (default) fsync-ea por commit;
    /// [`FsyncPolicy::Normal`] agrupa los commits en un writer thread.
    ///
    /// Restaura el caché semántico persistido `<data>.cache.json` (SPEC-0093)
    /// de forma *best-effort*: ausente, corrupto o con versión desconocida deja
    /// el caché vacío sin impedir la apertura; en bases cifradas no se carga.
    ///
    /// Args:
    ///     config: Configuración de apertura.
    ///
    /// Returns:
    ///     La base lista para operar.
    ///
    /// Errors:
    ///     [`RuscaError::DatabaseInUse`] si otra instancia ya tiene abierta la
    ///     misma base (lock `<data>.lock` ocupado);
    ///     [`RuscaError::Io`] si falla el acceso a disco;
    ///     [`RuscaError::InvalidConfig`] si `pool_capacity == 0`;
    ///     [`RuscaError::CorruptManifest`] si el manifiesto es inválido o tiene
    ///     una `schema_version` desconocida;
    ///     [`RuscaError::WalCorrupt`] si la clave falta o no autentica.
    /// Abre la base en modo lectura/escritura (adquiere el lock exclusivo).
    pub fn open(config: DbConfig) -> Result<Self, RuscaError> {
        Self::open_with(config, false)
    }

    /// Abre la base en modo solo lectura (sin lock exclusivo, SPEC-0109).
    ///
    /// Permite a varios procesos leer la misma base concurrentemente mientras
    /// un único proceso la tiene abierta en modo write. Las operaciones de
    /// escritura (`insert`, `update`, `delete`, `create_table`, ...) devuelven
    /// [`RuscaError::ReadOnly`].
    pub fn open_readonly(config: DbConfig) -> Result<Self, RuscaError> {
        Self::open_with(config, true)
    }

    /// Indica si la base se abrió en modo solo lectura (SPEC-0109).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Rechaza una operación de escritura en modo solo lectura (SPEC-0109).
    pub fn ensure_writable(&self, operation: &str) -> Result<(), RuscaError> {
        if self.read_only {
            return Err(RuscaError::ReadOnly {
                operation: operation.to_string(),
            });
        }
        Ok(())
    }

    /// Implementación compartida de apertura.
    fn open_with(config: DbConfig, read_only: bool) -> Result<Self, RuscaError> {
        let wal_path = wal_path_for(&config.data_path);
        let manifest_path = manifest_path_for(&config.data_path);
        // Lock cross-proceso (SPEC-0076): solo en modo write. En modo solo
        // lectura no se adquiere (permite lectores concurrentes, SPEC-0109).
        let lock = if read_only {
            None
        } else {
            Some(LockGuard::acquire(&config.data_path)?)
        };
        let file = PagedFile::open(&config.data_path)?;
        let wal = match config.encryption.as_ref() {
            Some(encryption) => {
                Wal::open_encrypted_with(&wal_path, encryption.key(), config.fsync_policy)?
            }
            None => Wal::open_with(&wal_path, config.fsync_policy)?,
        };
        let (manifest, is_new) = load_or_create_manifest(&manifest_path)?;
        // Blob store opcional (SPEC-0038): se abre en claro (el cifrado del
        // blob store queda fuera de alcance en esta iteración).
        let blobs = match config.blob_path.as_ref() {
            Some(blob_path) => Some(BlobStore::open(blob_path)?),
            None => None,
        };
        let mut database = Self {
            file,
            pool: BufferPool::new(config.pool_capacity)?,
            wal,
            wal_path,
            encryption: config.encryption,
            indexes: BTreeMap::new(),
            primary: BTreeMap::new(),
            manifest,
            manifest_path,
            txn: if read_only {
                TxnManager::with_next_tx(u64::MAX - 1)
            } else {
                TxnManager::new()
            },
            active_tx: None,
            registry: ModelRegistry::new(),
            last_lsn: 0,
            wal_overlay: HashMap::new(),
            blobs,
            shadow_enabled: false,
            shadow: ShadowReport::default(),
            policy: IngestionPolicy::default(),
            models_path: config.data_path.with_extension("models.json"),
            semantic_cache: SemanticCache::default(),
            cost_report: CostReport::default(),
            role: Role::default(),
            audit_enabled: false,
            audit: Vec::new(),
            audit_epoch: std::time::Instant::now(),
            audit_last_nanos: 0,
            audit_path: config.data_path.with_extension("audit.jsonl"),
            cache_path: config.data_path.with_extension("cache.json"),
            blob_pipeline: BlobPipeline::default(),
            _lock: lock,
            read_only,
        };
        database.restore_registry()?;
        database.restore_audit()?;
        // SPEC-0106: en cifrado no se precarga el pool (recuperación perezosa,
        // patrón wal-index de SQLite): solo se construye el overlay que apunta
        // al commit record de cada página. En claro se sigue aplicando el
        // replay idempotente sobre el `.data` (SPEC-0004).
        let applied = if database.encryption.is_some() {
            database.build_overlay()?
        } else {
            database.replay()?
        };
        database.last_lsn = applied;
        database.manifest.checkpoint_lsn = applied;
        if is_new {
            database.manifest.store(&database.manifest_path)?;
        }
        database.rebuild_indexes()?;
        database.rebuild_primary_index()?;
        database.restore_txn_watermark()?;
        // Caché semántico persistido (SPEC-0093): best-effort; un sidecar
        // ausente/corrupto no impide abrir la base.
        database.restore_semantic_cache();
        Ok(database)
    }

    /// Indica si una página existe en el pool, en el archivo de datos o en el
    /// overlay del WAL (SPEC-0106).
    ///
    /// Evita intentar leer una página inexistente: el buffer pool reserva un
    /// marco antes de fallar, lo que con pools pequeños agotaría la capacidad.
    /// En modo cifrado el `.data` no crece, así que las páginas confirmadas
    /// viven en el overlay y deben contar como existentes.
    ///
    /// Args:
    ///     id: Página consultada.
    ///
    /// Returns:
    ///     `true` si la página está en memoria, en el overlay o dentro del archivo.
    pub(crate) fn page_exists(&self, id: PageId) -> bool {
        self.pool.contains(id)
            || id.0 < self.file.page_count()
            || self.wal_overlay.contains_key(&id)
    }

    /// Lee una página con resolución de tres vías (SPEC-0106).
    ///
    /// Orden: pool → overlay del WAL (materialización dirigida al commit
    /// record, modo cifrado) → `.data` (páginas pre-cifrado). Sin marco, sin
    /// overlay y fuera del archivo responde [`RuscaError::PageOutOfRange`].
    ///
    /// Errors:
    ///     [`RuscaError::PageOutOfRange`] si el id no existe;
    ///     [`RuscaError::WalCorrupt`] si el overlay apunta a un record que no
    ///     decodifica o no contiene la página (modo cifrado).
    pub fn read_page(&mut self, id: PageId) -> Result<Page, RuscaError> {
        // Despacho del miss por campos disjuntos (mismo patrón que el loader
        // de `.data`): el overlay y el WAL se leen sin mutar el pool.
        let file = &mut self.file;
        let overlay = &self.wal_overlay;
        let wal_path = &self.wal_path;
        let key = self.encryption.as_ref().map(|encryption| *encryption.key());
        let page = self.pool.get(id, |pid| match overlay.get(&pid) {
            Some((lsn, offset)) => {
                let record = ruscadb_wal::read_record_at(wal_path, key.as_ref(), *offset)?;
                if record.lsn != *lsn || record.kind != RecordKind::Commit {
                    return Err(RuscaError::WalCorrupt(format!(
                        "overlay desalineado: se esperaba el commit LSN {lsn} en el offset {offset}"
                    )));
                }
                materialize_commit_page(pid, &record.payload)
            }
            None => file.read_page(pid),
        })?;
        let cloned = page.clone();
        self.pool.unpin(id, false)?;
        Ok(cloned)
    }

    /// Escribe una página (sucia en el pool; no visible en disco hasta commit).
    ///
    /// Errors:
    ///     [`RuscaError::BufferPoolFull`] si no hay marcos desalojables.
    pub fn write_page(&mut self, page: &Page) -> Result<(), RuscaError> {
        let id = page.id();
        {
            let target = self
                .pool
                .get_mut(id, |pid| match self.file.read_page(pid) {
                    Ok(existing) => Ok(existing),
                    Err(RuscaError::PageOutOfRange { .. }) => Ok(Page::new(pid)),
                    Err(other) => Err(other),
                })?;
            target.data_mut().copy_from_slice(page.data());
        }
        self.pool.unpin(id, true)?;
        Ok(())
    }

    /// Confirma los cambios: WAL-first (append + fsync) y luego publica páginas.
    ///
    /// Si había páginas sucias, incrementa el `epoch` del manifiesto, fija
    /// `checkpoint_lsn` al LSN del commit y persiste el manifiesto de forma
    /// atómica. Si hay una transacción explícita en vuelo, la publica (SPEC-0019).
    ///
    /// Returns:
    ///     El `Lsn` del commit, o el siguiente LSN si no había páginas sucias.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura, el fsync o el manifiesto;
    ///     [`RuscaError::CorruptManifest`] si no se puede serializar el manifiesto;
    ///     [`RuscaError::InvalidConfig`] si la transacción activa no estaba en vuelo.
    pub fn commit(&mut self) -> Result<Lsn, RuscaError> {
        // Span de frontera (SPEC-0080): cubre la operación de commit real. Sin
        // subscriber es no-op (~1.2 ns); con él permite trazar commits.
        let span = tracing::debug_span!("commit");
        let _entered = span.enter();
        #[cfg(feature = "metrics")]
        let started = std::time::Instant::now();
        let dirty = self.pool.dirty_pages();
        let lsn = if dirty.is_empty() {
            self.wal.next_lsn()
        } else {
            let payload = encode_commit(&dirty);
            let (lsn, offset) = self.wal.append_at(0, RecordKind::Commit, &payload)?;
            self.wal.sync()?;
            if self.encryption.is_some() {
                // SPEC-0106: el commit ya es durable en el WAL, así que las
                // páginas pasan a ser víctimas limpias de LRU-K y el overlay
                // apunta a este record para reconstruirlas bajo presión.
                for page in &dirty {
                    self.pool.mark_clean(page.id())?;
                    self.wal_overlay.insert(page.id(), (lsn, offset));
                }
            } else {
                self.publish(dirty)?;
            }
            self.manifest.bump_epoch();
            self.manifest.checkpoint_lsn = lsn;
            self.manifest.store(&self.manifest_path)?;
            self.last_lsn = lsn;
            lsn
        };
        if let Some(tx) = self.active_tx.take() {
            self.txn.commit(tx)?;
        }
        #[cfg(feature = "metrics")]
        metrics::histogram!("ruscadb_commit_latency_seconds")
            .record(started.elapsed().as_secs_f64());
        Ok(lsn)
    }

    /// Cierra la base limpiamente (commit pendiente + fsync de páginas).
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el cierre.
    pub fn close(mut self) -> Result<(), RuscaError> {
        self.commit()?;
        self.file.flush()?;
        // Persiste el caché semántico (SPEC-0093) tras el cierre limpio: la
        // próxima apertura restaura las entradas si no hubo mutación posterior.
        self.persist_semantic_cache()?;
        Ok(())
    }

    /// Aborta la transacción activa y descarta los cambios sin confirmar.
    ///
    /// Semántica: la base vuelve a su **último estado confirmado**. Si hay una
    /// transacción en vuelo ([`Database::begin`]) se aborta con
    /// [`TxnManager::rollback`]; después se descartan todas las páginas sucias
    /// del buffer pool ([`BufferPool::discard`]), de modo que no se escriben a
    /// disco ni se publican en el WAL. Los índices derivados y el índice
    /// primario se reconstruyen desde el heap (persistido en claro, o
    /// materializable desde el overlay en cifrado) para eliminar las entradas
    /// de las filas abortadas y dejar la base operativa.
    ///
    /// ## Modo cifrado (SPEC-0105, SPEC-0106)
    ///
    /// Con las páginas confirmadas marcadas limpias en el commit (SPEC-0106),
    /// descartar las sucias **basta** para volver al último estado
    /// confirmado: las desalojadas se materializan desde el overlay al leerse.
    /// Las páginas creadas sin commit no están en el WAL ni en `.data`, por lo
    /// que dejan de existir (su `read_page` posterior falla con
    /// [`RuscaError::PageOutOfRange`]). No se escribe ni `.data` ni `.wal`.
    ///
    /// Returns:
    ///     `Ok(())` tras abortar y descartar; es un *no-op* sin cambios.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si una página sucia está pinneada
    ///     (invariante interno roto);
    ///     [`RuscaError::WalCorrupt`] si el overlay apunta a un record inválido
    ///     (modo cifrado, al reconstruir los índices);
    ///     [`RuscaError::CorruptManifest`] si el heap persistido es inválido.
    pub fn rollback(&mut self) -> Result<(), RuscaError> {
        if let Some(tx) = self.active_tx.take() {
            self.txn.rollback(tx)?;
        }
        self.discard_dirty_pages()?;
        self.rebuild_indexes()?;
        self.rebuild_primary_index()?;
        Ok(())
    }

    /// Descarta todos los marcos sucios del buffer pool (sin escribir a disco).
    ///
    /// Returns:
    ///     `Ok(())` cuando no quedan páginas sucias.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si alguna página sucia está pinneada.
    fn discard_dirty_pages(&mut self) -> Result<(), RuscaError> {
        let dirty = self.pool.dirty_pages();
        for page in dirty {
            self.pool.discard(page.id())?;
        }
        Ok(())
    }

    /// Reaplica los commit records del WAL sobre el `.data` (solo modo claro).
    ///
    /// Recovery idempotente SPEC-0004: con SPEC-0106 el modo cifrado no usa
    /// esta ruta — su estado vive en el overlay construido por
    /// [`Database::build_overlay`] y se materializa al leer.
    ///
    /// Returns:
    ///     El LSN del último commit aplicado (`0` si el WAL no tiene commits).
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si un commit record está truncado;
    ///     [`RuscaError::Io`] si falla la escritura del archivo de datos.
    fn replay(&mut self) -> Result<Lsn, RuscaError> {
        let mut pages = Vec::new();
        let mut last_applied = 0;
        for record in ruscadb_wal::read_records(&self.wal_path)? {
            if record.kind != RecordKind::Commit {
                continue;
            }
            pages.extend(decode_commit(&record.payload)?);
            last_applied = record.lsn;
        }
        for page in &pages {
            self.file.write_page(page)?;
        }
        self.file.flush()?;
        Ok(last_applied)
    }

    /// Construye el overlay del WAL en cifrado sin precargar el pool (SPEC-0106).
    ///
    /// Una sola pasada por los commit records reteniendo, por cada página, el
    /// LSN y el offset del record que contiene su última imagen confirmada
    /// (patrón wal-index de SQLite: recuperación perezosa, sin vuelcar el
    /// working set a RAM).
    ///
    /// Returns:
    ///     El LSN del último commit aplicado (`0` si el WAL no tiene commits).
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si un commit record está truncado.
    fn build_overlay(&mut self) -> Result<Lsn, RuscaError> {
        let key = self.encryption.as_ref().map(|encryption| *encryption.key());
        let mut last_applied = 0;
        for entry in ruscadb_wal::read_records_with_offsets(&self.wal_path, key.as_ref())? {
            if entry.record.kind != RecordKind::Commit {
                continue;
            }
            for page_id in commit_page_ids(&entry.record.payload)? {
                self.wal_overlay
                    .insert(PageId(page_id), (entry.record.lsn, entry.offset));
            }
            last_applied = entry.record.lsn;
        }
        Ok(last_applied)
    }

    /// Publica páginas sucias al `.data` y las marca limpias (solo modo claro).
    ///
    /// En modo cifrado no se llama: el WAL es la única fuente de verdad y las
    /// páginas confirmadas se marcan limpias en [`Database::commit`] junto con
    /// la actualización del overlay (SPEC-0106).
    fn publish(&mut self, dirty: Vec<Page>) -> Result<(), RuscaError> {
        for page in &dirty {
            self.file.write_page(page)?;
        }
        self.file.flush()?;
        for page in &dirty {
            self.pool.mark_clean(page.id())?;
        }
        Ok(())
    }

    /// Acceso de solo lectura al manifiesto versionado.
    ///
    /// Returns:
    ///     El manifiesto actual (`schema_version`, `epoch`, `checkpoint_lsn`).
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Último LSN confirmado (coincide con `checkpoint_lsn` tras un commit).
    ///
    /// Returns:
    ///     El LSN del último commit aplicado (`0` en una base nueva).
    pub fn last_lsn(&self) -> Lsn {
        self.last_lsn
    }

    /// Número de fsyncs del WAL realizados hasta ahora (SPEC-0081).
    ///
    /// Con [`DbConfig::fsync_policy`] = [`FsyncPolicy::Normal`] crece una vez
    /// por **grupo** de group commit (no por commit), lo que permite medir el
    /// coalescing (AC-0081-02).
    ///
    /// Returns:
    ///     El conteo monótono de `fsync` ejecutados sobre el WAL de la base.
    pub fn sync_count(&self) -> u64 {
        self.wal.sync_count()
    }

    /// Política de fsync del WAL con la que se abrió la base (SPEC-0081).
    ///
    /// Returns:
    ///     La [`FsyncPolicy`] activa.
    pub fn fsync_policy(&self) -> FsyncPolicy {
        self.wal.policy()
    }

    /// Inicia una transacción MVCC explícita y la marca como activa.
    ///
    /// Returns:
    ///     El `TxId` monótono de la nueva transacción.
    pub fn begin(&mut self) -> TxId {
        let tx = self.txn.begin();
        self.active_tx = Some(tx);
        tx
    }

    /// Toma un snapshot de visibilidad con el estado transaccional actual.
    ///
    /// Returns:
    ///     La vista fija de visibilidad para consultas "as of".
    pub fn snapshot(&self) -> Snapshot {
        self.txn.snapshot()
    }

    /// Transacción explícita en vuelo, si [`Database::begin`] fue llamada.
    ///
    /// Returns:
    ///     El `TxId` activo o `None`.
    pub fn active_tx(&self) -> Option<TxId> {
        self.active_tx
    }

    /// Ruta del fichero WAL asociado a la base (SPEC-0062).
    ///
    /// Returns:
    ///     La ruta del `.wal` derivada de la ruta de datos.
    pub(crate) fn wal_path(&self) -> &Path {
        &self.wal_path
    }

    /// Clave de cifrado del WAL, si la base se abrió cifrada (SPEC-0062).
    ///
    /// Returns:
    ///     La clave XChaCha20 (`[u8; 32]`) o `None` en bases en claro.
    pub(crate) fn encryption_key(&self) -> Option<[u8; 32]> {
        self.encryption.as_ref().map(|encryption| *encryption.key())
    }
}

/// Ruta del WAL derivada de la ruta de datos (extensión `.wal`).
fn wal_path_for(data_path: &Path) -> PathBuf {
    data_path.with_extension("wal")
}

/// Ruta del manifiesto derivada de la ruta de datos (`.manifest.json`).
///
/// Args:
///     data_path: Ruta del archivo de páginas.
///
/// Returns:
///     La ruta del manifiesto.
fn manifest_path_for(data_path: &Path) -> PathBuf {
    data_path.with_extension("manifest.json")
}

/// Carga el manifiesto si existe o crea uno nuevo.
///
/// La validación de `schema_version` vive en `ruscadb-txn`
/// (`Manifest::load`, SPEC-0077): aquí solo se propaga el error.
///
/// Args:
///     path: Ruta del fichero `MANIFEST.json`.
///
/// Returns:
///     El manifiesto y `true` si acaba de crearse (hay que persistirlo).
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el JSON es inválido;
///     [`RuscaError::SchemaVersionTooNew`] si la versión es más nueva que la
///     soportada; [`RuscaError::MigrationFailed`] si falta una migración;
///     [`RuscaError::Io`] si el fichero existe pero no se puede leer.
fn load_or_create_manifest(path: &Path) -> Result<(Manifest, bool), RuscaError> {
    if !path.exists() {
        return Ok((Manifest::new(), true));
    }
    let manifest = Manifest::load(path)?;
    Ok((manifest, false))
}

/// Serializa el conjunto de páginas sucias de un commit.
fn encode_commit(pages: &[Page]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(COUNT_SIZE + pages.len() * (ID_SIZE + PAGE_SIZE));
    payload.extend_from_slice(&(pages.len() as u32).to_le_bytes());
    for page in pages {
        payload.extend_from_slice(&page.id().0.to_le_bytes());
        payload.extend_from_slice(page.data());
    }
    payload
}

/// Deserializa el payload de un commit record.
fn decode_commit(payload: &[u8]) -> Result<Vec<Page>, RuscaError> {
    let count = read_u32(payload, 0)? as usize;
    let mut pages = Vec::with_capacity(count);
    let mut offset = COUNT_SIZE;
    for _ in 0..count {
        let id = read_u64(payload, offset)?;
        offset += ID_SIZE;
        let end = offset + PAGE_SIZE;
        let data = payload
            .get(offset..end)
            .ok_or_else(|| RuscaError::WalCorrupt("commit record truncado".to_string()))?;
        let mut page = Page::new(PageId(id));
        page.data_mut().copy_from_slice(data);
        pages.push(page);
        offset = end;
    }
    Ok(pages)
}

/// Extrae solo los `PageId` de un commit record (SPEC-0106, sin materializar
/// las páginas: la construcción del overlay no necesita los 4 KiB de cada una).
///
/// Args:
///     payload: Carga útil de un commit record ([`encode_commit`]).
///
/// Returns:
///     Los ids de página en orden de aparición.
///
/// Errors:
///     [`RuscaError::WalCorrupt`] si el payload está truncado.
fn commit_page_ids(payload: &[u8]) -> Result<Vec<u64>, RuscaError> {
    let count = read_u32(payload, 0)? as usize;
    let mut ids = Vec::with_capacity(count);
    let mut offset = COUNT_SIZE;
    for _ in 0..count {
        ids.push(read_u64(payload, offset)?);
        offset += ID_SIZE + PAGE_SIZE;
    }
    Ok(ids)
}

/// Materializa una página desde el payload de un commit record (SPEC-0106).
///
/// Copia solo el slot de 4 KiB de la página pedida en vez de decodificar el
/// record completo.
///
/// Args:
///     pid: Página solicitada.
///     payload: Carga útil del commit record que la contiene.
///
/// Returns:
///     La página con su imagen confirmada en ese record.
///
/// Errors:
///     [`RuscaError::WalCorrupt`] si el payload está truncado o no contiene
///     la página `pid`.
fn materialize_commit_page(pid: PageId, payload: &[u8]) -> Result<Page, RuscaError> {
    let count = read_u32(payload, 0)? as usize;
    let mut offset = COUNT_SIZE;
    for _ in 0..count {
        let id = read_u64(payload, offset)?;
        offset += ID_SIZE;
        let end = offset + PAGE_SIZE;
        let data = payload
            .get(offset..end)
            .ok_or_else(|| RuscaError::WalCorrupt("commit record truncado".to_string()))?;
        if id == pid.0 {
            let mut page = Page::new(pid);
            page.data_mut().copy_from_slice(data);
            return Ok(page);
        }
        offset = end;
    }
    Err(RuscaError::WalCorrupt(format!(
        "el commit record no contiene la página {}",
        pid.0
    )))
}

/// Lee un `u32` little-endian o devuelve error de commit corrupto.
fn read_u32(bytes: &[u8], at: usize) -> Result<u32, RuscaError> {
    let slice = bytes
        .get(at..at + 4)
        .ok_or_else(|| RuscaError::WalCorrupt(format!("commit truncado en offset {at}")))?;
    slice
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(|_| RuscaError::WalCorrupt("u32 de commit inválido".to_string()))
}

/// Lee un `u64` little-endian o devuelve error de commit corrupto.
fn read_u64(bytes: &[u8], at: usize) -> Result<u64, RuscaError> {
    let slice = bytes
        .get(at..at + 8)
        .ok_or_else(|| RuscaError::WalCorrupt(format!("commit truncado en offset {at}")))?;
    slice
        .try_into()
        .map(u64::from_le_bytes)
        .map_err(|_| RuscaError::WalCorrupt("u64 de commit inválido".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use rstest::rstest;
    use std::collections::BTreeMap;
    use std::fs::OpenOptions;
    use std::io::Write;

    fn write_marker(database: &mut Database, id: u64, marker: u8) -> Result<(), RuscaError> {
        let mut page = Page::new(PageId(id));
        page.data_mut()[0] = marker;
        database.write_page(&page)
    }

    /// AC-0004-01 — un commit sobrevive a cerrar y reabrir la base.
    #[rstest]
    #[case(2)]
    #[case(4)]
    #[case(16)]
    // @spec AC-0004-01
    fn test_ac_0004_01_commit_survives_reopen(#[case] capacity: usize) {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        {
            let mut database = Database::open(DbConfig::new(&path, capacity)).expect("open");
            write_marker(&mut database, 0, 42).expect("write");
            write_marker(&mut database, 3, 7).expect("write");
            database.commit().expect("commit");
        }
        let mut database = Database::open(DbConfig::new(&path, capacity)).expect("reopen");
        assert_eq!(database.read_page(PageId(0)).expect("read").data()[0], 42);
        assert_eq!(database.read_page(PageId(3)).expect("read").data()[0], 7);
    }

    /// AC-0004-02 — el replay restaura páginas si el archivo de datos se pierde.
    #[test]
    // @spec AC-0004-02
    fn test_ac_0004_02_recovery_replays_committed_pages() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        {
            let mut database = Database::open(DbConfig::new(&path, 4)).expect("open");
            write_marker(&mut database, 0, 42).expect("write");
            write_marker(&mut database, 1, 43).expect("write");
            database.commit().expect("commit");
        }
        std::fs::remove_file(&path).expect("borra el archivo de datos");

        let mut database = Database::open(DbConfig::new(&path, 4)).expect("reopen");
        assert_eq!(database.read_page(PageId(0)).expect("read").data()[0], 42);
        assert_eq!(database.read_page(PageId(1)).expect("read").data()[0], 43);
    }

    /// AC-0004-03 — la cola rasgada del WAL se trunca sin perder el commit válido.
    #[test]
    // @spec AC-0004-03
    fn test_ac_0004_03_torn_wal_tail_is_truncated() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let wal_path = path.with_extension("wal");
        {
            let mut database = Database::open(DbConfig::new(&path, 4)).expect("open");
            write_marker(&mut database, 1, 9).expect("write");
            database.commit().expect("commit");
        }
        {
            let mut wal = OpenOptions::new()
                .append(true)
                .open(&wal_path)
                .expect("abre wal");
            wal.write_all(&[0xAA, 0xAA, 0xAA]).expect("basura");
            wal.sync_all().expect("sync");
        }
        let mut database = Database::open(DbConfig::new(&path, 4)).expect("reopen");
        assert_eq!(database.read_page(PageId(1)).expect("read").data()[0], 9);
    }

    /// Backpressure: escribir más páginas sucias que la capacidad falla.
    #[test]
    fn test_write_backpressure_when_pool_saturated() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let mut database = Database::open(DbConfig::new(&path, 2)).expect("open");
        write_marker(&mut database, 0, 1).expect("write");
        write_marker(&mut database, 1, 2).expect("write");
        let error = write_marker(&mut database, 2, 3).unwrap_err();
        assert!(matches!(error, RuscaError::BufferPoolFull { capacity: 2 }));
    }

    /// `close` persiste el commit pendiente antes de terminar.
    #[test]
    fn test_close_persists_pending_commit() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        {
            let mut database = Database::open(DbConfig::new(&path, 4)).expect("open");
            write_marker(&mut database, 2, 55).expect("write");
            database.close().expect("close");
        }
        let mut database = Database::open(DbConfig::new(&path, 4)).expect("reopen");
        assert_eq!(database.read_page(PageId(2)).expect("read").data()[0], 55);
    }

    proptest! {
        /// Invariante: todo commit sobrevive a reopen con contenido exacto.
        #[test]
        fn prop_committed_pages_survive_reopen(
            markers in prop::collection::vec((0u64..16, any::<u8>()), 1..20),
        ) {
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join("db.data");
            let mut expected: BTreeMap<u64, u8> = BTreeMap::new();
            {
                let mut database = Database::open(DbConfig::new(&path, 32)).expect("open");
                for (raw, marker) in &markers {
                    let id = raw % 16;
                    write_marker(&mut database, id, *marker).expect("write");
                    expected.insert(id, *marker);
                }
                database.commit().expect("commit");
            }
            let mut database = Database::open(DbConfig::new(&path, 32)).expect("reopen");
            for (id, marker) in &expected {
                let page = database.read_page(PageId(*id)).expect("read");
                prop_assert_eq!(page.data()[0], *marker);
            }
        }
    }

    /// AC-0106-03 — el commit cifrado marca limpias las páginas confirmadas
    /// para que LRU-K pueda desalojarlas (SPEC-0106, FR-0106-02).
    #[test]
    // @spec AC-0106-03
    fn test_ac_0106_03_encrypted_commit_marks_pages_clean() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let mut config = DbConfig::new(&path, 8);
        config.encryption = Some(EncryptionConfig::new([0x45; 32]));
        let mut database = Database::open(config).expect("open");
        write_marker(&mut database, 0, 42).expect("write");
        database.commit().expect("commit");
        assert_eq!(
            database.pool.is_dirty(PageId(0)),
            Some(false),
            "la pagina confirmada debe quedar limpia (desalojable)"
        );
        assert!(
            database.pool.dirty_pages().is_empty(),
            "el pool no debe conservar marcas sucias tras el commit cifrado"
        );
    }

    /// AC-0106-04 — el overlay registra cada página confirmada con el LSN de
    /// su commit y el offset exacto de su record (SPEC-0106, FR-0106-01 y
    /// NF-0106-01: lectura dirigida, sin barrido lineal del WAL).
    #[test]
    // @spec AC-0106-04
    fn test_ac_0106_04_encrypted_overlay_tracks_committed_pages() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let wal_path = path.with_extension("wal");
        let key = [0x45u8; 32];
        let mut config = DbConfig::new(&path, 8);
        config.encryption = Some(EncryptionConfig::new(key));
        let mut database = Database::open(config).expect("open");
        write_marker(&mut database, 0, 42).expect("write");
        write_marker(&mut database, 3, 7).expect("write");
        let lsn = database.commit().expect("commit");

        for id in [PageId(0), PageId(3)] {
            let (record_lsn, offset) = database
                .wal_overlay
                .get(&id)
                .unwrap_or_else(|| panic!("overlay sin la pagina {id:?}"));
            assert_eq!(*record_lsn, lsn, "el overlay apunta al LSN del commit");
            let record = ruscadb_wal::read_record_at(&wal_path, Some(&key), *offset)
                .expect("lectura dirigida en el offset del overlay");
            assert_eq!(record.lsn, lsn, "el record en el offset es el commit");
            assert_eq!(record.kind, RecordKind::Commit);
        }
    }

    /// AC-0106-07 — en cifrado solo el dirty sin confirmar agota el pool
    /// (SPEC-0106, NF-0106-02: el límite restante es idéntico al modo claro).
    #[test]
    // @spec AC-0106-07
    fn test_ac_0106_07_encrypted_pool_full_only_for_uncommitted_dirty() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let mut config = DbConfig::new(&path, 1);
        config.encryption = Some(EncryptionConfig::new([0x45; 32]));
        let mut database = Database::open(config).expect("open");

        // A confirmada: con SPEC-0106 queda limpia y es victima de desalojo
        // (hoy el publish cifrado es no-op y A seguiria sucia: write B fallaria).
        write_marker(&mut database, 0, 42).expect("write A");
        database.commit().expect("commit A");
        write_marker(&mut database, 1, 43).expect("write B desaloja A limpia");

        // B sigue sucia (sin confirmar): no hay victima para C.
        let error = write_marker(&mut database, 2, 44).expect_err("sin victimas");
        assert!(
            matches!(error, RuscaError::BufferPoolFull { capacity: 1 }),
            "se esperaba BufferPoolFull, se obtuvo {error:?}"
        );
    }
}
