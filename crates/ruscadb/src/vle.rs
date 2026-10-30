//! Single-file VLE (SPEC-0137): contenedor de registros con checksum.
//!
//! Formato binario autocontenido en un unico fichero:
//! `magic u64 | version u32 | count u32 | (key_len u32, key, value_len u32,
//! value, checksum u32)*`. El checksum (FNV-1a) detecta truncamientos y
//! corrupcion; `read_all` valida cotas antes de reservar (sin panics).

use std::fs;
use std::path::Path;

use ruscadb_core::RuscaError;

/// Magia del contenedor (`"RUSCVLE1"`).
const VLE_MAGIC: u64 = 0x5255_5343_564C_4531;
/// Version del formato.
const VLE_VERSION: u32 = 1;

/// Registro del contenedor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VleRecord {
    /// Clave UTF-8.
    pub key: String,
    /// Valor binario.
    pub value: Vec<u8>,
}

/// Base VLE (single-file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VleDatabase {
    /// Ruta del fichero unico.
    pub path: String,
}

impl VleDatabase {
    /// Crea la referencia a una base VLE.
    ///
    /// Args:
    ///     path: Ruta del fichero.
    ///
    /// Returns:
    ///     Referencia lista.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    /// Ruta del fichero.
    ///
    /// Returns:
    ///     Ruta declarada.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Escribe todos los registros (sobrescribe el fichero).
    ///
    /// Args:
    ///     records: Registros a persistir.
    ///
    /// Returns:
    ///     Numero de registros escritos.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura; [`RuscaError::ResourceLimit`]
    ///     si una clave/valor excede `u32`.
    pub fn create(&self, records: &[VleRecord]) -> Result<usize, RuscaError> {
        let mut buffer = Vec::with_capacity(16 + records.len() * 32);
        buffer.extend_from_slice(&VLE_MAGIC.to_le_bytes());
        buffer.extend_from_slice(&VLE_VERSION.to_le_bytes());
        let count = u32::try_from(records.len()).map_err(|_| RuscaError::ResourceLimit {
            resource: "vle_records".to_string(),
            limit: u64::from(u32::MAX),
            actual: records.len() as u64,
        })?;
        buffer.extend_from_slice(&count.to_le_bytes());
        for record in records {
            let key = record.key.as_bytes();
            let key_len = u32::try_from(key.len()).map_err(|_| RuscaError::ResourceLimit {
                resource: "vle_key".to_string(),
                limit: u64::from(u32::MAX),
                actual: key.len() as u64,
            })?;
            let value_len =
                u32::try_from(record.value.len()).map_err(|_| RuscaError::ResourceLimit {
                    resource: "vle_value".to_string(),
                    limit: u64::from(u32::MAX),
                    actual: record.value.len() as u64,
                })?;
            buffer.extend_from_slice(&key_len.to_le_bytes());
            buffer.extend_from_slice(key);
            buffer.extend_from_slice(&value_len.to_le_bytes());
            buffer.extend_from_slice(&record.value);
            buffer.extend_from_slice(&checksum(key, &record.value).to_le_bytes());
        }
        fs::write(&self.path, buffer)?;
        Ok(records.len())
    }

    /// Lee y valida todos los registros.
    ///
    /// Returns:
    ///     Registros en orden de escritura.
    ///
    /// Errors:
    ///     [`RuscaError::CorruptManifest`] si la magia/version/checksum no
    ///     cuadran o el fichero esta truncado; [`RuscaError::Io`] si falla la
    ///     lectura.
    pub fn read_all(&self) -> Result<Vec<VleRecord>, RuscaError> {
        let bytes = fs::read(&self.path)?;
        if bytes.len() < 16 {
            return Err(corrupt("cabecera VLE truncada"));
        }
        let magic = u64::from_le_bytes(bytes[0..8].try_into().unwrap_or([0; 8]));
        if magic != VLE_MAGIC {
            return Err(corrupt("magia VLE invalida"));
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap_or([0; 4]));
        if version != VLE_VERSION {
            return Err(corrupt("version VLE no soportada"));
        }
        let count = u32::from_le_bytes(bytes[12..16].try_into().unwrap_or([0; 4])) as usize;
        let mut cursor = 16usize;
        let mut records = Vec::with_capacity(count.min(1_024));
        for index in 0..count {
            let key_len = read_u32(&bytes, &mut cursor, index)? as usize;
            let key = read_slice(&bytes, &mut cursor, key_len, index)?;
            let value_len = read_u32(&bytes, &mut cursor, index)? as usize;
            let value = read_slice(&bytes, &mut cursor, value_len, index)?;
            let expected = read_u32(&bytes, &mut cursor, index)?;
            if checksum(key, value) != expected {
                return Err(corrupt("checksum VLE no coincide"));
            }
            let key_text =
                String::from_utf8(key.to_vec()).map_err(|_| corrupt("clave VLE no es UTF-8"))?;
            records.push(VleRecord {
                key: key_text,
                value: value.to_vec(),
            });
        }
        if cursor != bytes.len() {
            return Err(corrupt("bytes sobrantes tras el ultimo registro"));
        }
        Ok(records)
    }

    /// Anade un registro (reescribe el contenedor).
    ///
    /// Coste `O(n)` por el rewrite completo: para cargas masivas usar
    /// [`Self::create`] con el lote ya construido.
    ///
    /// Args:
    ///     key: Clave.
    ///     value: Valor.
    ///
    /// Returns:
    ///     Numero de registros tras el append.
    ///
    /// Errors:
    ///     Las de [`Self::read_all`] y [`Self::create`].
    pub fn append(
        &self,
        key: impl Into<String>,
        value: impl Into<Vec<u8>>,
    ) -> Result<usize, RuscaError> {
        let mut records = if Path::new(&self.path).exists() {
            self.read_all()?
        } else {
            Vec::new()
        };
        records.push(VleRecord {
            key: key.into(),
            value: value.into(),
        });
        self.create(&records)
    }

    /// Numero de registros.
    ///
    /// Returns:
    ///     Cardinalidad.
    ///
    /// Errors:
    ///     Las de [`Self::read_all`].
    pub fn len(&self) -> Result<usize, RuscaError> {
        Ok(self.read_all()?.len())
    }

    /// Indica si el contenedor no tiene registros.
    ///
    /// Returns:
    ///     `true` si vacio o el fichero no existe.
    ///
    /// Errors:
    ///     Las de [`Self::read_all`] si el fichero existe y esta corrupto.
    pub fn is_empty(&self) -> Result<bool, RuscaError> {
        if !Path::new(&self.path).exists() {
            return Ok(true);
        }
        Ok(self.len()? == 0)
    }
}

/// Error de corrupcion accionable.
fn corrupt(detail: &str) -> RuscaError {
    RuscaError::CorruptManifest(format!("VLE: {detail}"))
}

/// Lee un `u32` LE avanzando el cursor.
fn read_u32(bytes: &[u8], cursor: &mut usize, index: usize) -> Result<u32, RuscaError> {
    let end = cursor.saturating_add(4);
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| corrupt(&format!("registro {index} truncado")))?;
    *cursor = end;
    Ok(u32::from_le_bytes(slice.try_into().unwrap_or([0; 4])))
}

/// Lee `len` bytes avanzando el cursor.
fn read_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    index: usize,
) -> Result<&'a [u8], RuscaError> {
    let end = cursor.saturating_add(len);
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| corrupt(&format!("registro {index} truncado")))?;
    *cursor = end;
    Ok(slice)
}

/// FNV-1a de 32 bits sobre clave + valor (separador incluido).
fn checksum(key: &[u8], value: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in key.iter().chain(&0xFFu8.to_le_bytes()).chain(value) {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{VleDatabase, VleRecord};

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ruscadb-vle-{tag}-{}", std::process::id()))
    }

    #[test] // @spec AC-0137-01
    fn test_ac_0137_01_vle_create() {
        let database = VleDatabase::new("test.pvdb");
        assert_eq!(database.path(), "test.pvdb");
    }

    /// Round-trip de registros y append.
    #[test]
    fn vle_roundtrip_and_append() {
        let path = temp_path("rt");
        let _ = std::fs::remove_file(&path);
        let database = VleDatabase::new(path.to_string_lossy().into_owned());
        assert!(database.is_empty().expect("empty"));
        let records = vec![
            VleRecord {
                key: "a".to_string(),
                value: b"alpha".to_vec(),
            },
            VleRecord {
                key: "bb".to_string(),
                value: Vec::new(),
            },
        ];
        assert_eq!(database.create(&records).expect("create"), 2);
        assert_eq!(database.read_all().expect("read"), records);
        assert!(!database.is_empty().expect("empty"));

        assert_eq!(database.append("c", b"gamma".to_vec()).expect("append"), 3);
        let restored = database.read_all().expect("read");
        assert_eq!(restored.len(), 3);
        assert_eq!(restored[2].key, "c");
        let _ = std::fs::remove_file(&path);
    }

    /// Corrupcion y truncamiento se detectan (sin panic).
    #[test]
    fn vle_detects_corruption() {
        let path = temp_path("corrupt");
        let _ = std::fs::remove_file(&path);
        let database = VleDatabase::new(path.to_string_lossy().into_owned());
        database
            .create(&[VleRecord {
                key: "k".to_string(),
                value: b"v".to_vec(),
            }])
            .expect("create");

        // Truncado.
        let bytes = std::fs::read(&path).expect("read");
        std::fs::write(&path, &bytes[..bytes.len() - 1]).expect("truncate");
        assert!(database.read_all().is_err());

        // Byte alterado (checksum).
        let mut corrupted = bytes.clone();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0x01;
        std::fs::write(&path, &corrupted).expect("write");
        assert!(database.read_all().is_err());

        // Magia invalida.
        let mut wrong_magic = bytes;
        wrong_magic[0] ^= 0xFF;
        std::fs::write(&path, &wrong_magic).expect("write");
        assert!(database.read_all().is_err());
        let _ = std::fs::remove_file(&path);
    }
}
