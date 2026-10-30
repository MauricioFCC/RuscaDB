//! Engine columnar opcional (SPEC-0110): almacenamiento por columnas en memoria.
//!
//! Sin dependencias externas. La integraciÃ³n con Arrow/Lance queda como trabajo
//! futuro (feature `columnar` activa este mÃ³dulo sin pulls adicionales).

#[cfg(all(feature = "alp", feature = "fastlanes"))]
use crate::alp_compression::AlpCompressor;
#[cfg(all(feature = "alp", feature = "fastlanes"))]
use crate::fastlanes::FastLanes;

/// Valor tipado de una celda columnar.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnValue {
    /// Nulo.
    Null,
    /// Booleano.
    Bool(bool),
    /// Entero.
    Int(i64),
    /// Flotante.
    Float(f64),
    /// Texto.
    Text(String),
}

impl ColumnValue {
    /// Convierte a texto.
    pub fn as_text(&self) -> String {
        match self {
            Self::Null => "NULL".to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Int(n) => n.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Text(t) => t.clone(),
        }
    }

    /// Intenta convertir a f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }
}

/// Tabla columnar en memoria: un `Vec` por columna.
#[derive(Debug, Clone)]
pub struct ColumnarTable {
    /// Nombres de las columnas en orden de inserciÃ³n.
    columns: Vec<String>,
    /// Datos por columna (cada entrada es una fila).
    data: Vec<Vec<ColumnValue>>,
}

impl ColumnarTable {
    /// Crea una tabla columnar vacÃ­a con las columnas dadas.
    pub fn new(columns: Vec<String>) -> Self {
        let width = columns.len();
        Self {
            columns,
            data: (0..width).map(|_| Vec::new()).collect(),
        }
    }

    /// NÃºmero de columnas.
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    /// NÃºmero de filas.
    pub fn len(&self) -> usize {
        self.data.first().map(|c| c.len()).unwrap_or(0)
    }

    /// Indica si la tabla estÃ¡ vacÃ­a.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// AÃ±ade una fila (debe tener tantos valores como columnas).
    pub fn append(&mut self, row: Vec<ColumnValue>) -> Result<(), String> {
        if row.len() != self.columns.len() {
            return Err(format!(
                "fila con {} valores para {} columnas",
                row.len(),
                self.columns.len()
            ));
        }
        for (index, value) in row.into_iter().enumerate() {
            self.data[index].push(value);
        }
        Ok(())
    }

    /// Escanea las filas en orden de inserciÃ³n.
    pub fn scan(&self) -> Vec<Vec<ColumnValue>> {
        let rows = self.len();
        let mut out = Vec::with_capacity(rows);
        for row_index in 0..rows {
            let mut row = Vec::with_capacity(self.columns.len());
            for column in &self.data {
                row.push(column[row_index].clone());
            }
            out.push(row);
        }
        out
    }

    /// Escanea proyectando solo las columnas indicadas.
    pub fn project(&self, column_names: &[&str]) -> Result<Vec<Vec<ColumnValue>>, String> {
        let indices: Vec<usize> = column_names
            .iter()
            .map(|name| {
                self.columns
                    .iter()
                    .position(|c| c == *name)
                    .ok_or_else(|| format!("columna '{name}' no existe"))
            })
            .collect::<Result<_, _>>()?;
        let rows = self.len();
        let mut out = Vec::with_capacity(rows);
        for row_index in 0..rows {
            let mut row = Vec::with_capacity(indices.len());
            for &idx in &indices {
                row.push(self.data[idx][row_index].clone());
            }
            out.push(row);
        }
        Ok(out)
    }

    /// Filtra filas por igualdad en una columna.
    pub fn filter_eq(
        &self,
        column: &str,
        value: &ColumnValue,
    ) -> Result<Vec<Vec<ColumnValue>>, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        let rows = self.len();
        let mut out = Vec::new();
        for row_index in 0..rows {
            if self.data[idx][row_index] == *value {
                let mut row = Vec::with_capacity(self.columns.len());
                for col in &self.data {
                    row.push(col[row_index].clone());
                }
                out.push(row);
            }
        }
        Ok(out)
    }

    /// Filtra filas por comparaciÃ³n (mayor que) en una columna numÃ©rica.
    pub fn filter_gt(&self, column: &str, threshold: f64) -> Result<Vec<Vec<ColumnValue>>, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        let rows = self.len();
        let mut out = Vec::new();
        for row_index in 0..rows {
            if let Some(val) = self.data[idx][row_index].as_f64()
                && val > threshold
            {
                let mut row = Vec::with_capacity(self.columns.len());
                for col in &self.data {
                    row.push(col[row_index].clone());
                }
                out.push(row);
            }
        }
        Ok(out)
    }

    /// Agrega una columna numÃ©rica: suma.
    pub fn sum(&self, column: &str) -> Result<f64, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        let mut total = 0.0;
        for val in &self.data[idx] {
            if let Some(v) = val.as_f64() {
                total += v;
            }
        }
        Ok(total)
    }

    /// Agrega una columna numÃ©rica: promedio.
    pub fn avg(&self, column: &str) -> Result<f64, String> {
        let s = self.sum(column)?;
        let count = self.len() as f64;
        if count == 0.0 {
            return Ok(0.0);
        }
        Ok(s / count)
    }

    /// Agrega una columna numÃ©rica: mÃ­nimo.
    pub fn min(&self, column: &str) -> Result<f64, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        self.data[idx]
            .iter()
            .filter_map(|v| v.as_f64())
            .fold(None, |acc, v| match acc {
                None => Some(v),
                Some(a) => Some(a.min(v)),
            })
            .ok_or_else(|| "sin valores numericos".to_string())
    }

    /// Agrega una columna numÃ©rica: mÃ¡ximo.
    pub fn max(&self, column: &str) -> Result<f64, String> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))?;
        self.data[idx]
            .iter()
            .filter_map(|v| v.as_f64())
            .fold(None, |acc, v| match acc {
                None => Some(v),
                Some(a) => Some(a.max(v)),
            })
            .ok_or_else(|| "sin valores numericos".to_string())
    }

    /// Nombres de las columnas.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Indice de `column` en el esquema.
    #[cfg(all(feature = "alp", feature = "fastlanes"))]
    fn column_index(&self, column: &str) -> Result<usize, String> {
        self.columns
            .iter()
            .position(|candidate| candidate == column)
            .ok_or_else(|| format!("columna '{column}' no existe"))
    }

    /// Comprime una columna `Int` con FOR + bit-packing (SPEC-0176).
    ///
    /// Args:
    ///     column: Nombre de la columna.
    ///
    /// Returns:
    ///     Bloque comprimido.
    ///
    /// Errors:
    ///     Texto accionable si la columna no existe, no es `Int` o tiene NULL.
    #[cfg(all(feature = "alp", feature = "fastlanes"))]
    pub fn encode_int_column(&self, column: &str) -> Result<Vec<u8>, String> {
        let index = self.column_index(column)?;
        let mut values = Vec::with_capacity(self.len());
        for row in self.scan() {
            match row.get(index) {
                Some(ColumnValue::Int(value)) => values.push(*value),
                Some(ColumnValue::Null) => {
                    return Err(format!("columna '{column}' contiene NULL"));
                }
                _ => return Err(format!("columna '{column}' no es INT")),
            }
        }
        Ok(ColumnChunk::encode_i64(&values))
    }

    /// Descomprime una columna `Int`.
    ///
    /// Args:
    ///     bytes: Bloque producido por [`Self::encode_int_column`].
    ///
    /// Returns:
    ///     Valores reconstruidos.
    ///
    /// Errors:
    ///     Texto accionable si el bloque esta vacio o corrupto.
    #[cfg(all(feature = "alp", feature = "fastlanes"))]
    pub fn decode_int_column(&self, bytes: &[u8]) -> Result<Vec<i64>, String> {
        let values = ColumnChunk::decode_i64(bytes);
        if values.is_empty() && !self.is_empty() {
            return Err("bloque de enteros vacio o corrupto".to_string());
        }
        Ok(values)
    }

    /// Comprime una columna `Float` con ALP (SPEC-0175).
    ///
    /// Args:
    ///     column: Nombre de la columna.
    ///
    /// Returns:
    ///     Bloque comprimido (exacto bit a bit).
    ///
    /// Errors:
    ///     Texto accionable si la columna no existe, no es `Float` o tiene NULL.
    #[cfg(all(feature = "alp", feature = "fastlanes"))]
    pub fn encode_float_column(&self, column: &str) -> Result<Vec<u8>, String> {
        let index = self.column_index(column)?;
        let mut values = Vec::with_capacity(self.len());
        for row in self.scan() {
            match row.get(index) {
                Some(ColumnValue::Float(value)) => values.push(*value),
                Some(ColumnValue::Null) => {
                    return Err(format!("columna '{column}' contiene NULL"));
                }
                _ => return Err(format!("columna '{column}' no es FLOAT")),
            }
        }
        Ok(ColumnChunk::encode_f64(&values))
    }

    /// Descomprime una columna `Float`.
    ///
    /// Args:
    ///     bytes: Bloque producido por [`Self::encode_float_column`].
    ///
    /// Returns:
    ///     Valores reconstruidos bit a bit.
    ///
    /// Errors:
    ///     Texto accionable si el bloque esta vacio o corrupto.
    #[cfg(all(feature = "alp", feature = "fastlanes"))]
    pub fn decode_float_column(&self, bytes: &[u8]) -> Result<Vec<f64>, String> {
        let values = ColumnChunk::decode_f64(bytes);
        if values.is_empty() && !self.is_empty() {
            return Err("bloque de flotantes vacio o corrupto".to_string());
        }
        Ok(values)
    }
}

/// Bloque columnar comprimido (SPEC-0175/0176).
///
/// Enteros: FOR (se resta el minimo) + bit-packing LSB-first de [`FastLanes`];
/// flotantes: [`AlpCompressor`]. Ambos con fallback crudo exacto.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg(all(feature = "alp", feature = "fastlanes"))]
pub struct ColumnChunk;

/// Magia del bloque columnar de enteros.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const MAGIC_I64: u8 = 0xCB;
/// Modo empaquetado (FOR + bit-packing).
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const MODE_PACKED: u8 = 0;
/// Modo crudo (i64 LE exacto).
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const MODE_RAW: u8 = 1;

#[cfg(all(feature = "alp", feature = "fastlanes"))]
impl ColumnChunk {
    /// Codifica una columna de enteros.
    ///
    /// Args:
    ///     values: Valores de la columna.
    ///
    /// Returns:
    ///     Bloque comprimido que [`Self::decode_i64`] reconstruye exacto.
    pub fn encode_i64(values: &[i64]) -> Vec<u8> {
        let count = values.len() as u32;
        let min = values.iter().copied().min().unwrap_or(0);
        let max = values.iter().copied().max().unwrap_or(0);
        let range = (max as i128) - (min as i128);
        if range > i128::from(u32::MAX) {
            let mut out = Vec::with_capacity(6 + values.len() * 8);
            out.push(MAGIC_I64);
            out.push(MODE_RAW);
            out.extend_from_slice(&count.to_le_bytes());
            for value in values {
                out.extend_from_slice(&value.to_le_bytes());
            }
            return out;
        }
        let deltas: Vec<u32> = values.iter().map(|value| (*value - min) as u32).collect();
        let max_delta = deltas.iter().copied().max().unwrap_or(0);
        let width = FastLanes::bit_width(max_delta);
        let packed = FastLanes::new().pack(&deltas, width);
        let mut out = Vec::with_capacity(15 + packed.len());
        out.push(MAGIC_I64);
        out.push(MODE_PACKED);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&min.to_le_bytes());
        out.push(width as u8);
        out.extend_from_slice(&packed);
        out
    }

    /// Decodifica una columna de enteros.
    ///
    /// Args:
    ///     bytes: Bloque producido por [`Self::encode_i64`].
    ///
    /// Returns:
    ///     Valores reconstruidos; vacio si el bloque es invalido o truncado.
    pub fn decode_i64(bytes: &[u8]) -> Vec<i64> {
        if bytes.len() < 6 || bytes[0] != MAGIC_I64 {
            return Vec::new();
        }
        let mode = bytes[1];
        let count = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
        match mode {
            MODE_RAW => {
                if bytes.len() < 6 + count * 8 {
                    return Vec::new();
                }
                bytes[6..6 + count * 8]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|chunk| i64::from_le_bytes(*chunk))
                    .collect()
            }
            MODE_PACKED => {
                if bytes.len() < 15 {
                    return Vec::new();
                }
                let min = i64::from_le_bytes([
                    bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12],
                    bytes[13],
                ]);
                let width = u32::from(bytes[14]).min(32);
                FastLanes::new()
                    .unpack(&bytes[15..], count, width)
                    .into_iter()
                    .map(|delta| min.wrapping_add(i64::from(delta)))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Codifica una columna de flotantes con ALP.
    ///
    /// Args:
    ///     values: Valores de la columna.
    ///
    /// Returns:
    ///     Bloque comprimido (exacto bit a bit).
    pub fn encode_f64(values: &[f64]) -> Vec<u8> {
        AlpCompressor::new().encode(values)
    }

    /// Decodifica una columna de flotantes ALP.
    ///
    /// Args:
    ///     bytes: Bloque producido por [`Self::encode_f64`].
    ///
    /// Returns:
    ///     Valores reconstruidos (vacio si el bloque es invalido).
    pub fn decode_f64(bytes: &[u8]) -> Vec<f64> {
        AlpCompressor::new().decode(bytes)
    }

    /// Ratio de compresion frente a `i64` crudo.
    ///
    /// Args:
    ///     values: Columna original.
    ///
    /// Returns:
    ///     `bytes_crudos / bytes_comprimidos`.
    pub fn ratio_i64(values: &[i64]) -> f32 {
        if values.is_empty() {
            return 1.0;
        }
        let raw = (values.len() * 8) as f32;
        raw / (Self::encode_i64(values).len() as f32).max(1.0)
    }
}

/// Magia del fichero columnar (`"RCOL1"`).
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const COLUMNAR_MAGIC: &[u8; 5] = b"RCOL1";
/// Version del formato columnar en disco.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const COLUMNAR_VERSION: u32 = 1;
/// Codificacion: bloque de enteros (FOR + bit-packing).
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const COLUMNAR_INT: u8 = 0;
/// Codificacion: bloque de flotantes (ALP).
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const COLUMNAR_FLOAT: u8 = 1;
/// Codificacion: valores etiquetados (mezcla, NULL, bool o texto).
#[cfg(all(feature = "alp", feature = "fastlanes"))]
const COLUMNAR_TAGGED: u8 = 2;

/// Estadisticas de una escritura columnar en disco.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnarFileStats {
    /// Filas escritas.
    pub rows: usize,
    /// Columnas escritas.
    pub columns: usize,
    /// Tamano crudo estimado (cada valor numerico 8 B, texto con prefijo).
    pub raw_bytes: usize,
    /// Tamano real del fichero codificado.
    pub encoded_bytes: usize,
}

#[cfg(all(feature = "alp", feature = "fastlanes"))]
impl ColumnarFileStats {
    /// Ratio de compresion `raw / encoded` (1.0 si no hay datos).
    ///
    /// Returns:
    ///     Factor de compresion (mayor es mejor).
    pub fn ratio(&self) -> f64 {
        if self.encoded_bytes == 0 {
            return 1.0;
        }
        self.raw_bytes as f64 / self.encoded_bytes as f64
    }
}

#[cfg(all(feature = "alp", feature = "fastlanes"))]
impl ColumnarTable {
    /// Serializa la tabla a un fichero columnar `RCOL1`.
    ///
    /// Cada columna se codifica con el codec mas especifico posible:
    /// enteros con FOR + bit-packing, flotantes con **ALP** y el resto con
    /// valores etiquetados. El orden de las columnas se preserva.
    ///
    /// Returns:
    ///     Bytes del contenedor.
    pub fn to_bytes(&self) -> Vec<u8> {
        let rows = self.len() as u32;
        let mut out = Vec::new();
        out.extend_from_slice(COLUMNAR_MAGIC);
        out.extend_from_slice(&COLUMNAR_VERSION.to_le_bytes());
        out.extend_from_slice(&rows.to_le_bytes());
        out.extend_from_slice(&(self.columns.len() as u32).to_le_bytes());
        for (name, values) in self.columns.iter().zip(self.data.iter()) {
            let name_bytes = name.as_bytes();
            out.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(name_bytes);
            let (encoding, payload) = encode_column(values);
            out.push(encoding);
            out.extend_from_slice(&(values.len() as u32).to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&payload);
        }
        out
    }

    /// Reconstruye una tabla desde un contenedor `RCOL1`.
    ///
    /// Args:
    ///     bytes: Bytes producidos por [`Self::to_bytes`].
    ///
    /// Returns:
    ///     Tabla reconstruida (valores exactos bit a bit).
    ///
    /// Errors:
    ///     Texto accionable si la magia, la version, una cota o el padding no
    ///     son validos.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 17 || &bytes[0..5] != COLUMNAR_MAGIC {
            return Err("columnar: magia invalida o fichero truncado".to_string());
        }
        let mut cursor = 5usize;
        let version = read_u32(bytes, &mut cursor)?;
        if version != COLUMNAR_VERSION {
            return Err(format!("columnar: version {version} no soportada"));
        }
        let rows = read_u32(bytes, &mut cursor)? as usize;
        let column_count = read_u32(bytes, &mut cursor)? as usize;
        if column_count > 65_536 {
            return Err(format!("columnar: {column_count} columnas excede la cota"));
        }
        let mut columns = Vec::with_capacity(column_count.min(1_024));
        let mut data = Vec::with_capacity(column_count.min(1_024));
        for _ in 0..column_count {
            let name_len = read_u32(bytes, &mut cursor)? as usize;
            let name = read_text(bytes, &mut cursor, name_len)?;
            if columns.iter().any(|existing| existing == &name) {
                return Err(format!("columnar: columna duplicada '{name}'"));
            }
            let encoding = *bytes
                .get(cursor)
                .ok_or_else(|| "columnar: codificacion truncada".to_string())?;
            cursor += 1;
            let count = read_u32(bytes, &mut cursor)? as usize;
            if count != rows {
                return Err(format!(
                    "columnar: columna '{name}' con {count} valores y {rows} filas"
                ));
            }
            let payload_len = read_u32(bytes, &mut cursor)? as usize;
            let payload = read_slice(bytes, &mut cursor, payload_len)?;
            let values = decode_column(encoding, payload, count)?;
            columns.push(name);
            data.push(values);
        }
        if cursor != bytes.len() {
            return Err("columnar: bytes sobrantes tras la ultima columna".to_string());
        }
        Ok(Self { columns, data })
    }

    /// Escribe la tabla en `path` de forma atomica (`tmp` + rename).
    ///
    /// Args:
    ///     path: Ruta destino.
    ///
    /// Returns:
    ///     Estadisticas de la escritura.
    ///
    /// Errors:
    ///     Texto accionable si falla la escritura o el rename.
    pub fn write_to(&self, path: impl AsRef<std::path::Path>) -> Result<ColumnarFileStats, String> {
        let path = path.as_ref();
        let bytes = self.to_bytes();
        let tmp = path.with_extension(format!(
            "{}.tmp",
            path.extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("rcol")
        ));
        std::fs::write(&tmp, &bytes)
            .map_err(|error| format!("columnar: no se pudo escribir {}: {error}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|error| {
            format!(
                "columnar: no se pudo renombrar {} -> {}: {error}",
                tmp.display(),
                path.display()
            )
        })?;
        Ok(ColumnarFileStats {
            rows: self.len(),
            columns: self.columns.len(),
            raw_bytes: self.raw_size(),
            encoded_bytes: bytes.len(),
        })
    }

    /// Lee una tabla desde un fichero `RCOL1`.
    ///
    /// Args:
    ///     path: Ruta del fichero.
    ///
    /// Returns:
    ///     Tabla reconstruida.
    ///
    /// Errors:
    ///     Texto accionable si falla la lectura o el contenido es invalido.
    pub fn read_from(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|error| format!("columnar: no se pudo leer {}: {error}", path.display()))?;
        Self::from_bytes(&bytes)
    }

    /// Tamano crudo estimado de la tabla en memoria.
    ///
    /// Se usa para reportar el ratio de compresion: cada valor numerico cuenta
    /// 8 B, cada texto `4 + len` y cada NULL 1 B.
    ///
    /// Returns:
    ///     Bytes crudos estimados.
    pub fn raw_size(&self) -> usize {
        self.data
            .iter()
            .flat_map(|column| column.iter())
            .map(|value| match value {
                ColumnValue::Text(text) => 4 + text.len(),
                ColumnValue::Null => 1,
                _ => 8,
            })
            .sum()
    }
}

/// Codifica una columna eligiendo el codec mas especifico.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
fn encode_column(values: &[ColumnValue]) -> (u8, Vec<u8>) {
    if values
        .iter()
        .all(|value| matches!(value, ColumnValue::Int(_)))
    {
        let ints: Vec<i64> = values
            .iter()
            .map(|value| match value {
                ColumnValue::Int(number) => *number,
                _ => 0,
            })
            .collect();
        return (COLUMNAR_INT, ColumnChunk::encode_i64(&ints));
    }
    if values
        .iter()
        .all(|value| matches!(value, ColumnValue::Float(_)))
    {
        let floats: Vec<f64> = values
            .iter()
            .map(|value| match value {
                ColumnValue::Float(number) => *number,
                _ => 0.0,
            })
            .collect();
        return (COLUMNAR_FLOAT, ColumnChunk::encode_f64(&floats));
    }
    let mut payload = Vec::new();
    for value in values {
        match value {
            ColumnValue::Null => payload.push(0),
            ColumnValue::Bool(flag) => {
                payload.push(1);
                payload.push(u8::from(*flag));
            }
            ColumnValue::Int(number) => {
                payload.push(2);
                payload.extend_from_slice(&number.to_le_bytes());
            }
            ColumnValue::Float(number) => {
                payload.push(3);
                payload.extend_from_slice(&number.to_le_bytes());
            }
            ColumnValue::Text(text) => {
                payload.push(4);
                payload.extend_from_slice(&(text.len() as u32).to_le_bytes());
                payload.extend_from_slice(text.as_bytes());
            }
        }
    }
    (COLUMNAR_TAGGED, payload)
}

/// Decodifica una columna segun su codificacion.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
fn decode_column(encoding: u8, payload: &[u8], count: usize) -> Result<Vec<ColumnValue>, String> {
    match encoding {
        COLUMNAR_INT => {
            let values = ColumnChunk::decode_i64(payload);
            if values.len() != count {
                return Err("columnar: bloque de enteros corrupto".to_string());
            }
            Ok(values.into_iter().map(ColumnValue::Int).collect())
        }
        COLUMNAR_FLOAT => {
            let values = ColumnChunk::decode_f64(payload);
            if values.len() != count {
                return Err("columnar: bloque de flotantes corrupto".to_string());
            }
            Ok(values.into_iter().map(ColumnValue::Float).collect())
        }
        COLUMNAR_TAGGED => decode_tagged(payload, count),
        other => Err(format!("columnar: codificacion {other} invalida")),
    }
}

/// Decodifica una columna con valores etiquetados.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
fn decode_tagged(payload: &[u8], count: usize) -> Result<Vec<ColumnValue>, String> {
    let mut cursor = 0usize;
    let mut values = Vec::with_capacity(count.min(1_024));
    for index in 0..count {
        let tag = *payload
            .get(cursor)
            .ok_or_else(|| format!("columnar: valor {index} truncado"))?;
        cursor += 1;
        let value = match tag {
            0 => ColumnValue::Null,
            1 => {
                let flag = *payload
                    .get(cursor)
                    .ok_or_else(|| format!("columnar: bool {index} truncado"))?;
                cursor += 1;
                match flag {
                    0 => ColumnValue::Bool(false),
                    1 => ColumnValue::Bool(true),
                    other => return Err(format!("columnar: bool {other} invalido")),
                }
            }
            2 => {
                let raw = read_slice(payload, &mut cursor, 8)?;
                ColumnValue::Int(i64::from_le_bytes(
                    raw.try_into()
                        .map_err(|_| "columnar: i64 truncado".to_string())?,
                ))
            }
            3 => {
                let raw = read_slice(payload, &mut cursor, 8)?;
                ColumnValue::Float(f64::from_le_bytes(
                    raw.try_into()
                        .map_err(|_| "columnar: f64 truncado".to_string())?,
                ))
            }
            4 => {
                let len = read_u32(payload, &mut cursor)? as usize;
                ColumnValue::Text(read_text(payload, &mut cursor, len)?)
            }
            other => return Err(format!("columnar: tag {other} invalido")),
        };
        values.push(value);
    }
    if cursor != payload.len() {
        return Err("columnar: bytes sobrantes en columna etiquetada".to_string());
    }
    Ok(values)
}

/// Lee un `u32` LE avanzando el cursor.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let raw = read_slice(bytes, cursor, 4)?;
    Ok(u32::from_le_bytes(
        raw.try_into()
            .map_err(|_| "columnar: u32 truncado".to_string())?,
    ))
}

/// Lee `len` bytes avanzando el cursor.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
fn read_slice<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8], String> {
    let end = cursor.saturating_add(len);
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| "columnar: fichero truncado".to_string())?;
    *cursor = end;
    Ok(slice)
}

/// Lee `len` bytes como UTF-8.
#[cfg(all(feature = "alp", feature = "fastlanes"))]
fn read_text(bytes: &[u8], cursor: &mut usize, len: usize) -> Result<String, String> {
    let raw = read_slice(bytes, cursor, len)?;
    String::from_utf8(raw.to_vec()).map_err(|_| "columnar: UTF-8 invalido".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0110-01
    fn test_ac_0110_01_columnar_append_scan() {
        let mut table = ColumnarTable::new(vec!["a".to_string(), "b".to_string()]);
        table
            .append(vec![
                ColumnValue::Int(1),
                ColumnValue::Text("x".to_string()),
            ])
            .expect("append");
        table
            .append(vec![
                ColumnValue::Int(2),
                ColumnValue::Text("y".to_string()),
            ])
            .expect("append");
        assert_eq!(table.len(), 2);
        assert_eq!(table.scan().len(), 2);
        let projected = table.project(&["a"]).expect("project");
        assert_eq!(
            projected,
            vec![vec![ColumnValue::Int(1)], vec![ColumnValue::Int(2)]]
        );
        let filtered = table.filter_eq("a", &ColumnValue::Int(1)).expect("filter");
        assert_eq!(filtered.len(), 1);
        let filtered_gt = table.filter_gt("a", 1.0).expect("filter_gt");
        assert_eq!(filtered_gt.len(), 1);
        assert!((table.sum("a").expect("sum") - 3.0).abs() < 1e-9);
        assert!((table.avg("a").expect("avg") - 1.5).abs() < 1e-9);
        assert!((table.min("a").expect("min") - 1.0).abs() < 1e-9);
        assert!((table.max("a").expect("max") - 2.0).abs() < 1e-9);
    }
}

#[cfg(all(test, feature = "alp", feature = "fastlanes"))]
mod chunk_tests {
    use super::*;

    /// Round-trip exacto de una columna de enteros (FOR + bit-packing).
    #[test]
    fn int_column_roundtrip() {
        let values: Vec<i64> = (0..500).map(|index| 1_000 + index * 7).collect();
        let bytes = ColumnChunk::encode_i64(&values);
        assert_eq!(ColumnChunk::decode_i64(&bytes), values);
        assert!(ColumnChunk::ratio_i64(&values) > 3.0);
    }

    /// Round-trip bit a bit de una columna de flotantes (ALP).
    #[test]
    fn float_column_roundtrip() {
        let values = vec![1.25f64, -4.5, 0.0, -0.0, 1e-9, f64::NAN];
        let bytes = ColumnChunk::encode_f64(&values);
        assert_eq!(
            ColumnChunk::decode_f64(&bytes)
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            values
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }

    /// `ColumnarTable` comprime y restaura sus columnas tipadas.
    #[test]
    fn table_columns_compress() {
        let mut table = ColumnarTable::new(vec!["a".to_string(), "b".to_string()]);
        for index in 0..100 {
            table
                .append(vec![
                    ColumnValue::Int(index * 2),
                    ColumnValue::Float(index as f64 * 0.25),
                ])
                .expect("append");
        }
        let ints = table.encode_int_column("a").expect("encode int");
        assert_eq!(
            table.decode_int_column(&ints).expect("decode int").len(),
            100
        );
        let floats = table.encode_float_column("b").expect("encode float");
        let restored = table.decode_float_column(&floats).expect("decode float");
        assert_eq!(restored.len(), 100);
        assert!((restored[12] - 3.0).abs() < 1e-9);

        // Errores accionables.
        assert!(table.encode_int_column("zzz").is_err());
        assert!(table.encode_float_column("a").is_err());
    }

    /// Construye una tabla mixta para las pruebas de fichero.
    #[allow(clippy::expect_used)]
    fn sample_table() -> ColumnarTable {
        let mut table = ColumnarTable::new(vec![
            "id".to_string(),
            "score".to_string(),
            "label".to_string(),
            "flag".to_string(),
            "maybe".to_string(),
        ]);
        for index in 0..200i64 {
            let maybe = if index % 7 == 0 {
                ColumnValue::Null
            } else {
                ColumnValue::Int(index * 3)
            };
            table
                .append(vec![
                    ColumnValue::Int(index),
                    ColumnValue::Float(index as f64 * 0.25),
                    ColumnValue::Text(format!("fila-{index}")),
                    ColumnValue::Bool(index % 2 == 0),
                    maybe,
                ])
                .expect("append");
        }
        table
    }

    /// `RCOL1`: round-trip exacto (ALP en flotantes, codecs especificos).
    #[test]
    fn columnar_file_roundtrip() {
        let table = sample_table();
        let bytes = table.to_bytes();
        let restored = ColumnarTable::from_bytes(&bytes).expect("from_bytes");
        assert_eq!(restored.columns(), table.columns());
        assert_eq!(restored.scan(), table.scan());
        assert_eq!(restored.len(), 200);
    }

    /// El fichero aplica ALP/FOR y reduce el tamano crudo.
    #[test]
    fn columnar_file_compresses_floats() {
        let table = sample_table();
        let stats = table
            .write_to(
                std::env::temp_dir().join(format!("ruscadb-columnar-{}.rcol", std::process::id())),
            )
            .expect("write_to");
        assert_eq!(stats.rows, 200);
        assert_eq!(stats.columns, 5);
        assert!(
            stats.encoded_bytes < stats.raw_bytes,
            "encoded {} raw {}",
            stats.encoded_bytes,
            stats.raw_bytes
        );
        assert!(stats.ratio() > 1.2, "ratio {}", stats.ratio());

        let path =
            std::env::temp_dir().join(format!("ruscadb-columnar-{}.rcol", std::process::id()));
        let restored = ColumnarTable::read_from(&path).expect("read_from");
        assert_eq!(restored.scan(), table.scan());
        let _ = std::fs::remove_file(path);
    }

    /// Magia/version/truncamiento/bytes sobrantes se rechazan sin panics.
    #[test]
    fn columnar_file_rejects_corruption() {
        let bytes = sample_table().to_bytes();
        assert!(ColumnarTable::from_bytes(b"RCOLX").is_err());

        let mut wrong_version = bytes.clone();
        wrong_version[5] = 99;
        assert!(ColumnarTable::from_bytes(&wrong_version).is_err());

        let truncated = &bytes[..bytes.len() - 1];
        assert!(ColumnarTable::from_bytes(truncated).is_err());

        let mut extra = bytes.clone();
        extra.push(0);
        assert!(ColumnarTable::from_bytes(&extra).is_err());

        // row_count incoherente con la columna.
        let mut wrong_rows = bytes.clone();
        wrong_rows[9] = 1;
        assert!(ColumnarTable::from_bytes(&wrong_rows).is_err());
    }

    /// La tabla vacia conserva columnas y tamaño.
    #[test]
    fn columnar_file_empty_roundtrip() {
        let table = ColumnarTable::new(vec!["a".to_string(), "b".to_string()]);
        let restored = ColumnarTable::from_bytes(&table.to_bytes()).expect("vacia");
        assert_eq!(restored.columns(), table.columns());
        assert!(restored.is_empty());
    }
}
