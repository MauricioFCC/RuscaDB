//! Motor de almacenamiento durable: composition root de RuscaDB.
//!
//! Combina `PagedFile` + `BufferPool` + `Wal` con política **WAL-first**:
//! el commit hace `fsync` del WAL antes de publicar páginas, y el arranque
//! ejecuta un replay idempotente de los commit records.
//!
//! Ver `specs/durable_engine.md` (SPEC-0004).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ruscadb_ai::ModelRegistry;
use ruscadb_btree::BPlusTree;
use ruscadb_core::{RecordId, RuscaError};
use ruscadb_multimodal::BlobStore;
use ruscadb_storage::{BufferPool, PAGE_SIZE, Page, PageId, PagedFile};
use ruscadb_txn::{CURRENT_SCHEMA_VERSION, Manifest, Snapshot, TxId, TxnManager};
use ruscadb_wal::{Lsn, RecordKind, Wal};

use crate::encryption::EncryptionConfig;
use crate::heap::RowLocator;
use crate::indexes::TableIndexes;

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
}

impl DbConfig {
    /// Crea una configuración de apertura (sin cifrado ni blob store).
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
    /// Blob store content-addressed abierto junto a la base (SPEC-0038).
    ///
    /// `None` cuando `DbConfig::blob_path` no se fijó: las operaciones de blob
    /// devuelven un error accionable (no configurado).
    pub(crate) blobs: Option<BlobStore>,
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
    /// Args:
    ///     config: Configuración de apertura.
    ///
    /// Returns:
    ///     La base lista para operar.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el acceso a disco;
    ///     [`RuscaError::InvalidConfig`] si `pool_capacity == 0`;
    ///     [`RuscaError::CorruptManifest`] si el manifiesto es inválido o tiene
    ///     una `schema_version` desconocida;
    ///     [`RuscaError::WalCorrupt`] si la clave falta o no autentica.
    pub fn open(config: DbConfig) -> Result<Self, RuscaError> {
        let wal_path = wal_path_for(&config.data_path);
        let manifest_path = manifest_path_for(&config.data_path);
        let file = PagedFile::open(&config.data_path)?;
        let wal = match config.encryption.as_ref() {
            Some(encryption) => Wal::open_encrypted(&wal_path, encryption.key())?,
            None => Wal::open(&wal_path)?,
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
            txn: TxnManager::new(),
            active_tx: None,
            registry: ModelRegistry::new(),
            last_lsn: 0,
            blobs,
        };
        let applied = database.replay()?;
        database.last_lsn = applied;
        database.manifest.checkpoint_lsn = applied;
        if is_new {
            database.manifest.store(&database.manifest_path)?;
        }
        database.rebuild_indexes()?;
        database.rebuild_primary_index()?;
        database.restore_txn_watermark()?;
        Ok(database)
    }

    /// Indica si una página existe en el pool o en el archivo de datos.
    ///
    /// Evita intentar leer una página inexistente: el buffer pool reserva un
    /// marco antes de fallar, lo que con pools pequeños agotaría la capacidad.
    ///
    /// Args:
    ///     id: Página consultada.
    ///
    /// Returns:
    ///     `true` si la página está en memoria o dentro del archivo.
    pub(crate) fn page_exists(&self, id: PageId) -> bool {
        self.pool.contains(id) || id.0 < self.file.page_count()
    }

    /// Lee una página (caché del pool; fallo de caché → `PagedFile`).
    ///
    /// Errors:
    ///     [`RuscaError::PageOutOfRange`] si el id no existe.
    pub fn read_page(&mut self, id: PageId) -> Result<Page, RuscaError> {
        let page = self.pool.get(id, |pid| self.file.read_page(pid))?;
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
        let dirty = self.pool.dirty_pages();
        let lsn = if dirty.is_empty() {
            self.wal.next_lsn()
        } else {
            let payload = encode_commit(&dirty);
            let lsn = self.wal.append(0, RecordKind::Commit, &payload)?;
            self.wal.sync()?;
            self.publish(dirty)?;
            self.manifest.bump_epoch();
            self.manifest.checkpoint_lsn = lsn;
            self.manifest.store(&self.manifest_path)?;
            self.last_lsn = lsn;
            lsn
        };
        if let Some(tx) = self.active_tx.take() {
            self.txn.commit(tx)?;
        }
        Ok(lsn)
    }

    /// Cierra la base limpiamente (commit pendiente + fsync de páginas).
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el cierre.
    pub fn close(mut self) -> Result<(), RuscaError> {
        self.commit()?;
        self.file.flush()?;
        Ok(())
    }

    /// Aborta la transacción activa y descarta los cambios sin confirmar.
    ///
    /// Semántica: la base vuelve a su **último estado confirmado**. Si hay una
    /// transacción en vuelo ([`Database::begin`]) se aborta con
    /// [`TxnManager::rollback`]; después se descartan todas las páginas sucias
    /// del buffer pool ([`BufferPool::discard`]), de modo que no se escriben a
    /// disco ni se publican en el WAL. Los índices derivados y el índice
    /// primario se reconstruyen desde el heap persistido para eliminar las
    /// entradas de las filas abortadas y dejar la base operativa.
    ///
    /// ## Límite documentado (modo cifrado)
    ///
    /// En modo cifrado las páginas nunca se publican a `.data` (el WAL es la
    /// fuente de verdad), así que un `rollback` no puede revertir el estado en
    /// memoria sin rehacer el replay. Se rechaza con
    /// [`RuscaError::InvalidConfig`]: para volver al último estado confirmado,
    /// cierra y reabre la base con la clave.
    ///
    /// Returns:
    ///     `Ok(())` tras abortar y descartar; es un *no-op* sin cambios.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si la base está en modo cifrado o si
    ///     una página sucia está pinneada (invariante interno roto);
    ///     [`RuscaError::CorruptManifest`] si el heap persistido es inválido.
    pub fn rollback(&mut self) -> Result<(), RuscaError> {
        if self.encryption.is_some() {
            return Err(RuscaError::InvalidConfig(
                "rollback no está soportado en modo cifrado: las páginas viven en el WAL y \
                 nunca se publican a .data; cierra y reabre la base para volver al último \
                 estado confirmado"
                    .to_string(),
            ));
        }
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

    /// Reaplica los commit records del WAL (replay idempotente).
    ///
    /// Con cifrado, el replay descifra con la clave de apertura y restaura las
    /// páginas solo en el pool (el WAL es la fuente de verdad, SPEC-0013).
    ///
    /// Returns:
    ///     El LSN del último commit aplicado (`0` si el WAL no tiene commits).
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si un commit record está truncado;
    ///     [`RuscaError::Io`] si falla la escritura del archivo de datos.
    fn replay(&mut self) -> Result<Lsn, RuscaError> {
        let key = self.encryption.as_ref().map(|encryption| *encryption.key());
        let mut pages = Vec::new();
        let mut last_applied = 0;
        for record in ruscadb_wal::read_records_with_key(&self.wal_path, key.as_ref())? {
            if record.kind != RecordKind::Commit {
                continue;
            }
            pages.extend(decode_commit(&record.payload)?);
            last_applied = record.lsn;
        }
        if self.encryption.is_some() {
            for page in &pages {
                self.write_page(page)?;
            }
            return Ok(last_applied);
        }
        for page in &pages {
            self.file.write_page(page)?;
        }
        self.file.flush()?;
        Ok(last_applied)
    }

    /// Publica páginas sucias tras el fsync del WAL.
    ///
    /// En modo claro escribe al `.data` y marca limpio; en modo cifrado no toca
    /// el `.data` (las páginas quedan `dirty` en el pool, nunca desalojables).
    fn publish(&mut self, dirty: Vec<Page>) -> Result<(), RuscaError> {
        if self.encryption.is_some() {
            return Ok(());
        }
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

/// Carga el manifiesto si existe o crea uno nuevo, validando la versión.
///
/// Args:
///     path: Ruta del fichero `MANIFEST.json`.
///
/// Returns:
///     El manifiesto y `true` si acaba de crearse (hay que persistirlo).
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el JSON es inválido o la
///     `schema_version` es desconocida;
///     [`RuscaError::Io`] si el fichero existe pero no se puede leer.
fn load_or_create_manifest(path: &Path) -> Result<(Manifest, bool), RuscaError> {
    if !path.exists() {
        return Ok((Manifest::new(), true));
    }
    let manifest = Manifest::load(path)?;
    if manifest.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(RuscaError::CorruptManifest(format!(
            "schema_version {} desconocida en {} (se esperaba {CURRENT_SCHEMA_VERSION})",
            manifest.schema_version,
            path.display()
        )));
    }
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
}
