//! # ruscadb-ai
//!
//! Inferencia local de embeddings de RuscaDB: **Candle** por defecto (puro
//! Rust) con backend `onnx` opcional. Modelos: CLIP (imagen/texto), Whisper
//! (audio→texto), e5/MiniLM (texto).
//!
//! Implementa el puerto `EmbedPort` de `ruscadb-core`.
//! Diseño: ADR-005 y `docs/RuscaDB-roadmap.md` §6. Fase: F4.

#![forbid(unsafe_code)]

use ruscadb_core::RuscaError;

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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// Norma L2 de un vector.
    fn l2_norm(vector: &[f32]) -> f32 {
        vector.iter().map(|value| value * value).sum::<f32>().sqrt()
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
    }
}
