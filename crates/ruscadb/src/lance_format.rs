//! Formato columnar single-file (SPEC-0136): contenedor tipo Lance propio.
//!
//! **No es compatible con el formato Lance** (requiere el crate `lance`): este
//! contenedor serializa columnas tipadas con magia/version y permite el
//! round-trip exacto sin dependencias. La compresion ALP/FastLanes queda como
//! evolucion (hoy los valores van crudos para no acoplar features).

use std::collections::BTreeMap;

/// Tipo de columna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    /// Enteros de 64 bits.
    Int,
    /// Flotantes de 64 bits.
    Float,
}

/// Columna tipada.
#[derive(Debug, Clone, PartialEq)]
pub enum Column {
    /// Valores enteros.
    Int(Vec<i64>),
    /// Valores flotantes.
    Float(Vec<f64>),
}

impl Column {
    /// Tipo de la columna.
    ///
    /// Returns:
    ///     `ColumnKind`.
    pub fn kind(&self) -> ColumnKind {
        match self {
            Column::Int(_) => ColumnKind::Int,
            Column::Float(_) => ColumnKind::Float,
        }
    }

    /// Numero de valores.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        match self {
            Column::Int(values) => values.len(),
            Column::Float(values) => values.len(),
        }
    }

    /// Indica si no hay valores.
    ///
    /// Returns:
    ///     `true` si vacia.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Magia del contenedor (`"RLNC1"`).
const MAGIC: &[u8; 5] = b"RLNC1";
/// Version del formato.
const VERSION: u32 = 1;

/// Tabla estilo Lance (columnar, en memoria).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LanceTable {
    /// Nombre de la tabla.
    pub name: String,
    /// Filas declaradas (contrato historico de `append`).
    pub row_count: usize,
    /// Columnas por nombre.
    pub columns: BTreeMap<String, Column>,
}

impl LanceTable {
    /// Crea una tabla vacia.
    ///
    /// Args:
    ///     name: Nombre de la tabla.
    ///
    /// Returns:
    ///     Tabla vacia.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            row_count: 0,
            columns: BTreeMap::new(),
        }
    }

    /// Anade una fila logica sin columnas (compatibilidad).
    pub fn append(&mut self) {
        self.row_count += 1;
    }

    /// Numero de filas (columnas si existen; si no, el contador legacy).
    ///
    /// Returns:
    ///     Filas.
    pub fn len(&self) -> usize {
        self.columns
            .values()
            .map(Column::len)
            .max()
            .unwrap_or(self.row_count)
    }

    /// Indica si no hay filas.
    ///
    /// Returns:
    ///     `true` si vacia.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Anade un entero a una columna (la crea si falta).
    ///
    /// Args:
    ///     column: Nombre.
    ///     value: Valor.
    ///
    /// Errors:
    ///     Texto si la columna existente es de otro tipo.
    pub fn push_int(&mut self, column: &str, value: i64) -> Result<(), String> {
        match self
            .columns
            .entry(column.to_string())
            .or_insert_with(|| Column::Int(Vec::new()))
        {
            Column::Int(values) => {
                values.push(value);
                Ok(())
            }
            _ => Err(format!("la columna '{column}' no es INT")),
        }
    }

    /// Anade un flotante a una columna (la crea si falta).
    ///
    /// Args:
    ///     column: Nombre.
    ///     value: Valor.
    ///
    /// Errors:
    ///     Texto si la columna existente es de otro tipo.
    pub fn push_float(&mut self, column: &str, value: f64) -> Result<(), String> {
        match self
            .columns
            .entry(column.to_string())
            .or_insert_with(|| Column::Float(Vec::new()))
        {
            Column::Float(values) => {
                values.push(value);
                Ok(())
            }
            _ => Err(format!("la columna '{column}' no es FLOAT")),
        }
    }

    /// Serializa la tabla al contenedor columnar.
    ///
    /// Formato: `magic|version|row_count|name|columns`, con todas las
    /// longitudes y cardinalidades en **u32**; el consumo en memoria esta
    /// acotado por ese limite (no se contemplan tablas de mas de 4 294 967 295
    /// valores por columna).
    ///
    /// Returns:
    ///     Bytes `RLNC1`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.row_count as u32).to_le_bytes());
        let name = self.name.as_bytes();
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&(self.columns.len() as u32).to_le_bytes());
        for (column_name, column) in &self.columns {
            let name = column_name.as_bytes();
            out.extend_from_slice(&(name.len() as u32).to_le_bytes());
            out.extend_from_slice(name);
            out.push(match column.kind() {
                ColumnKind::Int => 0,
                ColumnKind::Float => 1,
            });
            out.extend_from_slice(&(column.len() as u32).to_le_bytes());
            match column {
                Column::Int(values) => {
                    for value in values {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
                Column::Float(values) => {
                    for value in values {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
        }
        out
    }

    /// Deserializa un contenedor columnar.
    ///
    /// Args:
    ///     bytes: Bytes producidos por [`Self::to_bytes`].
    ///
    /// Returns:
    ///     Tabla reconstruida.
    ///
    /// Errors:
    ///     Texto accionable si la magia, la version o una cota son invalidas.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 17 || &bytes[0..5] != MAGIC {
            return Err("contenedor: magia invalida o truncado".to_string());
        }
        let mut cursor = 5usize;
        let version = read_u32(bytes, &mut cursor)?;
        if version != VERSION {
            return Err(format!("contenedor: version {version} no soportada"));
        }
        let row_count = read_u32(bytes, &mut cursor)? as usize;
        let name_len = read_u32(bytes, &mut cursor)? as usize;
        let name = read_text(bytes, &mut cursor, name_len)?;
        let column_count = read_u32(bytes, &mut cursor)? as usize;
        let mut columns = BTreeMap::new();
        for _ in 0..column_count {
            let column_name_len = read_u32(bytes, &mut cursor)? as usize;
            let column_name = read_text(bytes, &mut cursor, column_name_len)?;
            let kind = *bytes
                .get(cursor)
                .ok_or_else(|| "contenedor: tipo truncado".to_string())?;
            cursor += 1;
            let count = read_u32(bytes, &mut cursor)? as usize;
            match kind {
                0 => {
                    let mut values = Vec::with_capacity(count.min(1_024));
                    for _ in 0..count {
                        let raw = read_u64(bytes, &mut cursor)?;
                        values.push(raw as i64);
                    }
                    columns.insert(column_name, Column::Int(values));
                }
                1 => {
                    let mut values = Vec::with_capacity(count.min(1_024));
                    for _ in 0..count {
                        values.push(f64::from_bits(read_u64(bytes, &mut cursor)?));
                    }
                    columns.insert(column_name, Column::Float(values));
                }
                other => return Err(format!("contenedor: tipo {other} invalido")),
            }
        }
        if cursor != bytes.len() {
            return Err("contenedor: bytes sobrantes".to_string());
        }
        Ok(Self {
            name,
            row_count,
            columns,
        })
    }
}

/// Lee un `u32` LE.
fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let end = cursor.saturating_add(4);
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| "contenedor: u32 truncado".to_string())?;
    *cursor = end;
    Ok(u32::from_le_bytes(slice.try_into().unwrap_or([0; 4])))
}

/// Lee un `u64` LE.
fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let end = cursor.saturating_add(8);
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| "contenedor: u64 truncado".to_string())?;
    *cursor = end;
    Ok(u64::from_le_bytes(slice.try_into().unwrap_or([0; 8])))
}

/// Lee `len` bytes como UTF-8.
fn read_text(bytes: &[u8], cursor: &mut usize, len: usize) -> Result<String, String> {
    let end = cursor.saturating_add(len);
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| "contenedor: texto truncado".to_string())?;
    *cursor = end;
    String::from_utf8(slice.to_vec()).map_err(|_| "contenedor: UTF-8 invalido".to_string())
}

#[cfg(test)]
mod tests {
    use super::{Column, LanceTable};

    #[test] // @spec AC-0136-01
    fn test_ac_0136_01_lance_append_scan() {
        let mut table = LanceTable::new("test");
        assert!(table.is_empty());
        table.append();
        table.append();
        assert_eq!(table.len(), 2);
        assert_eq!(table.name, "test");
    }

    /// Round-trip exacto de columnas INT/FLOAT.
    #[test]
    fn container_roundtrip() {
        let mut table = LanceTable::new("metrics");
        for index in 0..100 {
            table.push_int("ts", index * 1_000).expect("int");
            table
                .push_float("value", index as f64 * 0.5)
                .expect("float");
        }
        assert_eq!(table.len(), 100);
        let bytes = table.to_bytes();
        let restored = LanceTable::from_bytes(&bytes).expect("from_bytes");
        assert_eq!(restored, table);
        assert_eq!(
            restored.columns.get("ts"),
            Some(&Column::Int((0..100).map(|index| index * 1_000).collect()))
        );
        assert_eq!(
            restored.columns.get("value"),
            Some(&Column::Float(
                (0..100).map(|index| index as f64 * 0.5).collect()
            ))
        );
    }

    /// Tipos inconsistentes y contenedor corrupto fallan con mensaje.
    #[test]
    fn errors_are_actionable() {
        let mut table = LanceTable::new("t");
        table.push_int("a", 1).expect("int");
        assert!(table.push_float("a", 1.0).is_err());
        let bytes = table.to_bytes();
        assert!(LanceTable::from_bytes(b"garbage").is_err());
        assert!(LanceTable::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut wrong_version = bytes.clone();
        wrong_version[5] = 99;
        assert!(LanceTable::from_bytes(&wrong_version).is_err());
    }
}
