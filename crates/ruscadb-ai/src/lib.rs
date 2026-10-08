//! # ruscadb-ai
//!
//! Inferencia local de embeddings de RuscaDB: **Candle** por defecto (puro
//! Rust) con backend `onnx` opcional. Modelos: CLIP (imagen/texto), Whisper
//! (audio→texto), e5/MiniLM (texto).
//!
//! Implementa el puerto `EmbedPort` de `ruscadb-core`.
//! Diseño: ADR-005 y `docs/RuscaDB-roadmap.md` §6. Fase: F4.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ruscadb_core::{Embedding, Metric, RuscaError};

/// Bases y primo de FNV-1a de 64 bits (hash rápido y estable).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// Multiplicador primo de FNV-1a de 64 bits.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Máscara del bit más significativo, usado como signo del hashing trick.
const SIGN_BIT: u64 = 1 << 63;

/// Puerto de embeddings: convierte texto en un vector `float32` de dimensión fija.
pub trait Embedder {
    /// Dimensión del espacio vectorial que produce el embedder.
    ///
    /// Returns:
    ///     Número de componentes de cada vector generado.
    fn dim(&self) -> usize;

    /// Proyecta `text` en un vector `float32[dim]` L2-normalizado.
    ///
    /// Args:
    ///     text: Texto de entrada; se tokeniza por espacios en blanco.
    ///
    /// Returns:
    ///     Un vector de longitud [`Embedder::dim`]; si no hay tokens, todo ceros.
    fn embed(&self, text: &str) -> Vec<f32>;
}

/// Embedder local determinista basado en el *hashing trick* con proyección
/// firmada (feature hashing).
///
/// No usa GPU ni dependencias pesadas: tokeniza por espacios en blanco,
/// convierte cada token a minúsculas, lo hashea con FNV-1a de 64 bits y
/// acumula `±1` en `hash % dim`. El resultado se normaliza en L2.
/// Mismo texto ⇒ mismo vector (reproducible).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashingEmbedder {
    /// Dimensión del espacio vectorial.
    dim: usize,
}

impl HashingEmbedder {
    /// Crea un embedder de dimensión `dim`.
    ///
    /// Args:
    ///     dim: Dimensión del vector de salida; debe ser `>= 1`.
    ///
    /// Returns:
    ///     El embedder configurado.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si `dim == 0`.
    pub fn new(dim: usize) -> Result<Self, RuscaError> {
        if dim == 0 {
            return Err(RuscaError::InvalidConfig(
                "la dimensión del embedder debe ser >= 1".to_string(),
            ));
        }
        Ok(Self { dim })
    }
}

impl Embedder for HashingEmbedder {
    /// Devuelve la dimensión configurada.
    fn dim(&self) -> usize {
        self.dim
    }

    /// Embebe `text` con el hashing trick firmado y normaliza en L2.
    ///
    /// Args:
    ///     text: Texto de entrada.
    ///
    /// Returns:
    ///     Vector `float32` de longitud `dim`.
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0_f32; self.dim];
        for token in text.split_whitespace() {
            let folded = token.to_lowercase();
            let hash = fnv1a64(folded.as_bytes());
            let index = (hash % self.dim as u64) as usize;
            let sign = if hash & SIGN_BIT == 0 { 1.0 } else { -1.0 };
            vector[index] += sign;
        }
        normalize_l2(&mut vector);
        vector
    }
}

/// Hash FNV-1a de 64 bits sobre `bytes`.
///
/// Args:
///     bytes: Bytes a hashear.
///
/// Returns:
///     El digest de 64 bits.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Normaliza `vector` en L2 in-place; si la norma es cero no hace nada.
///
/// Args:
///     vector: Vector a normalizar.
fn normalize_l2(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

/// Especificación allowlisted de un modelo de embedding (SPEC-0032).
///
/// Fija la firma contractual con que un vector puede entrar al motor:
/// dimensión, métrica y versión del modelo (ADR-009).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    /// Dimensión esperada del vector (`float32[dim]`).
    pub dim: usize,
    /// Métrica de distancia esperada.
    pub metric: Metric,
    /// Versión del modelo; se copia a `RecordMeta.embedding_version`.
    pub version: u32,
}

/// Registro de modelos de embedding allowlisted (SPEC-0032).
///
/// Invariante **SI-2**: todo vector debe declarar `model_id`, `dim` y `metric`
/// ([`EmbeddingMeta`](ruscadb_core::EmbeddingMeta)); este registro valida esa
/// metadata cuando la allowlist está activa.
///
/// Registro **en memoria** (no persistido; la persistencia queda fuera de
/// alcance de SPEC-0032): se pierde al reabrir la base, igual que el resto de
/// estado no durable de la fachada.
///
/// Registro vacío ⇒ [`ModelRegistry::validate`] acepta todo (compatibilidad
/// con el comportamiento previo, NF-0032-01).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelRegistry {
    /// Modelos permitidos indexados por `model_id` (orden estable).
    models: BTreeMap<String, ModelSpec>,
}

impl ModelRegistry {
    /// Crea un registro vacío (sin allowlist activa).
    ///
    /// Returns:
    ///     Registro sin modelos; `validate` aceptará cualquier embedding.
    pub fn new() -> Self {
        Self {
            models: BTreeMap::new(),
        }
    }

    /// Registra o actualiza un modelo permitido.
    ///
    /// Args:
    ///     model_id: Identificador del modelo (no vacío).
    ///     dim: Dimensión esperada del vector (`>= 1`).
    ///     metric: Métrica de distancia esperada.
    ///     version: Versión del modelo (se copia a `embedding_version`).
    ///
    /// Returns:
    ///     `Ok(())` tras insertar o reemplazar la entrada.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si `model_id` está vacío o `dim == 0`.
    pub fn register(
        &mut self,
        model_id: &str,
        dim: usize,
        metric: Metric,
        version: u32,
    ) -> Result<(), RuscaError> {
        if model_id.is_empty() {
            return Err(RuscaError::InvalidConfig(
                "el model_id no puede estar vacío al registrar un modelo".to_string(),
            ));
        }
        if dim == 0 {
            return Err(RuscaError::InvalidConfig(format!(
                "la dimensión del modelo '{model_id}' debe ser >= 1"
            )));
        }
        self.models.insert(
            model_id.to_string(),
            ModelSpec {
                dim,
                metric,
                version,
            },
        );
        Ok(())
    }

    /// Indica si no hay ningún modelo registrado (allowlist inactiva).
    ///
    /// Returns:
    ///     `true` si el registro está vacío.
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Valida la metadata de un embedding contra la allowlist.
    ///
    /// Args:
    ///     embedding: Embedding cuya `meta` se contrasta con el registro.
    ///
    /// Returns:
    ///     `Ok(())` si el registro está vacío o si `model_id`, `dim` y `metric`
    ///     coinciden con la especificación registrada.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el `model_id` no está registrado o
    ///     su `dim`/`metric` no coincide (mensaje accionable WHAT+WHERE).
    pub fn validate(&self, embedding: &Embedding) -> Result<(), RuscaError> {
        if self.models.is_empty() {
            return Ok(());
        }
        let model_id = embedding.meta.model_id.as_str();
        let spec = self.models.get(model_id).ok_or_else(|| {
            RuscaError::InvalidConfig(format!(
                "modelo de embedding '{model_id}' no allowlisted: regístralo con \
                 Database::register_model antes de insertar vectores"
            ))
        })?;
        if spec.dim != embedding.meta.dim {
            return Err(RuscaError::InvalidConfig(format!(
                "dimensión incompatible para el modelo '{model_id}': registrada {}, recibida {}",
                spec.dim, embedding.meta.dim
            )));
        }
        if spec.metric != embedding.meta.metric {
            return Err(RuscaError::InvalidConfig(format!(
                "métrica incompatible para el modelo '{model_id}': registrada {:?}, recibida {:?}",
                spec.metric, embedding.meta.metric
            )));
        }
        Ok(())
    }

    /// Versión registrada de un modelo, si existe.
    ///
    /// Args:
    ///     model_id: Identificador del modelo consultado.
    ///
    /// Returns:
    ///     La versión registrada o `None` si el modelo no está allowlisted.
    pub fn version_of(&self, model_id: &str) -> Option<u32> {
        self.models.get(model_id).map(|spec| spec.version)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use ruscadb_core::EmbeddingMeta;

    /// Norma L2 de un vector.
    fn l2_norm(vector: &[f32]) -> f32 {
        vector.iter().map(|value| value * value).sum::<f32>().sqrt()
    }

    /// Construye un embedding válido (`dim == values.len()`) para el registro.
    ///
    /// Args:
    ///     model_id: Identificador del modelo.
    ///     values: Valores del vector.
    ///     metric: Métrica declarada.
    ///
    /// Returns:
    ///     El embedding consistente.
    fn sample_embedding(model_id: &str, values: Vec<f32>, metric: Metric) -> Embedding {
        let meta = EmbeddingMeta {
            model_id: model_id.to_string(),
            dim: values.len(),
            metric,
        };
        Embedding::new(values, meta).expect("embedding válido")
    }

    /// AC-0032-01 — un registro vacío acepta cualquier embedding.
    #[test] // @spec AC-0032-01
    fn test_ac_0032_01_empty_registry_accepts_all() {
        let registry = ModelRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.version_of("cualquier-modelo"), None);
        let any = sample_embedding("cualquier-modelo", vec![0.1, 0.2, 0.3], Metric::Cosine);
        assert!(
            registry.validate(&any).is_ok(),
            "registro vacío debe aceptar todo (NF-0032-01)"
        );
    }

    /// AC-0032-02 — un embedding que coincide con el modelo registrado se acepta.
    #[test] // @spec AC-0032-02
    fn test_ac_0032_02_registered_model_is_accepted() {
        let mut registry = ModelRegistry::new();
        registry
            .register("all-MiniLM-L6-v2", 3, Metric::Cosine, 4)
            .expect("register");
        assert!(!registry.is_empty());
        assert_eq!(registry.version_of("all-MiniLM-L6-v2"), Some(4));
        let matching = sample_embedding("all-MiniLM-L6-v2", vec![0.1, 0.2, 0.3], Metric::Cosine);
        assert!(registry.validate(&matching).is_ok());
    }

    /// AC-0032-03 — un `model_id` no registrado se rechaza con error accionable.
    #[test] // @spec AC-0032-03
    fn test_ac_0032_03_unregistered_model_is_rejected() {
        let mut registry = ModelRegistry::new();
        registry
            .register("allowlisted", 3, Metric::L2, 1)
            .expect("register");
        let unknown = sample_embedding("desconocido", vec![0.1, 0.2, 0.3], Metric::L2);
        let error = registry
            .validate(&unknown)
            .expect_err("modelo no allowlisted debe rechazarse");
        assert!(matches!(error, RuscaError::InvalidConfig(_)));
        assert!(
            error.to_string().contains("desconocido"),
            "el error debe citar el modelo: {error}"
        );
    }

    /// AC-0032-04 — `dim` o `metric` incompatibles se rechazan con error accionable.
    #[test] // @spec AC-0032-04
    fn test_ac_0032_04_dimension_or_metric_mismatch_rejected() {
        let mut registry = ModelRegistry::new();
        registry
            .register("m", 3, Metric::Cosine, 2)
            .expect("register");

        let bad_dim = sample_embedding("m", vec![0.1, 0.2], Metric::Cosine);
        let dim_error = registry
            .validate(&bad_dim)
            .expect_err("dimensión distinta debe rechazarse");
        assert!(matches!(dim_error, RuscaError::InvalidConfig(_)));
        assert!(dim_error.to_string().contains("dimensión"));

        let bad_metric = sample_embedding("m", vec![0.1, 0.2, 0.3], Metric::L2);
        let metric_error = registry
            .validate(&bad_metric)
            .expect_err("métrica distinta debe rechazarse");
        assert!(matches!(metric_error, RuscaError::InvalidConfig(_)));
        assert!(metric_error.to_string().contains("métrica"));
    }

    /// BVA — `register` valida `model_id` vacío y `dim == 0`.
    #[test]
    fn test_register_rejects_empty_id_and_zero_dim() {
        let mut registry = ModelRegistry::new();
        assert!(matches!(
            registry.register("", 3, Metric::L2, 1),
            Err(RuscaError::InvalidConfig(_))
        ));
        assert!(matches!(
            registry.register("m", 0, Metric::L2, 1),
            Err(RuscaError::InvalidConfig(_))
        ));
        assert!(registry.is_empty(), "un registro inválido no inserta nada");
    }

    /// AC-0009-01 — el embedding es determinista y reproducible.
    #[test] // @spec AC-0009-01
    fn test_ac_0009_01_embedding_is_deterministic() {
        let embedder = HashingEmbedder::new(64).expect("embedder");
        let first = embedder.embed("hola mundo");
        let second = embedder.embed("hola mundo");
        assert_eq!(first, second);

        let other = HashingEmbedder::new(64).expect("otro embedder");
        assert_eq!(first, other.embed("hola mundo"));
    }

    /// AC-0009-02 — el vector tiene exactamente la dimensión configurada.
    #[test] // @spec AC-0009-02
    fn test_ac_0009_02_embedding_dimension() {
        for dim in [1usize, 8, 32, 384] {
            let embedder = HashingEmbedder::new(dim).expect("embedder");
            let vector = embedder.embed("texto de prueba");
            assert_eq!(vector.len(), dim, "dim {dim}");
            assert_eq!(embedder.dim(), dim);
        }
    }

    /// AC-0009-03 — textos distintos producen vectores distintos.
    #[test] // @spec AC-0009-03
    fn test_ac_0009_03_distinct_texts_differ() {
        let embedder = HashingEmbedder::new(256).expect("embedder");
        assert_ne!(embedder.embed("gato"), embedder.embed("perro"));
        assert_ne!(embedder.embed("uno"), embedder.embed("dos"));
    }

    /// AC-0009-04 — el vector no nulo está normalizado en L2.
    #[test] // @spec AC-0009-04
    fn test_ac_0009_04_embedding_is_normalized() {
        let embedder = HashingEmbedder::new(128).expect("embedder");
        let vector = embedder.embed("vector de prueba");
        assert!(
            (l2_norm(&vector) - 1.0).abs() < 1e-5,
            "norma {} no es ~1",
            l2_norm(&vector)
        );
    }

    /// `new(0)` es configuración inválida; `new(1)` es válido.
    #[test]
    fn test_new_rejects_zero_dimension() {
        assert!(matches!(
            HashingEmbedder::new(0),
            Err(RuscaError::InvalidConfig(_))
        ));
        assert_eq!(HashingEmbedder::new(1).expect("dim 1").dim(), 1);
    }

    /// FNV-1a de 64 bits coincide con vectores de prueba conocidos.
    #[test]
    fn test_fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    /// El hashing firmado queda anclado a un vector conocido (dim = 8).
    #[test]
    fn test_embedding_known_vector_pins_hashing() {
        let embedder = HashingEmbedder::new(8).expect("embedder");
        // FNV-1a("a") = 0xaf63dc4c8601ec8c ⇒ índice 4 y bit de signo activo.
        assert_eq!(
            embedder.embed("a"),
            vec![0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0]
        );
    }

    /// El signo del hashing queda anclado a un vector positivo conocido (dim = 8).
    #[test]
    fn test_embedding_known_vector_positive_sign() {
        let embedder = HashingEmbedder::new(8).expect("embedder");
        // FNV-1a("hola") = 0x4029fbcc7e6d3137 ⇒ índice 7 y bit de signo apagado.
        assert_eq!(
            embedder.embed("hola"),
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
        );
    }

    /// La tokenización es insensible a mayúsculas y minúsculas.
    #[test]
    fn test_embedding_is_case_insensitive() {
        let embedder = HashingEmbedder::new(64).expect("embedder");
        assert_eq!(embedder.embed("GATO Azul"), embedder.embed("gato azul"));
    }

    /// Texto vacío o solo espacios produce el vector cero (norma 0).
    #[test]
    fn test_empty_text_zero_vector() {
        let embedder = HashingEmbedder::new(16).expect("embedder");
        let zeros = vec![0.0_f32; 16];
        assert_eq!(embedder.embed(""), zeros);
        assert_eq!(embedder.embed("   \t\n"), zeros);
    }

    /// `normalize_l2` no divide por cero y normaliza vectores no nulos.
    #[test]
    fn test_normalize_l2_handles_zero_and_nonzero() {
        let mut zero = [0.0_f32, 0.0];
        normalize_l2(&mut zero);
        assert_eq!(zero, [0.0, 0.0]);

        let mut unit = [3.0_f32, 4.0];
        normalize_l2(&mut unit);
        assert!((l2_norm(&unit) - 1.0).abs() < 1e-6);
    }

    proptest! {
        /// La dimensión del vector siempre coincide con la configurada.
        #[test]
        fn prop_embedding_has_configured_dimension(dim in 1usize..128) {
            let embedder = HashingEmbedder::new(dim).expect("embedder");
            prop_assert_eq!(embedder.embed("hola").len(), dim);
        }

        /// Un único token no vacío produce norma L2 exactamente 1.
        #[test]
        fn prop_single_token_is_normalized(word in "[a-zA-Z]{1,12}") {
            let embedder = HashingEmbedder::new(64).expect("embedder");
            let vector = embedder.embed(&word);
            let norm = l2_norm(&vector);
            prop_assert!((norm - 1.0).abs() < 1e-4, "norma {}", norm);
        }

        /// PBT de compatibilidad: con registro vacío todo embedding es válido
        /// (incluye `model_id` vacío y `dim == 0` como casos BVA).
        #[test]
        fn prop_empty_registry_accepts_any_embedding(
            model_id in "[a-zA-Z0-9_-]{0,12}",
            values in prop::collection::vec(-10.0f32..10.0f32, 0..16),
        ) {
            let registry = ModelRegistry::new();
            let meta = EmbeddingMeta {
                model_id,
                dim: values.len(),
                metric: Metric::InnerProduct,
            };
            let embedding = Embedding::new(values, meta).expect("embedding válido");
            prop_assert!(
                registry.validate(&embedding).is_ok(),
                "registro vacío nunca debe rechazar"
            );
        }
    }
}
