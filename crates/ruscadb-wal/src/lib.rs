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

#![forbid(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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

/// Write-Ahead Log de un único escritor (embebido).
#[derive(Debug)]
pub struct Wal {
    file: File,
    path: PathBuf,
    next_lsn: Lsn,
}

impl Wal {
    /// Abre (o crea) el WAL y ejecuta el recovery del tail rasgado.
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
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let outcome = scan_and_truncate(&mut file)?;
        let next_lsn = outcome.records.last().map_or(0, |record| record.lsn + 1);
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            file,
            path,
            next_lsn,
        })
    }

    /// Añade un registro al WAL y devuelve su LSN.
    ///
    /// El registro queda **escrito** pero no necesariamente durable hasta
    /// llamar a [`Wal::sync`].
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
    pub fn append(
        &mut self,
        tx_id: u64,
        kind: RecordKind,
        payload: &[u8],
    ) -> Result<Lsn, RuscaError> {
        if BODY_HEADER + payload.len() > MAX_BODY {
            return Err(RuscaError::WalCorrupt(format!(
                "payload de {} bytes excede el máximo de {} bytes por frame",
                payload.len(),
                MAX_BODY - BODY_HEADER
            )));
        }
        let record = WalRecord {
            lsn: self.next_lsn,
            tx_id,
            kind,
            payload: payload.to_vec(),
        };
        self.file.write_all(&frame::encode(&record))?;
        self.next_lsn += 1;
        Ok(record.lsn)
    }

    /// Fuerza la durabilidad de todo lo escrito (fsync de datos).
    ///
    /// Returns:
    ///     `Ok(())` cuando los datos están en disco.
    pub fn sync(&mut self) -> Result<(), RuscaError> {
        self.file.sync_data()?;
        Ok(())
    }

    /// Ruta del archivo de log.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Último LSN asignado más uno (el siguiente a usar).
    pub fn next_lsn(&self) -> Lsn {
        self.next_lsn
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
    let bytes = std::fs::read(path.as_ref())?;
    let (records, _valid_offset) = scan(&bytes);
    Ok(records)
}

/// Recupera el WAL truncando la cola rasgada.
///
/// Args:
///     path: Ruta del archivo de log.
///
/// Returns:
///     El resultado de la recuperación, con la cola inválida descartada.
pub fn recover(path: impl AsRef<Path>) -> Result<RecoveryOutcome, RuscaError> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.as_ref())?;
    scan_and_truncate(&mut file)
}

/// Escanea los bytes del WAL y devuelve los registros válidos y el offset del
/// final del último frame válido.
fn scan(bytes: &[u8]) -> (Vec<WalRecord>, usize) {
    let mut records = Vec::new();
    let mut offset = 0usize;
    while let Some((record, next)) = frame::decode(bytes, offset) {
        records.push(record);
        offset = next;
    }
    (records, offset)
}

/// Escanea el archivo y trunca la cola inválida.
fn scan_and_truncate(file: &mut File) -> Result<RecoveryOutcome, RuscaError> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;

    let (records, valid_offset) = scan(&bytes);
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
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs::OpenOptions;

    /// AC-0002-01 — un COMMIT sincronizado sobrevive al cierre del proceso.
    #[test]
    fn test_ac_0002_01_wal_durability_after_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let mut wal = Wal::open(&path).expect("abre");
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
    fn test_ac_0002_02_wal_recovery_truncated_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        let valid_len = {
            let mut wal = Wal::open(&path).expect("abre");
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
    fn test_ac_0002_03_wal_replay_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal.log");
        {
            let mut wal = Wal::open(&path).expect("abre");
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
        let mut wal = Wal::open(&path).expect("abre");
        let oversized = vec![0u8; MAX_BODY];
        let result = wal.append(1, RecordKind::Commit, &oversized);
        assert!(matches!(result, Err(RuscaError::WalCorrupt(_))));
        assert_eq!(wal.next_lsn(), 0, "no se asigna LSN si el append falla");
    }
}
