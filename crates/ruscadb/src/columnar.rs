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
}
