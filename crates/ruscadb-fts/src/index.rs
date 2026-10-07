//! Índice invertido `término -> postings` con ranking BM25 y borrado lógico.

use std::collections::{BTreeMap, BTreeSet};

use ruscadb_core::RecordId;
use serde::{Deserialize, Serialize};

use crate::bm25::Bm25;
use crate::tokenizer::tokenize;

/// Entrada de una lista de postings: documento y frecuencia de término.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    /// Documento que contiene el término.
    pub doc: RecordId,
    /// Número de apariciones del término dentro del documento (`tf`).
    pub tf: u32,
}

/// Índice invertido con estadísticas del corpus y borrado lógico.
///
/// Mantiene el mapa `término -> postings` ordenado por [`RecordId`], la
/// longitud de cada documento vivo y el conjunto de documentos borrados
/// (tombstones). El ranking BM25 se calcula en [`InvertedIndex::search`].
///
/// # Invariantes
///
/// - Cada lista de postings está ordenada de forma ascendente por `doc` y sin
///   duplicados.
/// - Todo `doc` referenciado por un posting está en `docs` (vivo) o en
///   `deleted` (tombstone); nunca en ambos.
/// - `docs` contiene exactamente los documentos vivos.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InvertedIndex {
    postings: BTreeMap<String, Vec<Posting>>,
    docs: BTreeMap<RecordId, u32>,
    deleted: BTreeSet<RecordId>,
}

impl InvertedIndex {
    /// Crea un índice invertido vacío.
    ///
    /// Returns:
    ///     Un índice sin términos ni documentos.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserta o reemplaza el documento `id` con el contenido `text`.
    ///
    /// La operación es idempotente por documento: si `id` ya existía, sus
    /// postings previos se descartan y se reindexa el texto nuevo. Si `id`
    /// estaba borrado, deja de ser un tombstone.
    ///
    /// Args:
    ///     id: Identificador del documento.
    ///     text: Texto indexado; se tokeniza con [`tokenize`].
    pub fn insert(&mut self, id: RecordId, text: &str) {
        self.drop_postings(&id);
        self.deleted.remove(&id);

        let tokens = tokenize(text);
        self.docs.insert(id, tokens.len() as u32);
        for (term, term_freq) in frequencies(&tokens) {
            self.push_posting(term, id, term_freq);
        }
    }

    /// Marca `id` como borrado (tombstone).
    ///
    /// El documento deja de contar como vivo y sus postings se excluyen del
    /// ranking. Es idempotente: borrar un `id` desconocido no tiene efecto.
    ///
    /// Args:
    ///     id: Identificador del documento a borrar.
    pub fn remove(&mut self, id: &RecordId) {
        self.docs.remove(id);
        self.deleted.insert(*id);
    }

    /// Indica si `term` está presente en el índice vivo.
    ///
    /// Args:
    ///     term: Término consultado; se normaliza a minúsculas.
    ///
    /// Returns:
    ///     `true` si algún documento vivo contiene el término.
    pub fn contains_term(&self, term: &str) -> bool {
        let normalized = term.to_lowercase();
        self.postings
            .get(&normalized)
            .is_some_and(|postings| postings.iter().any(|posting| self.is_live(posting)))
    }

    /// Número de documentos vivos del índice.
    ///
    /// Returns:
    ///     Cantidad de documentos no borrados.
    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }

    /// Número de términos distintos presentes en documentos vivos.
    ///
    /// Returns:
    ///     Cantidad de claves del índice con al menos un posting vivo.
    pub fn term_count(&self) -> usize {
        self.postings
            .values()
            .filter(|postings| postings.iter().any(|posting| self.is_live(posting)))
            .count()
    }

    /// Busca `query` y devuelve hasta `k` documentos ordenados por BM25.
    ///
    /// La consulta multi-término se resuelve como **unión (OR)** de los
    /// postings de sus términos únicos; la puntuación BM25 se acumula por
    /// documento. Los tombstones se excluyen. El desempate es estable por
    /// [`RecordId`] ascendente.
    ///
    /// # Complejidad
    ///
    /// `O(sum(df))` sobre los términos únicos de la consulta, sin recorrer el
    /// resto de documentos del corpus. El ordenamiento posterior es
    /// `O(c log c)` con `c` documentos candidatos (`c <= sum(df)`).
    ///
    /// Args:
    ///     query: Texto de la consulta; se tokeniza con [`tokenize`].
    ///     k: Número máximo de resultados; `k == 0` devuelve vacío.
    ///
    /// Returns:
    ///     Pares `(id, score)` ordenados por relevancia descendente, como
    ///     máximo `k` elementos.
    pub fn search(&self, query: &str, k: usize) -> Vec<(RecordId, f32)> {
        if k == 0 {
            return Vec::new();
        }
        let average = self.average_doc_len();
        let bm25 = Bm25::default();
        let mut scores: BTreeMap<RecordId, f32> = BTreeMap::new();
        for term in unique_terms(query) {
            self.accumulate_term(&term, &bm25, average, &mut scores);
        }
        rank(scores, k)
    }

    /// Acumula en `scores` la contribución BM25 del término `term`.
    ///
    /// Args:
    ///     term: Término ya normalizado.
    ///     bm25: Ponderador BM25 activo.
    ///     average: Longitud media de los documentos vivos.
    ///     scores: Acumulador de puntuaciones por documento.
    fn accumulate_term(
        &self,
        term: &str,
        bm25: &Bm25,
        average: f32,
        scores: &mut BTreeMap<RecordId, f32>,
    ) {
        let Some(postings) = self.postings.get(term) else {
            return;
        };
        let doc_freq = postings
            .iter()
            .filter(|posting| self.is_live(posting))
            .count();
        if doc_freq == 0 {
            return;
        }
        let idf = bm25.idf(self.docs.len(), doc_freq);
        for posting in postings {
            if !self.is_live(posting) {
                continue;
            }
            let Some(&doc_len) = self.docs.get(&posting.doc) else {
                continue;
            };
            let contribution = idf * bm25.tf_norm(posting.tf, doc_len, average);
            *scores.entry(posting.doc).or_insert(0.0) += contribution;
        }
    }

    /// Inserta el posting `(id, term_freq)` en la lista del término `term`.
    ///
    /// Args:
    ///     term: Término normalizado.
    ///     id: Documento del posting.
    ///     term_freq: Frecuencia de término.
    fn push_posting(&mut self, term: &str, id: RecordId, term_freq: u32) {
        let postings = self.postings.entry(term.to_string()).or_default();
        let position = postings.partition_point(|posting| posting.doc < id);
        postings.insert(
            position,
            Posting {
                doc: id,
                tf: term_freq,
            },
        );
    }

    /// Elimina de los postings todo rastro del documento `id`.
    ///
    /// Args:
    ///     id: Documento a eliminar del índice.
    fn drop_postings(&mut self, id: &RecordId) {
        for postings in self.postings.values_mut() {
            postings.retain(|posting| posting.doc != *id);
        }
        self.postings.retain(|_, postings| !postings.is_empty());
    }

    /// Comprueba si un posting pertenece a un documento vivo.
    ///
    /// Args:
    ///     posting: Posting a comprobar.
    ///
    /// Returns:
    ///     `true` si el documento no está marcado como borrado.
    fn is_live(&self, posting: &Posting) -> bool {
        !self.deleted.contains(&posting.doc)
    }

    /// Longitud media de los documentos vivos (`avgdl`).
    ///
    /// Returns:
    ///     La media de `doc_len`; `0.0` si no hay documentos vivos.
    fn average_doc_len(&self) -> f32 {
        if self.docs.is_empty() {
            return 0.0;
        }
        let total: u64 = self.docs.values().map(|&len| u64::from(len)).sum();
        total as f32 / self.docs.len() as f32
    }
}

/// Cuenta la frecuencia de cada término presente en `tokens`.
///
/// Args:
///     tokens: Términos normalizados en orden de aparición.
///
/// Returns:
///     Mapa `término -> tf` ordenado de forma lexicográfica.
fn frequencies(tokens: &[String]) -> BTreeMap<&str, u32> {
    let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
    for token in tokens {
        *counts.entry(token.as_str()).or_default() += 1;
    }
    counts
}

/// Devuelve los términos únicos de la consulta, ordenados y sin repetir.
///
/// Args:
///     query: Texto crudo de la consulta.
///
/// Returns:
///     Términos normalizados únicos en orden lexicográfico.
fn unique_terms(query: &str) -> Vec<String> {
    let mut terms = tokenize(query);
    terms.sort();
    terms.dedup();
    terms
}

/// Ordena las puntuaciones por relevancia y recorta a `k` resultados.
///
/// El desempate es estable por [`RecordId`] ascendente.
///
/// Args:
///     scores: Puntuaciones acumuladas por documento.
///     k: Número máximo de resultados.
///
/// Returns:
///     Pares `(id, score)` en orden de relevancia descendente.
fn rank(scores: BTreeMap<RecordId, f32>, k: usize) -> Vec<(RecordId, f32)> {
    let mut ranked: Vec<(RecordId, f32)> = scores.into_iter().collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.truncate(k);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invariante: cada lista de postings queda ordenada ascendente por `doc`.
    ///
    /// Endurecimiento adversarial (AdverTest) del mutante de `push_posting`
    /// (`<` -> `==`): un punto de partición incorrecto inserta al principio y
    /// rompe el orden, violando el invariante documentado del índice. El
    /// mutante `<=` es equivalente porque `insert` descarta los postings del
    /// documento antes de reinsertar (nunca hay ids duplicados por término).
    #[test]
    fn postings_stay_sorted_ascending_by_doc() {
        let mut index = InvertedIndex::new();
        let mut ids: Vec<RecordId> = (0..4).map(|_| RecordId::new()).collect();
        ids.sort();
        for id in &ids {
            index.insert(*id, "alfa beta gamma");
        }
        let postings = index.postings.get("alfa").expect("el término existe");
        assert_eq!(postings.len(), ids.len());
        assert!(
            postings.windows(2).all(|pair| pair[0].doc < pair[1].doc),
            "los postings deben estar ordenados por doc: {postings:?}"
        );
    }
}
