//! Modelo de datos unificado: el [`Record`] y sus componentes.
//!
//! Un único tipo físico soporta los cinco modelos (relacional, documento,
//! grafo, vector, time-series) y multimodal (blob + embedding). Cada modelo es
//! una proyección sobre los mismos campos. Ver `specs/core_record.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::RuscaError;
use crate::id::RecordId;

/// Mapa ordenado de escalares tipados (columnas relacionales, timestamps).
pub type ScalarMap = BTreeMap<String, ScalarValue>;

/// Valor escalar tipado de una columna.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ScalarValue {
    /// Ausencia de valor.
    Null,
    /// Booleano.
    Bool(bool),
    /// Entero con signo de 64 bits.
    Int(i64),
    /// Entero sin signo de 64 bits.
    UInt(u64),
    /// Flotante de 64 bits.
    Float(f64),
    /// Texto UTF-8.
    Text(String),
    /// Bytes crudos.
    Bytes(Vec<u8>),
    /// Marca de tiempo en milisegundos desde epoch Unix.
    TimestampMillis(i64),
}

/// Arista tipada del modelo de grafo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Etiqueta de la relación (p. ej. `tiene_dueno`).
    pub label: String,
    /// Nodo destino (o origen, según la dirección).
    pub node: RecordId,
}

/// Conjunto de aristas entrantes y salientes de un nodo.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeSet {
    /// Aristas salientes del nodo.
    pub out: Vec<Edge>,
    /// Aristas entrantes al nodo.
    pub incoming: Vec<Edge>,
}

/// Métrica de distancia de un espacio vectorial.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Metric {
    /// Distancia euclídea (L2).
    L2,
    /// Similitud coseno.
    Cosine,
    /// Producto interno.
    InnerProduct,
}

/// Metadata de procedencia de un embedding (ADR-009 / SI-2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingMeta {
    /// Identificador del modelo que generó el vector (p. ej. `all-MiniLM-L6-v2`).
    pub model_id: String,
    /// Dimensión del espacio vectorial.
    pub dim: usize,
    /// Métrica de distancia asociada.
    pub metric: Metric,
}

/// Embedding con su metadata de modelo obligatoria.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Embedding {
    /// Valores del vector (`float32[d]`).
    pub values: Vec<f32>,
    /// Metadata de procedencia (modelo, dimensión, métrica).
    pub meta: EmbeddingMeta,
}

impl Embedding {
    /// Crea un embedding validando que la dimensión coincida con el vector.
    ///
    /// Args:
    ///     values: Valores `float32` del vector.
    ///     meta: Metadata del modelo; su `dim` debe igualar `values.len()`.
    ///
    /// Returns:
    ///     El embedding, o [`RuscaError::DimensionMismatch`] si no coinciden.
    pub fn new(values: Vec<f32>, meta: EmbeddingMeta) -> Result<Self, RuscaError> {
        if values.len() != meta.dim {
            return Err(RuscaError::DimensionMismatch {
                expected: meta.dim,
                actual: values.len(),
            });
        }
        Ok(Self { values, meta })
    }
}

/// Puntero a un blob multimodal almacenado fuera de la página (ADR-010).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobPointer {
    /// URI o ruta relativa dentro del blob store.
    pub uri: String,
    /// Offset inicial en bytes dentro del blob.
    pub offset: u64,
    /// Longitud en bytes.
    pub len: u64,
    /// Tipo MIME (p. ej. `image/jpeg`).
    pub media_type: String,
}

/// Metadata de versión MVCC y procedencia de un registro.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordMeta {
    /// Transacción que creó esta versión.
    pub created_tx: u64,
    /// Transacción que borró esta versión (`None` si vive).
    pub deleted_tx: Option<u64>,
    /// LSN del WAL asociado.
    pub lsn: u64,
    /// Versión del modelo de embedding activa (ADR-009).
    pub embedding_version: Option<u32>,
}

/// (De)serializa `doc` como texto JSON para formatos binarios que no admiten
/// valores auto-descriptivos (p. ej. `postcard`, usado por el heap): el valor se
/// guarda como `String` y se parsea al leer. `None` se codifica igual que antes,
/// así que los registros sin documento conservan el formato.
mod doc_json_string {
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    /// Serializa `Option<Value>` como `Option<String>` (JSON textual).
    pub fn serialize<S: Serializer>(
        value: &Option<Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(document) => serializer.serialize_some(&document.to_string()),
            None => serializer.serialize_none(),
        }
    }

    /// Deserializa `Option<String>` (JSON textual) a `Option<Value>`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Value>, D::Error> {
        match Option::<String>::deserialize(deserializer)? {
            Some(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

/// Registro universal de RuscaDB: un solo tipo físico para los cinco modelos.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Identidad estable (ULID ordenable por tiempo).
    pub id: RecordId,
    /// Escalares tipados: PK, columnas relacionales, timestamp (time-series).
    pub scalars: ScalarMap,
    /// Documento JSON anidado y libre (modelo documental).
    #[serde(with = "doc_json_string")]
    pub doc: Option<serde_json::Value>,
    /// Aristas entrantes/salientes (modelo de grafo).
    pub edges: EdgeSet,
    /// Embedding con metadata (modelo vectorial).
    pub vector: Option<Embedding>,
    /// Puntero a bytes multimodales (modelo multimodal).
    pub blob: Option<BlobPointer>,
    /// Versión MVCC + procedencia.
    pub meta: RecordMeta,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Construye un registro con los seis campos poblados.
    fn sample_record() -> Record {
        let meta = EmbeddingMeta {
            model_id: "all-MiniLM-L6-v2".to_string(),
            dim: 3,
            metric: Metric::Cosine,
        };
        Record {
            id: RecordId::new(),
            scalars: ScalarMap::from([
                ("title".to_string(), ScalarValue::Text("gato".to_string())),
                ("score".to_string(), ScalarValue::Float(0.5)),
                (
                    "ts".to_string(),
                    ScalarValue::TimestampMillis(1_700_000_000_000),
                ),
            ]),
            doc: Some(json!({ "tags": ["gato", "mascota"], "nested": { "n": 1 } })),
            edges: EdgeSet {
                out: vec![Edge {
                    label: "tiene_dueno".to_string(),
                    node: RecordId::new(),
                }],
                incoming: vec![],
            },
            vector: Some(Embedding::new(vec![0.1, 0.2, 0.3], meta).expect("embedding válido")),
            blob: Some(BlobPointer {
                uri: "ab/cd/deadbeef.jpg".to_string(),
                offset: 0,
                len: 1024,
                media_type: "image/jpeg".to_string(),
            }),
            meta: RecordMeta {
                created_tx: 7,
                deleted_tx: None,
                lsn: 42,
                embedding_version: Some(1),
            },
        }
    }

    /// AC-0001-01 — el roundtrip de serialización es exacto.
    #[test]
    // @spec AC-0001-01
    fn test_ac_0001_01_record_roundtrip_is_lossless() {
        let original = sample_record();
        let encoded = serde_json::to_string(&original).expect("serializa");
        let decoded: Record = serde_json::from_str(&encoded).expect("deserializa");
        assert_eq!(original, decoded);
    }

    /// AC-0001-02 — el ULID más antiguo ordena primero (monotonía temporal).
    #[test]
    // @spec AC-0001-02
    fn test_ac_0001_02_ulid_ordering_is_monotonic() {
        let older = RecordId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let newer = RecordId::new();
        assert!(older < newer, "el ULID antiguo debe ordenar primero");
        assert!(older.to_string() < newer.to_string());
    }

    /// AC-0001-03 — la metadata del embedding es obligatoria y consistente.
    #[test]
    // @spec AC-0001-03
    fn test_ac_0001_03_embedding_metadata_is_mandatory() {
        let good = EmbeddingMeta {
            model_id: "all-MiniLM-L6-v2".to_string(),
            dim: 3,
            metric: Metric::Cosine,
        };
        assert!(Embedding::new(vec![0.1, 0.2, 0.3], good).is_ok());

        let bad = EmbeddingMeta {
            model_id: "m".to_string(),
            dim: 2,
            metric: Metric::L2,
        };
        let result = Embedding::new(vec![0.1, 0.2, 0.3], bad);
        assert!(matches!(
            result,
            Err(RuscaError::DimensionMismatch {
                expected: 2,
                actual: 3
            })
        ));
    }
}
