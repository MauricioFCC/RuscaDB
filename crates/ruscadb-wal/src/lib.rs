//! # ruscadb-wal
//!
//! Write-Ahead Log global de RuscaDB: frames con CRC32C, group commit
//! (coalescing fsync) y recovery por truncado del tail rasgado.
//!
//! ## Formato del frame
//!
//! ```text
//! [ len: u32 LE | body: len bytes | crc32c: u32 LE ]
//! body = [ lsn: u64 LE | tx_id: u64 LE | kind: u8 | payload: bytes ]
//! ```
//!
//! - `len` = longitud del `body`; el CRC32C cubre exactamente el `body`.
//! - **Torn write:** al reabrir se escanea secuencialmente y se **trunca** el
//!   archivo en el primer frame inválido (nunca se "repara").
//! - **Recovery = replay idempotente** desde el último frame válido.
//!
//! Especificación: `specs/wal_durability.md` (SPEC-0002).
//!
//! ## Cifrado en reposo (SPEC-0013)
//!
//! En modo cifrado el formato del frame **no cambia**: solo el `payload` se
//! envuelve en `[RCE1 | 0x01 | nonce 24 B | seal(payload)]` (XChaCha20-Poly1305
//! vía `ruscadb-crypto`). Las cabeceras (`lsn`/`tx_id`/`kind`) quedan en claro
//! para permitir el escaneo secuencial del recovery.
//!
//! - **Nonce determinista sin RNG**: `lsn.to_le_bytes() + [0u8; 16]`. La
//!   unicidad del nonce (exigida por el AEAD) deriva de la monotonía del LSN:
//!   cada frame usa un LSN único y creciente, luego cada nonce se usa una sola
//!   vez bajo la misma clave. Se almacena en el envelope y el lector verifica
//!   que coincide con el LSN del frame (anti-splice).
//! - **Defensa en profundidad**: el CRC32C cubre el `body` (incluido el
//!   envelope). Un CRC inválido trunca la cola (torn write); un CRC válido con
//!   AEAD inválido es [`RuscaError::WalCorrupt`]. Ambos fallos dan el mismo
//!   error visible.
//! - **Modo estricto por fichero**: mezclar frames claros y cifrados es error,
//!   nunca claro silencioso. El magic `RCE1` con longitud de envelope marca
//!   contenido cifrado; cualquier otro payload es claro (incluidos los que
//!   empiezan por `0x01`, como un commit de una sola página).

#![forbid(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ruscadb_core::RuscaError;

/// Número de secuencia del log (monótono creciente).
pub type Lsn = u64;

// ── Layout binario del frame ─────────────────────────────────────────────────
const LEN_SIZE: usize = 4;
const CRC_SIZE: usize = 4;
const LSN_SIZE: usize = 8;
const TX_ID_SIZE: usize = 8;
const KIND_SIZE: usize = 1;
const LSN_OFFSET: usize = 0;
const TX_ID_OFFSET: usize = LSN_OFFSET + LSN_SIZE;
const KIND_OFFSET: usize = TX_ID_OFFSET + TX_ID_SIZE;
const PAYLOAD_OFFSET: usize = KIND_OFFSET + KIND_SIZE;
/// Tamaño mínimo válido del `body` (cabecera sin payload).
const BODY_HEADER: usize = PAYLOAD_OFFSET;
/// Tamaño máximo aceptado del `body` (protección anti-DoS, 64 MiB).
const MAX_BODY: usize = 64 * 1024 * 1024;

/// Magic del envelope cifrado `[RCE1 | 0x01 | nonce 24 B | seal(payload)]`.
///
/// Un solo byte de versión colisiona con payloads claros (p. ej. un commit de
/// una página empieza por `count = 1`): el magic de 4 B lo hace imposible en la
/// práctica (`count = 0x31454352` serían ~830 M de páginas, muy por encima del
/// límite `MAX_BODY`).
const ENCRYPTED_MAGIC: [u8; 4] = *b"RCE1";
/// Byte de versión del envelope cifrado.
const ENCRYPTED_VERSION: u8 = 0x01;
/// Cabecera del envelope: magic (4) + versión (1).
const ENVELOPE_HEADER: usize = 4 + 1;
/// Tamaño del tag Poly1305 anexado por `ruscadb_crypto::seal` (16 B).
const AEAD_TAG_SIZE: usize = 16;
/// Sobrecoste del envelope sobre el claro: cabecera (5) + nonce (24) + tag (16).
///
/// Es también la longitud mínima de un envelope válido (claro vacío).
const ENVELOPE_OVERHEAD: usize = ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE + AEAD_TAG_SIZE;

/// Tipo de registro del WAL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordKind {
    /// Inicio de transacción.
    Begin = 1,
    /// Confirmación de transacción.
    Commit = 2,
    /// Aborto de transacción.
    Abort = 3,
    /// Punto de control (checkpoint).
    Checkpoint = 4,
}

impl RecordKind {
    /// Codifica el tipo como un byte estable en disco.
    fn to_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for RecordKind {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Begin),
            2 => Ok(Self::Commit),
            3 => Ok(Self::Abort),
            4 => Ok(Self::Checkpoint),
            other => Err(other),
        }
    }
}

/// Registro del WAL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalRecord {
    /// Número de secuencia asignado.
    pub lsn: Lsn,
    /// Transacción propietaria.
    pub tx_id: u64,
    /// Tipo de registro.
    pub kind: RecordKind,
    /// Carga útil opaca (serializada por capas superiores).
    pub payload: Vec<u8>,
}

/// Resultado de un proceso de recuperación.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryOutcome {
    /// Registros válidos recuperados en orden de LSN.
    pub records: Vec<WalRecord>,
    /// Offset del final del último frame válido.
    pub last_valid_offset: u64,
    /// Bytes descartados (cola rasgada) tras el último frame válido.
    pub truncated_bytes: u64,
}

/// Política de durabilidad (`fsync`) del WAL (SPEC-0081).
///
/// - [`FsyncPolicy::Full`] (por defecto): fsync del WAL **antes** de confirmar
///   cada commit; comportamiento histórico, durable commit a commit.
/// - [`FsyncPolicy::Normal`]: *group commit* — un writer thread agrupa de forma
///   *greedy* las peticiones de fsync pendientes y las sirve con un único
///   `fsync` por grupo. Los commits entrantes se coalescen (menos fsyncs que
///   commits) y el ack de cada uno llega **solo** tras el fsync de su grupo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FsyncPolicy {
    /// fsync por commit (comportamiento histórico, por defecto).
    #[default]
    Full,
    /// Group commit con writer thread (coalescing greedy + pipeline).
    Normal,
}

/// Ventana de agrupación *greedy* del writer thread de group commit (SPEC-0081).
///
/// Tras recibir la primera petición de un grupo, el writer espera hasta esta
/// ventana para incorporar las peticiones que lleguen: los commits concurrentes
/// se coalescen en un solo `fsync`. Acota el retardo añadido por lote.
const GROUP_WINDOW: Duration = Duration::from_millis(2);

/// Resultado del fsync de un grupo, clonable para todos sus waiters.
///
/// `std::io::Error` no es `Clone`: se transporta `(kind, mensaje)` y cada
/// waiter reconstruye un [`RuscaError::Io`] equivalente.
type FlushOutcome = Result<(), (std::io::ErrorKind, String)>;

/// Petición de fsync dirigida al writer thread de group commit.
struct FlushRequest {
    /// Canal por el que se entrega el ack tras el fsync del grupo.
    ack: Sender<FlushOutcome>,
}

/// Coordinador del group commit (`FsyncPolicy::Normal`).
struct GroupCommit {
    /// Canal de peticiones; dropearlo señaliza al writer para terminar.
    request_tx: Option<Sender<FlushRequest>>,
    /// Handle del writer thread; se une en [`Drop`] (sin deadlock).
    handle: Option<JoinHandle<()>>,
}

impl GroupCommit {
    /// Envía una petición de fsync y espera su ack (durabilidad del grupo).
    ///
    /// Returns:
    ///     `Ok(())` cuando el grupo que contiene esta petición ya es durable.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si el writer no está disponible, si el fsync del
    ///     grupo falla o si el writer termina antes de entregar el ack.
    fn flush(&self) -> Result<(), RuscaError> {
        let Some(request_tx) = self.request_tx.as_ref() else {
            return Err(RuscaError::Io(std::io::Error::other(
                "group commit: writer no disponible",
            )));
        };
        let (ack_tx, ack_rx) = mpsc::channel();
        request_tx.send(FlushRequest { ack: ack_tx }).map_err(|_| {
            RuscaError::Io(std::io::Error::other(
                "group commit: writer terminó antes de encolar la petición",
            ))
        })?;
        match ack_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err((kind, message))) => Err(RuscaError::Io(std::io::Error::new(kind, message))),
            Err(_) => Err(RuscaError::Io(std::io::Error::other(
                "group commit: writer terminó antes del ack",
            ))),
        }
    }
}

impl Drop for GroupCommit {
    /// Drena el writer (dropeando el canal) y lo une sin deadlock.
    fn drop(&mut self) {
        self.request_tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Mecanismo de fsync activo: en línea (`Full`) o writer thread (`Normal`).
enum Fsync {
    /// Handle dedicado para el fsync síncrono por commit (`Full`).
    Inline(File),
    /// Coordinator del writer thread (`Normal`).
    Group(GroupCommit),
}

/// Escritor serializado del log: archivo y siguiente LSN.
///
/// El `Mutex` permite `append(&self)` (uso concurrente) y garantiza que los
/// frames se escriban íntegros y en orden, sin entrelazar bytes entre hilos.
struct Appender {
    /// Archivo del WAL (escrituras de `append`).
    file: File,
    /// Siguiente LSN a asignar.
    next_lsn: Lsn,
}

/// Write-Ahead Log de un único escritor (embebido).
pub struct Wal {
    /// Escritor serializado (permite `&self` y uso concurrente entre hilos).
    appender: Mutex<Appender>,
    path: PathBuf,
    /// Clave AEAD opcional (`None` = modo claro). Se borra en [`Drop`].
    key: Option<[u8; 32]>,
    /// Política de fsync activa.
    policy: FsyncPolicy,
    /// Mecanismo de fsync (`Inline` síncrono o writer thread de group commit).
    fsync: Fsync,
    /// Contador de fsyncs realizados (SPEC-0081, AC-0081-02).
    sync_count: Arc<AtomicU64>,
}

impl std::fmt::Debug for Wal {
    /// Formato sin exponer la clave (solo indica si hay cifrado).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wal")
            .field("path", &self.path)
            .field("next_lsn", &self.next_lsn())
            .field("policy", &self.policy)
            .field("encrypted", &self.key.is_some())
            .finish()
    }
}

impl Drop for Wal {
    /// Borra el material de clave por sobreescritura (best-effort sin `zeroize`).
    fn drop(&mut self) {
        if let Some(key) = self.key.as_mut() {
            for byte in key.iter_mut() {
                *byte = 0;
            }
        }
    }
}

/// Apertura compartida por los constructores de [`Wal`].
///
/// Args:
///     path: Ruta del archivo de log.
///     key: Clave AEAD (`None` = modo claro).
///     policy: Política de fsync ([`FsyncPolicy::Full`] o [`FsyncPolicy::Normal`]).
///
/// Returns:
///     El WAL abierto, con `next_lsn` continuando la secuencia previa.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] si el recovery con clave falla (sin truncar);
///     [`RuscaError::Io`] si falla el acceso a disco o no se puede lanzar el
///     writer thread de group commit.
fn open_impl(
    path: impl AsRef<Path>,
    key: Option<[u8; 32]>,
    policy: FsyncPolicy,
) -> Result<Wal, RuscaError> {
    let path = path.as_ref().to_path_buf();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    let outcome = scan_and_truncate_with_key(&mut file, key.as_ref())?;
    let next_lsn = outcome.records.last().map_or(0, |record| record.lsn + 1);
    file.seek(SeekFrom::End(0))?;
    let sync_count = Arc::new(AtomicU64::new(0));
    let fsync = match policy {
        // `Full`: handle dedicado (mismo objeto de archivo) para el fsync.
        FsyncPolicy::Full => Fsync::Inline(file.try_clone()?),
        // `Normal`: el writer thread fsync-ea sobre su propio handle.
        FsyncPolicy::Normal => Fsync::Group(spawn_group_commit(
            file.try_clone()?,
            Arc::clone(&sync_count),
        )?),
    };
    Ok(Wal {
        appender: Mutex::new(Appender { file, next_lsn }),
        path,
        key,
        policy,
        fsync,
        sync_count,
    })
}

/// Lanza el writer thread del group commit y devuelve su coordinador.
///
/// Args:
///     fsync_file: Handle (clon) dedicado al `fsync` del writer thread.
///     counter: Contador compartido de fsyncs del grupo.
///
/// Returns:
///     El [`GroupCommit`] con el canal y el handle del writer.
///
/// Errors:
///     [`RuscaError::Io`] si el sistema no puede crear el hilo.
fn spawn_group_commit(
    fsync_file: File,
    counter: Arc<AtomicU64>,
) -> Result<GroupCommit, RuscaError> {
    let (request_tx, request_rx) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .name("ruscadb-wal-flush".to_string())
        .spawn(move || flush_worker(request_rx, fsync_file, counter))
        .map_err(RuscaError::Io)?;
    Ok(GroupCommit {
        request_tx: Some(request_tx),
        handle: Some(handle),
    })
}

/// Bucle del writer thread: agrupa peticiones *greedy* y hace un fsync por grupo.
///
/// Coalescing: tras la primera petición de un grupo, drena las ya pendientes y
/// espera hasta [`GROUP_WINDOW`] para incorporar las que lleguen (los commits
/// concurrentes comparten un único `fsync`). Pipeline: mientras el `fsync` del
/// grupo N corre, el canal sigue recibiendo las peticiones del grupo N+1, que se
/// sirven en la siguiente iteración. Termina cuando el canal se cierra (drop
/// del coordinador), tras servir el grupo en curso.
///
/// Args:
///     request_rx: Canal receptor de peticiones de fsync.
///     fsync_file: Handle dedicado usado solo para `fsync`.
///     counter: Contador compartido de fsyncs del grupo.
fn flush_worker(request_rx: Receiver<FlushRequest>, fsync_file: File, counter: Arc<AtomicU64>) {
    let mut disconnected = false;
    while !disconnected {
        let Ok(first) = request_rx.recv() else {
            break;
        };
        let mut group = vec![first];
        // Coalescing greedy: todo lo ya pendiente entra en el mismo grupo.
        while let Ok(request) = request_rx.try_recv() {
            group.push(request);
        }
        // Ventana de agrupación: captura los commits que llegan en paralelo.
        let deadline = Instant::now() + GROUP_WINDOW;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match request_rx.recv_timeout(deadline - now) {
                Ok(request) => group.push(request),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        // Un solo fsync sirve a todo el grupo; el ack se emite después.
        let outcome: FlushOutcome = fsync_file
            .sync_data()
            .map_err(|error| (error.kind(), error.to_string()));
        if outcome.is_ok() {
            counter.fetch_add(1, Ordering::SeqCst);
        }
        for request in group {
            let _ = request.ack.send(outcome.clone());
        }
    }
}

impl Wal {
    /// Abre (o crea) el WAL y ejecuta el recovery del tail rasgado.
    ///
    /// Usa [`FsyncPolicy::Full`] (fsync por commit), el comportamiento histórico.
    ///
    /// Args:
    ///     path: Ruta del archivo de log.
    ///
    /// Returns:
    ///     El WAL abierto, con `next_lsn` continuando la secuencia previa.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el acceso a disco.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RuscaError> {
        open_impl(path, None, FsyncPolicy::Full)
    }

    /// Abre (o crea) el WAL con una política de fsync explícita (SPEC-0081).
    ///
    /// [`FsyncPolicy::Full`] conserva el comportamiento de [`Wal::open`];
    /// [`FsyncPolicy::Normal`] habilita group commit (coalescing + pipeline).
    ///
    /// Args:
    ///     path: Ruta del archivo de log.
    ///     policy: Política de durabilidad.
    ///
    /// Returns:
    ///     El WAL abierto, con `next_lsn` continuando la secuencia previa.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el acceso a disco o el writer thread.
    pub fn open_with(path: impl AsRef<Path>, policy: FsyncPolicy) -> Result<Self, RuscaError> {
        open_impl(path, None, policy)
    }

    /// Abre (o crea) el WAL en modo cifrado (SPEC-0013, AC-0013-01).
    ///
    /// El formato del frame no cambia: cada `payload` se envuelve en
    /// `[RCE1 | 0x01 | nonce 24 B | seal(payload)]` con nonce determinista derivado
    /// del LSN (`lsn.to_le_bytes() + [0u8; 16]`; único por monotonía del LSN,
    /// sin RNG). La clave se borra en [`Drop`].
    ///
    /// Args:
    ///     path: Ruta del archivo de log.
    ///     key: Clave simétrica de 32 B (XChaCha20-Poly1305 vía `ruscadb-crypto`).
    ///
    /// Returns:
    ///     El WAL abierto, con `next_lsn` continuando la secuencia previa.
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si hay frames cifrados ilegibles con esta
    ///     clave; [`RuscaError::Io`] si falla el acceso a disco.
    pub fn open_encrypted(path: impl AsRef<Path>, key: &[u8; 32]) -> Result<Self, RuscaError> {
        open_impl(path, Some(*key), FsyncPolicy::Full)
    }

    /// Abre (o crea) el WAL cifrado con una política de fsync explícita (SPEC-0081).
    ///
    /// Combina el cifrado de [`Wal::open_encrypted`] con la política de
    /// [`Wal::open_with`] (p. ej. group commit en reposo cifrado).
    ///
    /// Args:
    ///     path: Ruta del archivo de log.
    ///     key: Clave simétrica de 32 B (XChaCha20-Poly1305 vía `ruscadb-crypto`).
    ///     policy: Política de durabilidad.
    ///
    /// Returns:
    ///     El WAL abierto, con `next_lsn` continuando la secuencia previa.
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si hay frames cifrados ilegibles con esta
    ///     clave; [`RuscaError::Io`] si falla el acceso a disco o el writer thread.
    pub fn open_encrypted_with(
        path: impl AsRef<Path>,
        key: &[u8; 32],
        policy: FsyncPolicy,
    ) -> Result<Self, RuscaError> {
        open_impl(path, Some(*key), policy)
    }

    /// Añade un registro al WAL y devuelve su LSN.
    ///
    /// El registro queda **escrito** pero no necesariamente durable hasta
    /// llamar a [`Wal::sync`]. Es seguro llamarlo desde varios hilos: el
    /// `Mutex` interno serializa los `append` (frames íntegros y LSN en orden).
    ///
    /// En modo cifrado el `payload` se sella con el nonce del LSN asignado; el
    /// límite de tamaño incluye el sobrecoste del envelope (45 B).
    ///
    /// Args:
    ///     tx_id: Transacción propietaria.
    ///     kind: Tipo de registro.
    ///     payload: Carga útil opaca.
    ///
    /// Returns:
    ///     El `Lsn` asignado (monótono creciente).
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si el payload excede el tamaño máximo por
    ///     frame; [`RuscaError::Io`] si falla la escritura.
    pub fn append(&self, tx_id: u64, kind: RecordKind, payload: &[u8]) -> Result<Lsn, RuscaError> {
        self.append_at(tx_id, kind, payload)
            .map(|(lsn, _offset)| lsn)
    }

    /// Igual que [`Wal::append`] pero devuelve también el offset de fichero
    /// del frame escrito (SPEC-0106).
    ///
    /// El offset es lo que registra el overlay del pool cifrado para volver a
    /// leer este registro con [`read_record_at`] sin barrer el log.
    ///
    /// Args:
    ///     tx_id: Transacción propietaria.
    ///     kind: Tipo de registro.
    ///     payload: Carga útil opaca.
    ///
    /// Returns:
    ///     El `Lsn` asignado y el offset de comienzo del frame.
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si el payload excede el tamaño máximo por
    ///     frame; [`RuscaError::Io`] si falla la escritura.
    pub fn append_at(
        &self,
        tx_id: u64,
        kind: RecordKind,
        payload: &[u8],
    ) -> Result<(Lsn, u64), RuscaError> {
        let mut appender = self.lock_appender()?;
        let stored: Vec<u8> = match self.key.as_ref() {
            None => payload.to_vec(),
            Some(key) => seal_payload(key, appender.next_lsn, payload),
        };
        if BODY_HEADER + stored.len() > MAX_BODY {
            return Err(RuscaError::WalCorrupt(format!(
                "payload de {} bytes excede el máximo de {} bytes por frame",
                payload.len(),
                MAX_BODY - BODY_HEADER
            )));
        }
        let record = WalRecord {
            lsn: appender.next_lsn,
            tx_id,
            kind,
            payload: stored,
        };
        let offset = appender.file.stream_position()?;
        appender.file.write_all(&frame::encode(&record))?;
        appender.next_lsn += 1;
        Ok((record.lsn, offset))
    }

    /// Toma el lock del escritor o falla con error accionable si está envenenado.
    ///
    /// Returns:
    ///     El guard del [`Appender`] serializado.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si un panic previo envenenó el mutex.
    fn lock_appender(&self) -> Result<MutexGuard<'_, Appender>, RuscaError> {
        self.appender.lock().map_err(|_| {
            RuscaError::Io(std::io::Error::other(
                "wal: el mutex del appender quedó envenenado por un panic previo",
            ))
        })
    }

    /// Fuerza la durabilidad de todo lo escrito (fsync de datos).
    ///
    /// Con [`FsyncPolicy::Full`] hace `fsync` en línea. Con
    /// [`FsyncPolicy::Normal`] delega en el writer thread: la llamada retorna
    /// **solo** cuando el grupo que contiene esta petición ya es durable.
    ///
    /// Returns:
    ///     `Ok(())` cuando los datos están en disco.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla el `fsync` (o el writer thread no está).
    pub fn sync(&self) -> Result<(), RuscaError> {
        match &self.fsync {
            Fsync::Inline(file) => {
                file.sync_data()?;
                self.sync_count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            Fsync::Group(group) => group.flush(),
        }
    }

    /// Ruta del archivo de log.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Último LSN asignado más uno (el siguiente a usar).
    pub fn next_lsn(&self) -> Lsn {
        match self.appender.lock() {
            Ok(appender) => appender.next_lsn,
            Err(poisoned) => poisoned.into_inner().next_lsn,
        }
    }

    /// Política de fsync activa del WAL (SPEC-0081).
    ///
    /// Returns:
    ///     La [`FsyncPolicy`] con la que se abrió el WAL.
    pub fn policy(&self) -> FsyncPolicy {
        self.policy
    }

    /// Número de fsyncs realizados hasta ahora (SPEC-0081, AC-0081-02).
    ///
    /// Con [`FsyncPolicy::Normal`] crece una vez por **grupo** (no por commit),
    /// lo que permite medir el coalescing del group commit.
    ///
    /// Returns:
    ///     El conteo monótono de `fsync` ejecutados sobre el WAL.
    pub fn sync_count(&self) -> u64 {
        self.sync_count.load(Ordering::SeqCst)
    }

    /// Fuerza un checkpoint del WAL y lo hace durable (SPEC-0080).
    ///
    /// Añade un registro [`RecordKind::Checkpoint`] que marca hasta qué LSN el
    /// log está completamente escrito y lo sincroniza con `fsync`. Es el punto
    /// de control que habilita (en el futuro) reclamar el prefijo del WAL ya
    /// aplicado; emite el span `checkpoint` para observabilidad de frontera.
    ///
    /// Returns:
    ///     El `Lsn` asignado al registro de checkpoint.
    ///
    /// Errors:
    ///     [`RuscaError::WalCorrupt`] si el payload excede el máximo por frame;
    ///     [`RuscaError::Io`] si falla la escritura o el `fsync`.
    pub fn checkpoint(&mut self) -> Result<Lsn, RuscaError> {
        // Span de frontera (SPEC-0080): checkpoint real del WAL.
        let span = tracing::debug_span!("checkpoint", lsn = self.next_lsn());
        let _entered = span.enter();
        let lsn = self.append(0, RecordKind::Checkpoint, &[])?;
        self.sync()?;
        Ok(lsn)
    }
}

/// Lee los registros válidos del WAL sin modificarlo.
///
/// Args:
///     path: Ruta del archivo de log.
///
/// Returns:
///     Los registros válidos hasta el primer frame inválido.
pub fn read_records(path: impl AsRef<Path>) -> Result<Vec<WalRecord>, RuscaError> {
    read_records_with_key(path, None)
}

/// Lee los registros válidos del WAL descifrando con `key` (SPEC-0013).
///
/// Args:
///     path: Ruta del archivo de log.
///     key: Clave AEAD (`None` = modo claro).
///
/// Returns:
///     Los registros válidos hasta el primer frame inválido, con payloads en claro.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] ante payload versionado sin clave o AEAD inválido.
pub fn read_records_with_key(
    path: impl AsRef<Path>,
    key: Option<&[u8; 32]>,
) -> Result<Vec<WalRecord>, RuscaError> {
    let bytes = std::fs::read(path.as_ref())?;
    let (records, _valid_offset) = scan_with_key(&bytes, key)?;
    Ok(records)
}

/// Registro del WAL con su offset de fichero (SPEC-0106).
///
/// Es la unidad que consume el overlay del pool cifrado: el offset permite
/// volver al frame con [`read_record_at`] sin barrer el log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalEntry {
    /// Offset de comienzo del frame en el archivo.
    pub offset: u64,
    /// Registro decodificado (payload en claro si había clave).
    pub record: WalRecord,
}

/// Lee los registros válidos del WAL con su offset de fichero (SPEC-0106).
///
/// Args:
///     path: Ruta del archivo de log.
///     key: Clave AEAD (`None` = modo claro).
///
/// Returns:
///     Los registros válidos hasta el primer frame inválido, cada uno con el
///     offset donde comienza su frame.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] ante payload versionado sin clave o AEAD
///     inválido; [`RuscaError::Io`] si el archivo no se puede leer.
pub fn read_records_with_offsets(
    path: impl AsRef<Path>,
    key: Option<&[u8; 32]>,
) -> Result<Vec<WalEntry>, RuscaError> {
    let bytes = std::fs::read(path.as_ref())?;
    let mut entries = Vec::new();
    let mut offset = 0usize;
    while let Some((raw, next)) = frame::decode(&bytes, offset) {
        let payload = open_payload(key, raw.lsn, &raw.payload)?;
        entries.push(WalEntry {
            offset: offset as u64,
            record: WalRecord {
                lsn: raw.lsn,
                tx_id: raw.tx_id,
                kind: raw.kind,
                payload,
            },
        });
        offset = next;
    }
    Ok(entries)
}

/// Lee un registro del WAL en un offset de fichero conocido (SPEC-0106).
///
/// Es la lectura dirigida que usa el overlay `page_id -> (lsn, offset)` del
/// pool cifrado: un *seek* al offset del commit record sin barrer el WAL.
/// Los frames son auto-delimitados (prefijo de longitud), así que el offset
/// basta para decodificar el registro completo.
///
/// Args:
///     path: Ruta del archivo de log.
///     key: Clave AEAD (`None` = modo claro).
///     offset: Offset de comienzo del frame a leer.
///
/// Returns:
///     El registro decodificado en ese offset, con payload en claro.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] si no hay frame válido en ese offset o el
///     payload no autentica; [`RuscaError::Io`] si falla la lectura.
pub fn read_record_at(
    path: impl AsRef<Path>,
    key: Option<&[u8; 32]>,
    offset: u64,
) -> Result<WalRecord, RuscaError> {
    let bytes = std::fs::read(path.as_ref())?;
    let Some((raw, _next)) = frame::decode(&bytes, offset as usize) else {
        return Err(RuscaError::WalCorrupt(format!(
            "no hay frame válido en el offset {offset} del WAL"
        )));
    };
    let payload = open_payload(key, raw.lsn, &raw.payload)?;
    Ok(WalRecord {
        lsn: raw.lsn,
        tx_id: raw.tx_id,
        kind: raw.kind,
        payload,
    })
}

/// Resultado de la verificación estricta del WAL (SPEC-0062).
///
/// A diferencia de [`recover_with_key`], no trunca ni reescribe: solo clasifica
/// la zona no decodificable como escritura rasgada legítima (crash) o como
/// corrupción interna (frame completo con CRC inválido).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalVerification {
    /// Offset del final del último frame válido.
    pub valid_offset: usize,
    /// Bytes de cola no decodificables.
    pub trailing_bytes: usize,
    /// `true` si la cola es una corrupción, no una escritura rasgada.
    pub corrupted: bool,
    /// Offset donde comienza la corrupción (`None` si no la hay).
    pub corrupt_offset: Option<usize>,
}

/// Verifica el WAL sin truncar, distinguiendo corrupción de cola rasgada.
///
/// Una cola rasgada legítima (crash a mitad de frame) no es corrupción: el
/// frame queda incompleto. La corrupción es un frame **completo** cuyo CRC32C
/// no valida (el byte alterado está dentro de un frame íntegro en disco).
///
/// Args:
///     path: Ruta del archivo de log.
///     key: Clave AEAD (`None` = modo claro).
///
/// Returns:
///     La [`WalVerification`] con el offset válido y la clasificación de cola.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] ante payload versionado sin clave o AEAD
///     inválido; [`RuscaError::Io`] si el archivo no se puede leer.
pub fn verify(
    path: impl AsRef<Path>,
    key: Option<&[u8; 32]>,
) -> Result<WalVerification, RuscaError> {
    let bytes = std::fs::read(path.as_ref())?;
    let (_, valid_offset) = scan_with_key(&bytes, key)?;
    let trailing_bytes = bytes.len() - valid_offset;
    let corrupted = trailing_bytes > 0 && is_complete_invalid_frame(&bytes, valid_offset);
    Ok(WalVerification {
        valid_offset,
        trailing_bytes,
        corrupted,
        corrupt_offset: corrupted.then_some(valid_offset),
    })
}

/// Indica si la cola no decodificable es un frame completo con CRC inválido.
///
/// Args:
///     bytes: Contenido íntegro del archivo de log.
///     offset: Offset donde arrancó la zona no decodificable.
///
/// Returns:
///     `true` si hay un frame estructuralmente completo (longitud y CRC
///     presentes) que no decodificó: eso es corrupción, no cola rasgada.
fn is_complete_invalid_frame(bytes: &[u8], offset: usize) -> bool {
    let Some(len_bytes) = bytes.get(offset..offset + LEN_SIZE) else {
        return false;
    };
    let Ok(len_bytes) = <[u8; LEN_SIZE]>::try_from(len_bytes) else {
        return false;
    };
    let len = u32::from_le_bytes(len_bytes) as usize;
    if !(BODY_HEADER..=MAX_BODY).contains(&len) {
        return false;
    }
    let frame_end = offset + LEN_SIZE + len + CRC_SIZE;
    frame_end <= bytes.len()
}
/// Recupera el WAL truncando la cola rasgada.
///
/// Args:
///     path: Ruta del archivo de log.
///
/// Returns:
///     El resultado de la recuperación, con la cola inválida descartada.
pub fn recover(path: impl AsRef<Path>) -> Result<RecoveryOutcome, RuscaError> {
    recover_with_key(path, None)
}

/// Recupera el WAL truncando la cola rasgada y descifrando con `key` (SPEC-0013).
///
/// Args:
///     path: Ruta del archivo de log.
///     key: Clave AEAD (`None` = modo claro).
///
/// Returns:
///     El resultado de la recuperación, con la cola inválida descartada.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] ante payload versionado sin clave o AEAD
///     inválido (sin truncar: se preserva la evidencia).
pub fn recover_with_key(
    path: impl AsRef<Path>,
    key: Option<&[u8; 32]>,
) -> Result<RecoveryOutcome, RuscaError> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.as_ref())?;
    scan_and_truncate_with_key(&mut file, key)
}

/// Escanea los bytes del WAL, descifra cada payload y devuelve los registros
/// válidos con el offset del final del último frame válido.
///
/// Un frame estructuralmente inválido (longitud, CRC, tipo) termina el
/// escaneo sin error (cola rasgada). Un payload versionado sin clave, o con
/// AEAD inválido, es [`RuscaError::WalCorrupt`].
///
/// Args:
///     bytes: Contenido íntegro del archivo de log.
///     key: Clave AEAD (`None` = modo claro).
///
/// Returns:
///     Los registros con payloads en claro y el offset válido.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] ante mezcla de modos o AEAD inválido.
fn scan_with_key(
    bytes: &[u8],
    key: Option<&[u8; 32]>,
) -> Result<(Vec<WalRecord>, usize), RuscaError> {
    let mut records = Vec::new();
    let mut offset = 0usize;
    while let Some((raw, next)) = frame::decode(bytes, offset) {
        let payload = open_payload(key, raw.lsn, &raw.payload)?;
        records.push(WalRecord {
            lsn: raw.lsn,
            tx_id: raw.tx_id,
            kind: raw.kind,
            payload,
        });
        offset = next;
    }
    Ok((records, offset))
}

/// Escanea el archivo con `key` y trunca la cola inválida.
///
/// Ante [`RuscaError::WalCorrupt`] (mezcla de modos o AEAD inválido) NO trunca:
/// preserva la evidencia en disco y propaga el error.
fn scan_and_truncate_with_key(
    file: &mut File,
    key: Option<&[u8; 32]>,
) -> Result<RecoveryOutcome, RuscaError> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;

    let (records, valid_offset) = scan_with_key(&bytes, key)?;
    let truncated_bytes = bytes.len() as u64 - valid_offset as u64;
    if truncated_bytes > 0 {
        file.set_len(valid_offset as u64)?;
        file.sync_all()?;
    }
    Ok(RecoveryOutcome {
        records,
        last_valid_offset: valid_offset as u64,
        truncated_bytes,
    })
}

/// Indica si `stored` parece un envelope cifrado (magic `RCE1` + longitud mínima).
///
/// Solo el magic completo acredita el formato; los payloads claros nunca
/// empiezan por `RCE1` (ver [`ENCRYPTED_MAGIC`]).
fn looks_encrypted(stored: &[u8]) -> bool {
    stored.len() >= ENVELOPE_OVERHEAD && stored.starts_with(&ENCRYPTED_MAGIC)
}

/// Deriva el nonce determinista del envelope a partir del LSN.
///
/// Unicidad por monotonía del LSN (sin RNG): `lsn.to_le_bytes() + [0u8; 16]`.
/// El lector lo recomputa y lo compara con el almacenado (anti-splice).
///
/// Args:
///     lsn: Número de secuencia del frame propietario.
///
/// Returns:
///     El nonce de 24 B para `ruscadb_crypto::{seal, open}`.
fn envelope_nonce(lsn: Lsn) -> [u8; ruscadb_crypto::NONCE_SIZE] {
    let mut nonce = [0u8; ruscadb_crypto::NONCE_SIZE];
    nonce[..LSN_SIZE].copy_from_slice(&lsn.to_le_bytes());
    nonce
}

/// Envuelve `payload` en `[RCE1 | 0x01 | nonce 24 B | seal(payload)]`.
///
/// Args:
///     key: Clave simétrica de 32 B.
///     lsn: LSN del frame (fuente del nonce determinista).
///     payload: Carga útil en claro.
///
/// Returns:
///     El envelope listo para persistir como payload del frame.
fn seal_payload(key: &[u8; 32], lsn: Lsn, payload: &[u8]) -> Vec<u8> {
    let nonce = envelope_nonce(lsn);
    let ciphertext = ruscadb_crypto::seal(key, &nonce, payload);
    let mut envelope = Vec::with_capacity(ENVELOPE_OVERHEAD + payload.len());
    envelope.extend_from_slice(&ENCRYPTED_MAGIC);
    envelope.push(ENCRYPTED_VERSION);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    envelope
}

/// Extrae el `(nonce, ciphertext)` de un envelope versionado.
///
/// Args:
///     stored: Payload del frame (debe empezar por el magic `RCE1`).
///
/// Returns:
///     El nonce almacenado y el ciphertext sellado.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] si el envelope está truncado o su versión
///     no es soportada (el AEAD no cubre el byte de versión: un flip debe
///     rechazarse, no silenciarse).
fn parse_envelope(stored: &[u8]) -> Result<([u8; ruscadb_crypto::NONCE_SIZE], &[u8]), RuscaError> {
    if stored.get(4) != Some(&ENCRYPTED_VERSION) {
        return Err(RuscaError::WalCorrupt(format!(
            "versión de envelope no soportada: {:02x?}",
            stored.get(4)
        )));
    }
    let nonce_bytes = stored
        .get(ENVELOPE_HEADER..ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE)
        .filter(|_| stored.len() >= ENVELOPE_OVERHEAD)
        .ok_or_else(|| {
            RuscaError::WalCorrupt(format!(
                "envelope cifrado truncado ({} bytes, mínimo {})",
                stored.len(),
                ENVELOPE_OVERHEAD
            ))
        })?;
    let mut nonce = [0u8; ruscadb_crypto::NONCE_SIZE];
    nonce.copy_from_slice(nonce_bytes);
    Ok((
        nonce,
        &stored[ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE..],
    ))
}

/// Descifra (o valida en claro) el payload de un frame.
///
/// Reglas de modo estricto: sin clave solo se acepta claro; con clave solo se
/// acepta envelope (un frame claro bajo clave es mezcla de modos). El nonce
/// almacenado debe coincidir con el derivado del LSN (anti-splice) y el AEAD
/// debe verificar; cualquier desvío es [`RuscaError::WalCorrupt`].
///
/// Args:
///     key: Clave AEAD (`None` = modo claro).
///     lsn: LSN del frame (verificación del nonce).
///     stored: Payload tal como está en disco.
///
/// Returns:
///     El payload en claro.
///
/// Raises:
///     [`RuscaError::WalCorrupt`] ante mezcla de modos, envelope truncado,
///     nonce ajeno al LSN o AEAD inválido (clave errónea o manipulación).
fn open_payload(key: Option<&[u8; 32]>, lsn: Lsn, stored: &[u8]) -> Result<Vec<u8>, RuscaError> {
    if !looks_encrypted(stored) {
        if key.is_some() {
            return Err(RuscaError::WalCorrupt(format!(
                "frame lsn={lsn} en claro dentro de un WAL cifrado (mezcla de modos)"
            )));
        }
        return Ok(stored.to_vec());
    }
    let key = key.ok_or_else(|| {
        RuscaError::WalCorrupt(format!(
            "frame lsn={lsn} cifrado (magic RCE1) sin clave: abra con clave"
        ))
    })?;
    let (nonce, ciphertext) = parse_envelope(stored)?;
    if nonce != envelope_nonce(lsn) {
        return Err(RuscaError::WalCorrupt(format!(
            "frame lsn={lsn} con nonce ajeno a su LSN (posible splice)"
        )));
    }
    ruscadb_crypto::open(key, &nonce, ciphertext).map_err(|_| {
        RuscaError::WalCorrupt(format!(
            "frame lsn={lsn} no autenticable: clave errónea o manipulación (AEAD)"
        ))
    })
}

/// Codificación y decodificación del frame binario del WAL.
///
/// Aísla el layout (`len`/`body`/`crc`) y los offsets internos del `body`, de
/// modo que el escaneo y la escritura no manipulen bytes directamente.
mod frame {
    use super::{
        BODY_HEADER, CRC_SIZE, KIND_OFFSET, LEN_SIZE, LSN_OFFSET, LSN_SIZE, MAX_BODY,
        PAYLOAD_OFFSET, RecordKind, TX_ID_OFFSET, TX_ID_SIZE, WalRecord,
    };

    /// Serializa un registro a su frame binario.
    pub(super) fn encode(record: &WalRecord) -> Vec<u8> {
        let mut body = Vec::with_capacity(BODY_HEADER + record.payload.len());
        body.extend_from_slice(&record.lsn.to_le_bytes());
        body.extend_from_slice(&record.tx_id.to_le_bytes());
        body.push(record.kind.to_u8());
        body.extend_from_slice(&record.payload);
        let crc = crc32c::crc32c(&body);

        let mut out = Vec::with_capacity(LEN_SIZE + body.len() + CRC_SIZE);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }

    /// Decodifica el frame en `offset`.
    ///
    /// Returns:
    ///     `Some((registro, offset_siguiente))` si el frame es válido, o `None`
    ///     si la cola está rasgada, el CRC no valida o el tipo es desconocido
    ///     (fin del prefijo válido).
    pub(super) fn decode(bytes: &[u8], offset: usize) -> Option<(WalRecord, usize)> {
        let len =
            u32::from_le_bytes(bytes.get(offset..offset + LEN_SIZE)?.try_into().ok()?) as usize;
        if !(BODY_HEADER..=MAX_BODY).contains(&len) {
            return None;
        }
        let body_start = offset + LEN_SIZE;
        let crc_start = body_start + len;
        let frame_end = crc_start + CRC_SIZE;

        let body = bytes.get(body_start..crc_start)?;
        let stored_crc = u32::from_le_bytes(bytes.get(crc_start..frame_end)?.try_into().ok()?);
        if crc32c::crc32c(body) != stored_crc {
            return None;
        }
        decode_body(body).map(|record| (record, frame_end))
    }

    /// Decodifica el `body` de un frame ya validado por CRC.
    fn decode_body(body: &[u8]) -> Option<WalRecord> {
        let lsn = u64::from_le_bytes(
            body.get(LSN_OFFSET..LSN_OFFSET + LSN_SIZE)?
                .try_into()
                .ok()?,
        );
        let tx_id = u64::from_le_bytes(
            body.get(TX_ID_OFFSET..TX_ID_OFFSET + TX_ID_SIZE)?
                .try_into()
                .ok()?,
        );
        let kind = RecordKind::try_from(*body.get(KIND_OFFSET)?).ok()?;
        let payload = body.get(PAYLOAD_OFFSET..)?.to_vec();
        Some(WalRecord {
            lsn,
            tx_id,
            kind,
            payload,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod crash_tests;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs::OpenOptions;

    /// Clave de prueba fija (solo tests; en producción viene de KDF/entorno).
    fn test_key() -> [u8; 32] {
        [0x2au8; 32]
    }

    /// Otra clave de prueba para el caso de clave errónea.
    fn wrong_key() -> [u8; 32] {
        [0x99u8; 32]
    }

    /// Payloads de prueba: vacío, corto, binario y mayor que el envelope mínimo.
    fn sample_payloads() -> Vec<Vec<u8>> {
        vec![
            b"".to_vec(),
            b"a".to_vec(),
            b"secreto-0013-gamma".to_vec(),
            vec![0x01u8; 40],
            vec![0xabu8; 4096],
        ]
    }

    /// Recorre los frames del WAL crudo y devuelve `(inicio, fin)` del `body`.
    ///
    /// Args:
    ///     raw: Contenido íntegro del fichero.
    ///
    /// Returns:
    ///     Un span por frame estructuralmente completo.
    fn frame_spans(raw: &[u8]) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        let mut offset = 0usize;
        while let Some(len_bytes) = raw.get(offset..offset + LEN_SIZE) {
            let len = u32::from_le_bytes(len_bytes.try_into().expect("len")) as usize;
            let body_start = offset + LEN_SIZE;
            let body_end = body_start + len;
            if raw.get(body_end..body_end + CRC_SIZE).is_none() {
                break;
            }
            spans.push((body_start, body_end));
            offset = body_end + CRC_SIZE;
        }
        spans
    }

    /// El presupuesto anti-DoS es 64 MiB exactos (SPEC-0002).
    #[test]
    fn test_max_body_is_64_mib() {
        assert_eq!(MAX_BODY, 67_108_864);
    }

    /// `Debug` indica el modo sin exponer jamás el material de clave.
    #[test]
    fn test_wal_debug_redacts_key_material() {
        let dir = tempfile::tempdir().expect("tempdir");
        let clear = Wal::open(dir.path().join("clear.log")).expect("abre");
        assert!(format!("{clear:?}").contains("encrypted: false"));
        let path = dir.path().join("wal.log");
        let encrypted = Wal::open_encrypted(&path, &test_key()).expect("abre cifrado");
        let shown = format!("{encrypted:?}");
        assert!(shown.contains("encrypted: true"), "debug cifrado: {shown}");
        assert!(
            !shown.contains("42, 42"),
            "la clave no debe fugarse al log: {shown}"
        );
    }
    ///
    /// Escribe commits cifrados, reabre con la misma clave y verifica el replay
    /// exacto, la continuidad del LSN y la ausencia de claro en disco.
    /// Verifica que ningún payload distintivo aparezca en claro en `raw`.
    ///
    /// Args:
    ///     raw: Contenido íntegro del fichero.
    ///     payloads: Payloads en claro que no deben verse en disco.
    fn assert_no_plaintext_on_disk(raw: &[u8], payloads: &[Vec<u8>]) {
        for payload in payloads {
            if payload.len() < 16 {
                continue;
            }
            assert!(
                !raw.windows(payload.len()).any(|w| w == payload.as_slice()),
                "el claro no debe aparecer en disco"
            );
        }
    }

    /// Verifica que el nonce del frame `index` derive de su LSN.
    ///
    /// Args:
    ///     raw: Contenido íntegro del fichero.
    ///     index: Posición del frame (0 = primero).
    ///     lsn: LSN esperado del frame.
    fn assert_nonce_bound_to_lsn(raw: &[u8], index: usize, lsn: Lsn) {
        let spans = frame_spans(raw);
        let payload = &raw[spans[index].0 + PAYLOAD_OFFSET..spans[index].1];
        let mut expected = [0u8; ruscadb_crypto::NONCE_SIZE];
        expected[..LSN_SIZE].copy_from_slice(&lsn.to_le_bytes());
        assert_eq!(&payload[..4], &ENCRYPTED_MAGIC, "magic RCE1");
        assert_eq!(payload[4], ENCRYPTED_VERSION, "versión tras el magic");
        assert_eq!(
            &payload[ENVELOPE_HEADER..ENVELOPE_HEADER + ruscadb_crypto::NONCE_SIZE],
            &expected,
            "nonce derivado del LSN"
        );
    }

    /// AC-0013-01 — roundtrip cifrado del WAL con reapertura.
    ///
    /// Escribe commits cifrados, reabre con la misma clave y verifica el replay
    /// exacto, la continuidad del LSN y la ausencia de claro en disco.
    #[test] // @spec AC-0013-01
    fn test_ac_0013_01_encrypted_wal_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let key = test_key();
        let kinds = [
            RecordKind::Begin,
            RecordKind::Commit,
            RecordKind::Checkpoint,
        ];
        let payloads = sample_payloads();
        {
            let wal = Wal::open_encrypted(&path, &key).expect("abre cifrado");
            for (index, payload) in payloads.iter().enumerate() {
                let lsn = wal
                    .append(index as u64, kinds[index % kinds.len()], payload)
                    .expect("append");
                assert_eq!(lsn, index as u64);
            }
            wal.sync().expect("sync");
        }
        let raw = std::fs::read(&path).expect("lee crudo");
        assert_no_plaintext_on_disk(&raw, &payloads);
        assert_nonce_bound_to_lsn(&raw, 1, 1);
        let records = read_records_with_key(&path, Some(&key)).expect("lee");
        assert_eq!(records.len(), payloads.len());
        for (record, expected) in records.iter().zip(payloads.iter()) {
            assert_eq!(&record.payload, expected);
        }
        let wal = Wal::open_encrypted(&path, &key).expect("reabre");
        assert_eq!(wal.next_lsn(), payloads.len() as u64);
        wal.append(99, RecordKind::Abort, b"post").expect("append");
        assert_eq!(wal.next_lsn(), payloads.len() as u64 + 1);
    }

    /// AC-0013-02 — sin clave o con clave errónea el recovery falla.
    ///
    /// El fichero no se trunca ante el error (se preserva la evidencia) y con
    /// la clave correcta el recovery sigue funcionando.
    #[test] // @spec AC-0013-02
    fn test_ac_0013_02_wrong_or_missing_key_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let key = test_key();
        {
            let wal = Wal::open_encrypted(&path, &key).expect("abre cifrado");
            wal.append(7, RecordKind::Commit, b"dato").expect("append");
            wal.sync().expect("sync");
        }
        let len_before = std::fs::metadata(&path).expect("meta").len();
        assert!(matches!(
            recover_with_key(&path, None),
            Err(RuscaError::WalCorrupt(_))
        ));
        assert!(matches!(
            read_records_with_key(&path, None),
            Err(RuscaError::WalCorrupt(_))
        ));
        assert!(matches!(
            recover_with_key(&path, Some(&wrong_key())),
            Err(RuscaError::WalCorrupt(_))
        ));
        assert!(matches!(
            Wal::open_encrypted(&path, &wrong_key()),
            Err(RuscaError::WalCorrupt(_))
        ));
        let len_after = std::fs::metadata(&path).expect("meta").len();
        assert_eq!(len_before, len_after, "el error no debe truncar");
        let outcome = recover_with_key(&path, Some(&key)).expect("clave correcta");
        assert_eq!(outcome.records.len(), 1);
        assert_eq!(outcome.records[0].payload, b"dato");
    }

    /// Manipulación con CRC recalculado: el AEAD la detecta (`WalCorrupt`, sin truncar).
    #[test]
    fn test_encrypted_wal_crc_preserving_tamper_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let key = test_key();
        {
            let wal = Wal::open_encrypted(&path, &key).expect("abre");
            wal.append(1, RecordKind::Commit, b"integro")
                .expect("append");
            wal.sync().expect("sync");
        }
        let len_before = std::fs::metadata(&path).expect("meta").len();
        let mut raw = std::fs::read(&path).expect("lee crudo");
        let spans = frame_spans(&raw);
        assert_eq!(spans.len(), 1);
        let (start, end) = spans[0];
        raw[start + PAYLOAD_OFFSET] ^= 0x01;
        let crc = crc32c::crc32c(&raw[start..end]);
        raw[end..end + CRC_SIZE].copy_from_slice(&crc.to_le_bytes());
        std::fs::write(&path, &raw).expect("escribe");
        assert!(matches!(
            recover_with_key(&path, Some(&key)),
            Err(RuscaError::WalCorrupt(_))
        ));
        assert_eq!(
            std::fs::metadata(&path).expect("meta").len(),
            len_before,
            "el error preserva la evidencia"
        );
    }

    /// Corrupción ingenua (CRC roto) en la cola: equivale a torn write (trunca, `Ok`).
    #[test]
    fn test_encrypted_wal_broken_crc_tail_truncates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let key = test_key();
        {
            let wal = Wal::open_encrypted(&path, &key).expect("abre");
            wal.append(1, RecordKind::Begin, b"uno").expect("append");
            wal.append(1, RecordKind::Commit, b"dos").expect("append");
            wal.sync().expect("sync");
        }
        let mut raw = std::fs::read(&path).expect("lee crudo");
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        std::fs::write(&path, &raw).expect("escribe");
        let outcome = recover_with_key(&path, Some(&key)).expect("recover");
        assert_eq!(outcome.records.len(), 1);
        assert_eq!(outcome.records[0].payload, b"uno");
        assert!(outcome.truncated_bytes > 0);
    }

    /// El límite de tamaño incluye el sobrecoste del envelope (45 B).
    #[test]
    fn test_encrypted_append_rejects_oversized_with_overhead() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let wal = Wal::open_encrypted(&path, &test_key()).expect("abre");
        let limit = MAX_BODY - BODY_HEADER - ENVELOPE_OVERHEAD;
        wal.append(1, RecordKind::Commit, &vec![0x55u8; limit])
            .expect("ajuste exacto ok");
        assert!(matches!(
            wal.append(1, RecordKind::Commit, &vec![0x55u8; limit + 1]),
            Err(RuscaError::WalCorrupt(_))
        ));
        assert_eq!(wal.next_lsn(), 1, "no se asigna LSN si el append falla");
    }

    /// Barrido determinista: varias claves × tamaños × kinds en roundtrip.
    #[test]
    fn test_encrypted_wal_roundtrip_many_payloads() {
        let keys = [[0u8; 32], [0xFFu8; 32], test_key()];
        let lens = [0usize, 1, 40, 41, 100, 5000];
        let kinds = [
            RecordKind::Begin,
            RecordKind::Commit,
            RecordKind::Abort,
            RecordKind::Checkpoint,
        ];
        for key in keys {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("wal.log");
            let mut expected = Vec::new();
            {
                let wal = Wal::open_encrypted(&path, &key).expect("abre");
                for (index, len) in lens.iter().enumerate() {
                    let payload = vec![(index + 1) as u8; *len];
                    wal.append(index as u64, kinds[index % kinds.len()], &payload)
                        .expect("append");
                    expected.push(payload);
                }
                wal.sync().expect("sync");
            }
            let records = read_records_with_key(&path, Some(&key)).expect("lee");
            assert_eq!(records.len(), expected.len());
            for (record, payload) in records.iter().zip(expected.iter()) {
                assert_eq!(&record.payload, payload);
            }
        }
    }

    /// Modo claro intacto: un claro que empieza por `0x01` es claro legítimo.
    ///
    /// Regresión: un commit de una sola página empieza por `count = 1` LE
    /// (`01 00 00 00…`); el detector por magic `RCE1` nunca lo confunde con
    /// un envelope cifrado.
    #[test]
    fn test_clear_payload_starting_with_0x01_is_plaintext() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let wal = Wal::open(&path).expect("abre");
        wal.append(1, RecordKind::Commit, b"\x01corto")
            .expect("append");
        wal.sync().expect("sync");
        drop(wal);
        let records = read_records(&path).expect("lee en claro");
        assert_eq!(records[0].payload, b"\x01corto");
        assert!(matches!(
            read_records_with_key(&path, Some(&test_key())),
            Err(RuscaError::WalCorrupt(_))
        ));
        // Un claro largo que empieza por 0x01 (forma de un commit de 1
        // página) sigue siendo claro: solo el magic RCE1 marca cifrado.
        let single_page_commit = vec![0x01u8; ENVELOPE_OVERHEAD];
        let wal = Wal::open(&path).expect("reabre");
        wal.append(2, RecordKind::Commit, &single_page_commit)
            .expect("append");
        wal.sync().expect("sync");
        drop(wal);
        let records = read_records(&path).expect("lee en claro");
        assert_eq!(records[1].payload, single_page_commit);
    }

    /// AC-0002-01 — un COMMIT sincronizado sobrevive al cierre del proceso.
    #[test]
    // @spec AC-0002-01
    fn test_ac_0002_01_wal_durability_after_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let wal = Wal::open(&path).expect("abre");
            wal.append(1, RecordKind::Commit, b"row-1").expect("append");
            wal.sync().expect("sync");
        }
        let records = read_records(&path).expect("lee");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload, b"row-1");
        assert_eq!(records[0].kind, RecordKind::Commit);
    }

    /// AC-0002-02 — el recovery trunca la cola rasgada.
    #[test]
    // @spec AC-0002-02
    fn test_ac_0002_02_wal_recovery_truncated_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let valid_len = {
            let wal = Wal::open(&path).expect("abre");
            wal.append(1, RecordKind::Begin, b"a").expect("append");
            wal.append(1, RecordKind::Commit, b"b").expect("append");
            wal.sync().expect("sync");
            std::fs::metadata(&path).expect("meta").len()
        };

        // Simula torn write: 4 bytes que prometen un body inexistente.
        {
            let mut file = OpenOptions::new().append(true).open(&path).expect("reabre");
            file.write_all(&[0xFF, 0x00, 0x00, 0x00]).expect("basura");
            file.sync_all().expect("sync");
        }

        let outcome = recover(&path).expect("recover");
        assert_eq!(outcome.records.len(), 2);
        assert!(outcome.truncated_bytes >= 4);
        let final_len = std::fs::metadata(&path).expect("meta").len();
        assert_eq!(final_len, valid_len);
        assert_eq!(final_len, outcome.last_valid_offset);
    }

    /// AC-0002-03 — el replay del WAL es idempotente.
    #[test]
    // @spec AC-0002-03
    fn test_ac_0002_03_wal_replay_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let wal = Wal::open(&path).expect("abre");
            wal.append(1, RecordKind::Begin, b"a").expect("append");
            wal.append(1, RecordKind::Commit, b"b").expect("append");
            wal.sync().expect("sync");
        }

        let first = recover(&path).expect("recover 1");
        let second = recover(&path).expect("recover 2");
        assert_eq!(first.records, second.records);
        assert_eq!(second.truncated_bytes, 0);

        // Aplicar el replay dos veces produce el mismo estado.
        let mut model: BTreeMap<Lsn, Vec<u8>> = BTreeMap::new();
        for record in &first.records {
            model.insert(record.lsn, record.payload.clone());
        }
        let once = model.clone();
        for record in &second.records {
            model.insert(record.lsn, record.payload.clone());
        }
        assert_eq!(once, model);
    }

    /// Un payload por encima del máximo se rechaza antes de escribir (no se
    /// pierde silenciosamente en recovery).
    #[test]
    fn test_append_rejects_oversized_payload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let wal = Wal::open(&path).expect("abre");
        let oversized = vec![0u8; MAX_BODY];
        let result = wal.append(1, RecordKind::Commit, &oversized);
        assert!(matches!(result, Err(RuscaError::WalCorrupt(_))));
        assert_eq!(wal.next_lsn(), 0, "no se asigna LSN si el append falla");
    }

    /// `verify` en un WAL sano: sin corrupción y sin cola no decodificable.
    #[test]
    fn test_verify_healthy_wal_is_not_corrupted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let wal = Wal::open(&path).expect("abre");
            wal.append(1, RecordKind::Begin, b"uno").expect("append");
            wal.append(1, RecordKind::Commit, b"dos").expect("append");
            wal.sync().expect("sync");
        }
        let verification = verify(&path, None).expect("verifica");
        assert!(!verification.corrupted, "un WAL sano no está corrupto");
        assert_eq!(verification.trailing_bytes, 0);
        assert_eq!(
            verification.corrupt_offset, None,
            "no debe haber offset de corrupción"
        );
    }

    /// `verify` distingue la corrupción interna (frame completo, CRC roto) de
    /// la cola rasgada: un byte alterado en el primer frame es corrupción.
    #[test]
    fn test_verify_detects_mid_wal_corruption() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let wal = Wal::open(&path).expect("abre");
            wal.append(1, RecordKind::Begin, b"a").expect("append");
            wal.append(1, RecordKind::Commit, b"b").expect("append");
            wal.sync().expect("sync");
        }
        let mut raw = std::fs::read(&path).expect("lee crudo");
        let spans = frame_spans(&raw);
        assert_eq!(spans.len(), 2, "dos frames esperados");
        raw[spans[0].0 + PAYLOAD_OFFSET] ^= 0xFF;
        std::fs::write(&path, &raw).expect("escribe corrupto");

        let verification = verify(&path, None).expect("verifica");
        assert!(
            verification.corrupted,
            "el frame completo con CRC roto es corrupción"
        );
        assert_eq!(
            verification.corrupt_offset,
            Some(0),
            "la corrupción empieza donde arranca el primer frame inválido"
        );
        assert!(
            verification.trailing_bytes >= spans[0].1 - spans[0].0,
            "la corrupción no se confunde con cola pequeña"
        );
    }

    /// `verify` no confunde una cola rasgada legítima (crash a mitad de frame)
    /// con corrupción.
    #[test]
    fn test_verify_torn_tail_is_not_corruption() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let wal = Wal::open(&path).expect("abre");
            wal.append(1, RecordKind::Commit, b"completo")
                .expect("append");
            wal.sync().expect("sync");
        }
        // Simula un crash a mitad de la escritura del segundo frame: 3 bytes
        // de cabecera (longitud) sin cuerpo ni CRC.
        let partial = [0x10u8, 0x00, 0x00];
        use std::fs::OpenOptions;
        use std::io::{Seek, SeekFrom, Write};
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("abre para rasgar");
        file.seek(SeekFrom::End(0)).expect("seek al final");
        file.write_all(&partial).expect("escribe cola rasgada");
        drop(file);

        let verification = verify(&path, None).expect("verifica");
        assert!(
            !verification.corrupted,
            "una cola incompleta se trunca, no corrompe"
        );
        assert!(
            verification.trailing_bytes == partial.len(),
            "los bytes rasgados quedan contabilizados: {}",
            verification.trailing_bytes
        );
    }

    /// `FsyncPolicy::Full` es la política por defecto (NF-0081-03).
    #[test]
    fn test_fsync_policy_defaults_to_full() {
        assert_eq!(FsyncPolicy::default(), FsyncPolicy::Full);
        let dir = tempfile::tempdir().expect("tempdir");
        let wal = Wal::open(dir.path().join("wal.log")).expect("abre");
        assert_eq!(wal.policy(), FsyncPolicy::Full);
        assert_eq!(wal.sync_count(), 0, "sin fsyncs previos");
    }

    /// Con `Full` cada `sync` hace exactamente un fsync (contador observable).
    #[test]
    fn test_full_policy_counts_one_fsync_per_sync() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let wal = Wal::open(&path).expect("abre");
        for index in 0..3u64 {
            wal.append(index, RecordKind::Commit, b"x").expect("append");
            wal.sync().expect("sync");
        }
        assert_eq!(wal.sync_count(), 3, "un fsync por commit en Full");
    }

    /// Con `Normal` todos los acks son durables (sobreviven a reapertura).
    #[test]
    fn test_normal_policy_acks_are_durable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let wal = Wal::open_with(&path, FsyncPolicy::Normal).expect("abre");
            assert_eq!(wal.policy(), FsyncPolicy::Normal);
            for index in 0..5u64 {
                wal.append(index, RecordKind::Commit, format!("d-{index}").as_bytes())
                    .expect("append");
                wal.sync().expect("sync");
            }
            assert!(wal.sync_count() >= 1, "al menos un fsync por grupo");
        }
        let records = read_records(&path).expect("lee");
        assert_eq!(records.len(), 5);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record.payload, format!("d-{index}").as_bytes());
        }
    }

    /// Una ráfaga concurrente con `Normal` coalesce fsyncs (AC-0081-02, unit).
    #[test]
    fn test_normal_policy_coalesces_fsyncs_under_burst() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let wal = std::sync::Arc::new(Wal::open_with(&path, FsyncPolicy::Normal).expect("abre"));
        let threads = 32usize;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(threads));
        let mut handles = Vec::with_capacity(threads);
        for index in 0..threads {
            let wal = std::sync::Arc::clone(&wal);
            let barrier = std::sync::Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                wal.append(
                    index as u64,
                    RecordKind::Commit,
                    format!("rec-{index}").as_bytes(),
                )
                .expect("append concurrente");
                wal.sync().expect("sync concurrente");
            }));
        }
        for handle in handles {
            handle.join().expect("join");
        }
        let syncs = wal.sync_count();
        assert!(
            syncs < threads as u64,
            "coalescing medible: {syncs} fsyncs para {threads} commits"
        );
        drop(wal);
        let records = read_records(&path).expect("lee");
        assert_eq!(records.len(), threads, "los N acks persisten en el WAL");
        for record in &records {
            assert_eq!(record.kind, RecordKind::Commit);
        }
    }

    /// El writer thread se une en `Drop` sin deadlock (NF-0081-02).
    #[test]
    fn test_normal_policy_drop_joins_writer_without_deadlock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let wal = Wal::open_with(&path, FsyncPolicy::Normal).expect("abre");
            wal.append(1, RecordKind::Begin, b"x").expect("append");
            // Sin `sync`: el drop debe drenar y unir el writer sin bloquearse.
        }
        let wal = Wal::open_with(&path, FsyncPolicy::Normal).expect("reabre");
        assert_eq!(wal.next_lsn(), 1, "el frame escrito se recupera");
    }

    /// `Normal` en modo cifrado: roundtrip con acks durables (SPEC-0013 + 0081).
    #[test]
    fn test_open_encrypted_with_normal_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let key = test_key();
        {
            let wal =
                Wal::open_encrypted_with(&path, &key, FsyncPolicy::Normal).expect("abre cifrado");
            for index in 0..3u64 {
                wal.append(index, RecordKind::Commit, format!("e-{index}").as_bytes())
                    .expect("append");
                wal.sync().expect("sync");
            }
        }
        let records = read_records_with_key(&path, Some(&key)).expect("lee");
        assert_eq!(records.len(), 3);
        assert_eq!(records[2].payload, b"e-2");
    }
}
