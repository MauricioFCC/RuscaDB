//! Búsqueda híbrida término+vector (SPEC-0063).
//!
//! Pipeline local y sin red: recall por dos vías (BM25 en `ruscadb-fts` y KNN
//! en `ruscadb-vector`), fusión **RRF** (inversión de rango, score-agnostic) y
//! re-scoring **late-interaction** opcional por overlap de términos (proxy
//! determinista de MaxSim; el almacén de embeddings palabra-nivel queda para
//! una spec posterior, decisión abierta #3).
//!
//! Degradación graceful: sin vía vectorial → BM25 puro (`Bm25Only`); sin vía
//! textual → KNN puro (`KnnOnly`); con ambas → `Rrf` (o `MaxSim` si el
//! re-scoring reordena el top-k).

use std::collections::BTreeMap;

use ruscadb_core::{Record, RecordId, RuscaError, ScalarValue};

use crate::catalog::Catalog;
use crate::database::Database;
use crate::heap::heap_scan;

/// Constante RRF: desplazamiento del ranking (K+rank), evita división por 0.
const RRF_K_RECIPROCAL: u32 = 60;
/// Multiplicador para pedir N > k candidatos por vía antes de fusionar.
const RRF_CANDIDATES_MULTIPLIER: usize = 4;
/// Dimensión de los token-embeddings deterministas (SPEC-0072).
const TOKEN_DIM: usize = 32;

/// Etapa que determinó el orden final de un hit híbrido (SPEC-0063).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridStage {
    /// Fusión RRF de ambas vías (texto y vector).
    Rrf,
    /// Re-scoring late-interaction (overlap) reordenó la fusión RRF.
    MaxSim,
    /// Solo existía la vía textual (degradación a BM25 puro).
    Bm25Only,
    /// Solo existía la vía vectorial (degradación a KNN puro).
    KnnOnly,
}

/// Hit de la búsqueda híbrida (SPEC-0063).
#[derive(Debug, Clone, PartialEq)]
pub struct HybridHit {
    /// Identidad del registro ganador.
    pub id: RecordId,
    /// Score de la etapa final (RRF o overlap si hubo re-scoring).
    pub score: f32,
    /// Etapa que determinó el orden final.
    pub stage: HybridStage,
}

/// Pesos por vía para la fusión RRF (SPEC-0063).
///
/// Por defecto ambos valen `1.0` (fusionado sin sesgo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridWeights {
    /// Peso de la vía textual (BM25).
    pub text: f32,
    /// Peso de la vía vectorial (KNN).
    pub vector: f32,
}

impl Default for HybridWeights {
    /// Pesos neutros: las dos vías pesan igual.
    fn default() -> Self {
        Self {
            text: 1.0,
            vector: 1.0,
        }
    }
}

/// Tokeniza en minúsculas separando por espacios (proxy de MaxSim).
///
/// Args:
///     text: Texto a tokenizar.
///
/// Returns:
///     Los términos en minúsculas.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Hash FNV-1a de 64 bits (base determinista de los token-embeddings).
///
/// Args:
///     bytes: Bytes del token.
///
/// Returns:
///     El hash FNV-1a.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Mezclador splitmix64 (expansión de una semilla a 64 bits de calidad).
///
/// Args:
///     seed: Semilla.
///
/// Returns:
///     El valor mezclado.
fn splitmix64(seed: u64) -> u64 {
    let mut value = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Token-embedding determinista por hash (hash embeddings, Svenstrup 2017).
///
/// Proyecta el token a `TOKEN_DIM` componentes en `[-1, 1]` normalizados L2;
/// sin modelo entrenado ni estado (mismo token → mismo vector).
///
/// Args:
///     token: Término (en minúsculas).
///
/// Returns:
///     El vector unitario del token (cero si el token es vacío).
fn token_embedding(token: &str) -> [f32; TOKEN_DIM] {
    let mut vector = [0.0_f32; TOKEN_DIM];
    let seed = fnv1a(token.as_bytes());
    for (index, component) in vector.iter_mut().enumerate() {
        let mixed = splitmix64(seed.wrapping_add(index as u64));
        let byte = (mixed & 0xff) as f32;
        *component = byte / 127.5 - 1.0;
    }
    let norm: f32 = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for component in &mut vector {
            *component /= norm;
        }
    }
    vector
}

/// Producto punto de dos token-embeddings normalizados (coseno).
///
/// Args:
///     left: Primer vector unitario.
///     right: Segundo vector unitario.
///
/// Returns:
///     El coseno en `[-1, 1]`.
fn dot(left: &[f32; TOKEN_DIM], right: &[f32; TOKEN_DIM]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

/// MaxSim de late-interaction (ColBERT): `Σ_i max_j cos(q_i, d_j)`.
///
/// Args:
///     query_tokens: Términos de la consulta.
///     doc_tokens: Términos del documento.
///
/// Returns:
///     El score MaxSim (0 si algún lado está vacío); continuo, no entero.
fn maxsim(query_tokens: &[String], doc_tokens: &[String]) -> f32 {
    if query_tokens.is_empty() || doc_tokens.is_empty() {
        return 0.0;
    }
    let doc_vectors: Vec<[f32; TOKEN_DIM]> = doc_tokens
        .iter()
        .map(|token| token_embedding(token))
        .collect();
    query_tokens
        .iter()
        .map(|query_token| {
            let query_vector = token_embedding(query_token);
            doc_vectors
                .iter()
                .map(|doc_vector| dot(&query_vector, doc_vector))
                .fold(f32::MIN, f32::max)
        })
        .sum()
}

/// Ranking por vía de cada candidato (posición 1-based).
struct ViaRanks {
    /// Posición en la lista BM25 (`None` si no está en ella).
    text: Option<usize>,
    /// Posición en la lista KNN (`None` si no está en ella).
    vector: Option<usize>,
}

impl Database {
    /// Búsqueda híbrida término+vector con fusión RRF + re-scoring.
    ///
    /// Cada vía aporta `N = k * 4` candidatos (recall), se fusionan por
    /// inversión de rango ponderada y el top-k se re-puntúa por overlap de
    /// términos (proxy determinista de MaxSim); si el re-scoring **reordena**,
    /// la etapa se etiqueta `MaxSim`; si no, se sirve la lista RRF (`Rrf`).
    /// La ausencia de una vía **no** es error: degrada a la otra
    /// (`Bm25Only`/`KnnOnly`).
    ///
    /// Args:
    ///     table: Tabla destino.
    ///     text_query: Términos de búsqueda textual (primera columna FTS).
    ///     vector_query: Vector de consulta para el índice HNSW.
    ///     k: Número de hits a devolver.
    ///     weights: Pesos por vía para la fusión.
    ///
    /// Returns:
    ///     Los `k` mejores `HybridHit` (score desc, empates por `RecordId`).
    ///
    /// Errors:
    ///     [`RuscaError::CorruptManifest`] si el catálogo es ilegible;
    ///     [`RuscaError::Io`] si falla el escaneo del heap;
    ///     [`RuscaError::MissingVector`]/`DimensionMismatch` si el KNN falla.
    pub fn hybrid_search(
        &mut self,
        table: &str,
        text_query: Option<&str>,
        vector_query: Option<&[f32]>,
        k: usize,
        weights: HybridWeights,
    ) -> Result<Vec<HybridHit>, RuscaError> {
        let table_def = { Catalog::load(self)?.get(table)?.clone() };
        let rows = heap_scan(self, &table_def)?;
        let live: Vec<(RecordId, &Record)> = rows
            .iter()
            .filter(|(_, record)| record.meta.deleted_tx.is_none())
            .map(|(_, record)| (record.id, record))
            .collect();
        if k == 0 {
            return Ok(Vec::new());
        }
        let candidates = k.saturating_mul(RRF_CANDIDATES_MULTIPLIER);

        let opt_index = self.table_indexes(table);
        let text_column = opt_index.and_then(|index| index.text_columns().into_iter().next());
        let text_list = match (text_column.as_deref(), text_query, opt_index) {
            (Some(column), Some(query), Some(index)) => {
                Some(index.text_search(column, query, candidates))
            }
            _ => None,
        };
        let allowed: std::collections::BTreeSet<RecordId> = live
            .iter()
            .filter(|(_, record)| record.vector.is_some())
            .map(|(id, _)| *id)
            .collect();
        let vector_list = match (vector_query, opt_index, allowed.is_empty()) {
            (Some(query), Some(index), false) if index.has_vector() => {
                Some(index.filtered_vector_search(query, candidates, &allowed)?)
            }
            _ => None,
        };

        let stage = match (text_list.is_some(), vector_list.is_some()) {
            (true, true) => HybridStage::Rrf,
            (true, false) => HybridStage::Bm25Only,
            (false, true) => HybridStage::KnnOnly,
            (false, false) => return Ok(Vec::new()),
        };
        let texts: BTreeMap<RecordId, String> = match &text_column {
            Some(column) => live
                .iter()
                .filter_map(|(id, record)| match record.scalars.get(column) {
                    Some(ScalarValue::Text(value)) => Some((*id, value.clone())),
                    _ => None,
                })
                .collect(),
            None => BTreeMap::new(),
        };

        let fused = fuse(text_list.as_deref(), vector_list.as_deref(), weights);
        let (order, final_stage) = rescore_or_keep(fused, text_query, &texts, k, stage);
        Ok(order
            .into_iter()
            .take(k)
            .map(|(id, score)| HybridHit {
                id,
                score,
                stage: final_stage,
            })
            .collect())
    }
}

/// Fusiona las dos listas por inversión de rango ponderada (RRF).
///
/// Args:
///     text_list: Top-N BM25 (solo ids, orden = ranking).
///     vector_list: Top-N KNN `(id, distancia)` en orden.
///     weights: Pesos por vía.
///
/// Returns:
///     Candidatos `(id, score_rrf)` ordenados por score desc (empate por id).
fn fuse(
    text_list: Option<&[RecordId]>,
    vector_list: Option<&[(RecordId, f32)]>,
    weights: HybridWeights,
) -> Vec<(RecordId, f32)> {
    let mut ranks: BTreeMap<RecordId, ViaRanks> = BTreeMap::new();
    if let Some(list) = text_list {
        for (index, id) in list.iter().enumerate() {
            ranks
                .entry(*id)
                .or_insert(ViaRanks {
                    text: None,
                    vector: None,
                })
                .text = Some(index + 1);
        }
    }
    if let Some(list) = vector_list {
        for (index, (id, _)) in list.iter().enumerate() {
            ranks
                .entry(*id)
                .or_insert(ViaRanks {
                    text: None,
                    vector: None,
                })
                .vector = Some(index + 1);
        }
    }
    let mut scored: Vec<(RecordId, f32)> = ranks
        .into_iter()
        .map(|(id, rank)| {
            let text = rank.text.map_or(0.0_f64, |r| {
                weights.text as f64 / (RRF_K_RECIPROCAL as f64 + r as f64)
            });
            let vector = rank.vector.map_or(0.0_f64, |r| {
                weights.vector as f64 / (RRF_K_RECIPROCAL as f64 + r as f64)
            });
            // Suma conmutativa bit a bit en IEEE: empates deterministas.
            let score = text + vector;
            (id, score as f32)
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    scored
}

/// Re-puntúa el top-k por overlap o conserva la lista fusionada.
///
/// El re-scoring solo aplica cuando ambas vías existen (`Rrf`) y hay columna
/// FTS con candidatos; si no reordena, se sirve la lista RRF con su etapa.
///
/// Args:
///     fused: Lista fusionada ordenada.
///     text_query: Consulta textual (`None` → sin re-scoring).
///     texts: Texto de la columna FTS por `RecordId`.
///     k: Profundidad del re-scoring (top-k).
///     base: Etapa antes del re-scoring.
///
/// Returns:
///     La lista final `(id, score)` y la etapa final (uniforme por lista).
fn rescore_or_keep(
    fused: Vec<(RecordId, f32)>,
    text_query: Option<&str>,
    texts: &BTreeMap<RecordId, String>,
    k: usize,
    base: HybridStage,
) -> (Vec<(RecordId, f32)>, HybridStage) {
    let Some(query) = text_query else {
        return (fused, base);
    };
    let tokens = tokenize(query);
    let eligible = base == HybridStage::Rrf
        && !tokens.is_empty()
        && fused.iter().any(|(id, _)| texts.contains_key(id));
    if !eligible {
        return (fused, base);
    }
    // El re-scoring cubre TODO el top-k (SPEC-0071): limitarlo a una
    // profundidad menor dejaría la cola con scores RRF y rompería la
    // monotonía de la lista devuelta.
    let depth = k;
    let top: Vec<(RecordId, f32)> = fused.iter().take(depth).copied().collect();
    let mut ranked: Vec<(usize, f32)> = top
        .iter()
        .enumerate()
        .map(|(position, (id, _))| {
            // MaxSim real (SPEC-0072) sobre los token-embeddings del documento.
            let score = texts
                .get(id)
                .map_or(0.0, |text| maxsim(&tokens, &tokenize(text)));
            (position, score)
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    let reordered: Vec<(RecordId, f32)> = ranked
        .iter()
        .map(|(position, score)| (top[*position].0, *score))
        .collect();
    let changed = top
        .iter()
        .map(|(id, _)| *id)
        .ne(reordered.iter().map(|(id, _)| *id));
    if !changed {
        // El overlap no reordena: se sirve la lista RRF intacta (scores RRF).
        return (fused, base);
    }
    let mut final_list: Vec<(RecordId, f32)> = reordered;
    final_list.extend(fused.iter().skip(depth).copied());
    (final_list, HybridStage::MaxSim)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// AC-0072-01 — MaxSim premia la cobertura de términos de la consulta.
    #[test]
    // @spec AC-0072-01
    fn test_ac_0072_01_maxsim_rewards_coverage() {
        let query = tokenize("alfa beta gamma");
        let three = maxsim(&query, &tokenize("alfa beta gamma"));
        let two = maxsim(&query, &tokenize("alfa beta"));
        let one = maxsim(&query, &tokenize("alfa"));
        assert!(
            three > two && two > one,
            "la cobertura debe ordenar los scores: {three} > {two} > {one}"
        );
    }

    /// AC-0072-02 — la repetición de un término no infla el score (max por término).
    #[test]
    // @spec AC-0072-02
    fn test_ac_0072_02_maxsim_ignores_repetition() {
        let query = tokenize("alfa beta");
        let repeated = maxsim(&query, &tokenize("alfa alfa alfa beta"));
        let plain = maxsim(&query, &tokenize("alfa beta"));
        assert_eq!(
            repeated, plain,
            "el max por término ignora las apariciones repetidas"
        );
    }

    /// AC-0072-04 — el score es determinista entre llamadas (sin estado).
    #[test]
    // @spec AC-0072-04
    fn test_ac_0072_04_maxsim_is_deterministic() {
        let query = tokenize("gato perro");
        let document = tokenize("gato duerme");
        let first = maxsim(&query, &document);
        let second = maxsim(&query, &document);
        assert_eq!(first.to_bits(), second.to_bits(), "bit a bit idéntico");
        assert_eq!(
            token_embedding("gato"),
            token_embedding("gato"),
            "mismo token → mismo vector"
        );
    }

    /// La fusión RRF puntúa más al candidato presente en ambas vías.
    #[test]
    fn fuse_prioritizes_overlap() {
        let id_a = RecordId::new();
        let id_b = RecordId::new();
        let id_c = RecordId::new();
        let text = vec![id_a, id_b];
        let vector: Vec<(RecordId, f32)> = vec![(id_a, 0.1), (id_c, 0.2)];
        let fused = fuse(Some(&text), Some(&vector), HybridWeights::default());
        assert_eq!(fused[0].0, id_a, "intersección primero (2/61 vs 1/62)");
    }

    /// Una sola vía conserva su orden (score monótono en el ranking).
    #[test]
    fn fuse_single_path_keeps_order() {
        let list = vec![RecordId::new(), RecordId::new(), RecordId::new()];
        let fused = fuse(Some(&list), None, HybridWeights::default());
        let ids: Vec<RecordId> = fused.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, list, "una sola vía conserva su orden");
    }

    /// La re-puntuación por overlap reordena según la densidad de términos.
    #[test]
    fn rescore_reorders_by_overlap() {
        let id_a = RecordId::new();
        let id_b = RecordId::new();
        let fused = vec![(id_a, 0.9), (id_b, 0.8)];
        let mut texts = BTreeMap::new();
        texts.insert(id_a, "alfa".to_string());
        texts.insert(id_b, "alfa beta gamma".to_string());
        let (order, stage) =
            rescore_or_keep(fused, Some("alfa beta gamma"), &texts, 2, HybridStage::Rrf);
        assert_eq!(order[0].0, id_b, "el más redundante pasa primero");
        assert_eq!(stage, HybridStage::MaxSim, "el reorden etiqueta MaxSim");
    }

    /// Sin textos candidatos el re-scoring es no-op y se sirve RRF.
    #[test]
    fn rescore_without_texts_keeps_rrf() {
        let id_a = RecordId::new();
        let fused = vec![(id_a, 0.9)];
        let texts = BTreeMap::new();
        let (order, stage) =
            rescore_or_keep(fused.clone(), Some("gato"), &texts, 1, HybridStage::Rrf);
        assert_eq!(order, fused);
        assert_eq!(stage, HybridStage::Rrf);
    }

    /// AC-0071-01 — con `k` mayor que la profundidad histórica (20) el
    /// re-scoring cubre TODO el top-k y los scores quedan en una sola escala
    /// (monótonos no crecientes).
    #[test]
    // @spec AC-0071-01
    fn test_ac_0071_01_hybrid_scores_monotonic_beyond_depth() {
        let mut fused = Vec::new();
        let mut texts = BTreeMap::new();
        for index in 0..25_usize {
            let id = RecordId::new();
            fused.push((id, 1.0 - index as f32 * 0.01));
            let text = if index >= 20 {
                "alfa beta gamma"
            } else {
                "alfa"
            };
            texts.insert(id, text.to_string());
        }
        let (order, stage) =
            rescore_or_keep(fused, Some("alfa beta gamma"), &texts, 25, HybridStage::Rrf);
        assert_eq!(order.len(), 25, "conserva los 25 candidatos");
        assert_eq!(stage, HybridStage::MaxSim, "el reorden cambia la lista");
        for window in order.windows(2) {
            assert!(
                window[0].1 >= window[1].1,
                "los scores del top-k deben ser no crecientes: {} < {}",
                window[0].1,
                window[1].1
            );
        }
    }
}
