//! # ruscadb-fts
//!
//! Full-text search de RuscaDB: **tokenizador Unicode**, **índice invertido**
//! (postings con frecuencia de término) y **ranking BM25**. Especificación:
//! `specs/full_text_search.md` (SPEC-0014).
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §5.4 (modelo full-text: índice invertido +
//! BM25 con `k1 = 1.2`, `b = 0.75`).

#![forbid(unsafe_code)]

mod bm25;
mod index;
mod tokenizer;

pub use bm25::Bm25;
pub use index::{InvertedIndex, Posting};
pub use tokenizer::tokenize;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use ruscadb_core::RecordId;

    /// Compara dos flotantes con una tolerancia absoluta.
    ///
    /// Args:
    ///     left: Valor observado.
    ///     right: Valor esperado.
    ///
    /// Returns:
    ///     `true` si la diferencia absoluta es menor que `1e-4`.
    fn approx_eq(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-4
    }

    /// AC-0014-01 — tokeniza, separa y normaliza a minúsculas.
    #[test]
    fn test_ac_0014_01_tokenize_lowercases_and_splits() {
        assert_eq!(
            tokenize("the quick brown fox"),
            vec!["the", "quick", "brown", "fox"]
        );
        assert_eq!(tokenize("¡Gato, PERRO!"), vec!["gato", "perro"]);
        assert_eq!(tokenize(""), Vec::<String>::new());
        assert_eq!(tokenize("   ,,,   "), Vec::<String>::new());
    }

    /// AC-0014-02 — solo aparecen los documentos que contienen el término.
    #[test]
    fn test_ac_0014_02_inverted_index_returns_matching_docs() {
        let mut index = InvertedIndex::new();
        let cat = RecordId::new();
        let dog = RecordId::new();
        index.insert(cat, "gato");
        index.insert(dog, "perro");

        let hits = index.search("gato", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, cat);
        assert!(!hits.iter().any(|(id, _)| *id == dog));
        assert_eq!(index.search("perro", 10)[0].0, dog);
        assert!(index.search("pajaro", 10).is_empty());
        assert_eq!(index.doc_count(), 2);
    }

    /// AC-0014-03 — BM25 ordena primero el documento más relevante.
    #[test]
    fn test_ac_0014_03_bm25_ranks_relevant_first() {
        let mut index = InvertedIndex::new();
        let dense = RecordId::new();
        let sparse = RecordId::new();
        let long = RecordId::new();
        index.insert(dense, "gato gato gato");
        index.insert(sparse, "gato");
        index.insert(long, "gato perro ave pez rata topo");

        let hits = index.search("gato", 10);
        assert_eq!(
            hits.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![dense, sparse, long]
        );
        assert!(hits[0].1 > hits[1].1);
        assert!(hits[1].1 > hits[2].1);
    }

    /// AC-0014-04 — el roundtrip serde preserva el ranking.
    #[test]
    fn test_ac_0014_04_serde_roundtrip_preserves_ranking() {
        let mut index = InvertedIndex::new();
        index.insert(RecordId::new(), "gato gato gato");
        index.insert(RecordId::new(), "gato perro");
        index.insert(RecordId::new(), "perro ave pez rata topo");

        let before = index.search("gato perro", 10);
        let encoded = serde_json::to_string(&index).expect("serializa el índice");
        let decoded: InvertedIndex = serde_json::from_str(&encoded).expect("deserializa el índice");
        assert_eq!(before, decoded.search("gato perro", 10));
        assert_eq!(index, decoded);
    }

    /// AC-0014-05 — un documento borrado no aparece en los resultados.
    #[test]
    fn test_ac_0014_05_deleted_docs_are_excluded() {
        let mut index = InvertedIndex::new();
        let kept = RecordId::new();
        let removed = RecordId::new();
        index.insert(kept, "gato perro");
        index.insert(removed, "gato gato gato");

        index.remove(&removed);

        let hits = index.search("gato", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, kept);
        assert_eq!(index.doc_count(), 1);
        assert!(index.contains_term("gato"));

        index.remove(&removed);
        assert_eq!(index.doc_count(), 1);
    }

    /// El término desaparece del índice vivo cuando se borra su único doc.
    #[test]
    fn test_contains_term_reflects_live_docs() {
        let mut index = InvertedIndex::new();
        let id = RecordId::new();
        index.insert(id, "Gato");
        assert!(index.contains_term("gato"));
        assert!(index.contains_term("GATO"));
        assert!(!index.contains_term("perro"));

        index.remove(&id);
        assert!(!index.contains_term("gato"));
        assert_eq!(index.term_count(), 0);
    }

    /// Reinsertar un `id` reemplaza su contenido (idempotencia por documento).
    #[test]
    fn test_reinsert_replaces_content() {
        let mut index = InvertedIndex::new();
        let id = RecordId::new();
        index.insert(id, "gato");
        index.insert(id, "perro");

        assert_eq!(index.doc_count(), 1);
        assert_eq!(index.term_count(), 1);
        assert!(index.search("gato", 10).is_empty());
        assert_eq!(index.search("perro", 10)[0].0, id);
    }

    /// Reinsertar un `id` borrado lo revive con el contenido nuevo.
    #[test]
    fn test_reinsert_after_remove_revives_document() {
        let mut index = InvertedIndex::new();
        let id = RecordId::new();
        index.insert(id, "gato");
        index.remove(&id);
        assert!(index.search("gato", 10).is_empty());

        index.insert(id, "gato");
        assert_eq!(index.search("gato", 10)[0].0, id);
        assert_eq!(index.doc_count(), 1);
    }

    /// Una consulta multi-término es la unión (OR) de sus postings.
    #[test]
    fn test_search_multi_term_is_union() {
        let mut index = InvertedIndex::new();
        let both = RecordId::new();
        let one = RecordId::new();
        let other = RecordId::new();
        index.insert(both, "gato perro");
        index.insert(one, "gato");
        index.insert(other, "perro");

        let hits = index.search("gato perro", 10);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].0, both);
        assert!(hits.iter().any(|(id, _)| *id == one));
        assert!(hits.iter().any(|(id, _)| *id == other));
    }

    /// Un término repetido en la consulta no multiplica su peso.
    #[test]
    fn test_repeated_query_terms_count_once() {
        let mut index = InvertedIndex::new();
        index.insert(RecordId::new(), "gato");
        assert_eq!(index.search("gato", 10), index.search("gato gato gato", 10));
    }

    /// BVA de `k`: 0, 1, exacto y mayor que el número de documentos.
    #[test]
    fn test_search_k_boundaries() {
        let mut index = InvertedIndex::new();
        for _ in 0..3 {
            index.insert(RecordId::new(), "gato");
        }

        assert!(index.search("gato", 0).is_empty());
        assert_eq!(index.search("gato", 1).len(), 1);
        assert_eq!(index.search("gato", 3).len(), 3);
        assert_eq!(index.search("gato", 4).len(), 3);
        assert_eq!(index.search("gato", usize::MAX).len(), 3);
    }

    /// Un índice vacío devuelve vacío sin entrar en panic.
    #[test]
    fn test_empty_index_search_is_empty() {
        let index = InvertedIndex::new();
        assert!(index.search("gato", 10).is_empty());
        assert_eq!(index.doc_count(), 0);
        assert_eq!(index.term_count(), 0);
        assert!(!index.contains_term("gato"));
    }

    /// `Default` reproduce los parámetros BM25 del roadmap.
    #[test]
    fn test_bm25_default_parameters() {
        let bm25 = Bm25::default();
        assert!(approx_eq(bm25.k1, 1.2));
        assert!(approx_eq(bm25.b, 0.75));
    }

    /// IDF con valores conocidos y casos límite.
    #[test]
    fn test_bm25_idf_known_values() {
        let bm25 = Bm25::default();
        assert!(approx_eq(bm25.idf(3, 3), 0.133_531_4));
        assert!(approx_eq(bm25.idf(4, 1), 1.203_972_8));
        assert!(approx_eq(bm25.idf(0, 0), 0.0));
        assert!(approx_eq(bm25.idf(3, 0), 0.0));
    }

    /// `tf_norm` con valores conocidos y casos límite.
    #[test]
    fn test_bm25_tf_norm_known_values() {
        let bm25 = Bm25::default();
        assert!(approx_eq(bm25.tf_norm(1, 2, 2.0), 1.0));
        assert!(approx_eq(bm25.tf_norm(3, 3, 3.0), 1.571_428_6));
        assert!(approx_eq(bm25.tf_norm(0, 3, 3.0), 0.0));
        assert!(approx_eq(bm25.tf_norm(1, 1, 0.0), 0.0));
        assert!(approx_eq(bm25.tf_norm(1, 2, -1.0), 0.0));
    }

    /// `score` combina IDF y normalización de `tf`.
    #[test]
    fn test_bm25_score_combines_components() {
        let bm25 = Bm25::default();
        let expected = bm25.idf(3, 3) * bm25.tf_norm(3, 3, 3.0);
        assert!(approx_eq(bm25.score(3, 3, 3.0, 3, 3), expected));
    }

    /// La puntuación de `search` coincide con la fórmula BM25 del roadmap.
    #[test]
    fn test_search_score_matches_bm25_formula() {
        let mut index = InvertedIndex::new();
        let target = RecordId::new();
        index.insert(target, "gato gato");
        index.insert(RecordId::new(), "perro");

        let hits = index.search("gato", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, target);
        assert!(approx_eq(hits[0].1, 0.871_385));
    }

    /// Las listas de postings se mantienen ordenadas por `RecordId`.
    #[test]
    fn test_postings_are_sorted_by_doc() {
        let lower = RecordId::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("ULID válido");
        let higher = RecordId::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAW").expect("ULID válido");
        assert!(lower < higher, "los ULID de prueba deben ordenarse igual");

        let mut index = InvertedIndex::new();
        index.insert(higher, "gato");
        index.insert(lower, "gato");

        let value = serde_json::to_value(&index).expect("serializa el índice");
        let postings = value["postings"]["gato"].as_array().expect("postings");
        let docs: Vec<String> = postings
            .iter()
            .map(|posting| posting["doc"].as_str().expect("doc textual").to_string())
            .collect();
        assert_eq!(docs, vec![lower.to_string(), higher.to_string()]);
    }

    /// Estadísticas de corpus: docs, términos y longitud media.
    #[test]
    fn test_corpus_statistics() {
        let mut index = InvertedIndex::new();
        index.insert(RecordId::new(), "gato perro");
        index.insert(RecordId::new(), "gato");
        assert_eq!(index.doc_count(), 2);
        assert_eq!(index.term_count(), 2);
    }

    /// AC-0037-01 — recupera todos los documentos con términos del prefijo.
    #[test]
    fn test_ac_0037_01_prefix_matches_all() {
        let mut index = InvertedIndex::new();
        let dense = RecordId::new();
        let ghost = RecordId::new();
        let other = RecordId::new();
        index.insert(dense, "gato gato gato");
        index.insert(ghost, "gata");
        index.insert(other, "perro");

        let hits = index.search_prefix("ga", 10);
        let ids: Vec<RecordId> = hits.iter().map(|(id, _)| *id).collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(ids[0], dense, "el documento con mayor tf debe ir primero");
        assert!(ids.contains(&ghost));
        assert!(!ids.contains(&other));

        // BVA de `k`: exacto, 1 y mayor que el número de documentos.
        assert_eq!(index.search_prefix("ga", 2).len(), 2);
        assert_eq!(index.search_prefix("ga", 1).len(), 1);
        assert_eq!(index.search_prefix("ga", usize::MAX).len(), 2);

        // BVA de prefijo: 1 carácter y normalización a minúsculas.
        assert_eq!(index.search_prefix("g", 10).len(), 2);
        assert_eq!(index.search_prefix("GATA", 10)[0].0, ghost);
    }

    /// AC-0037-02 — un prefijo sin coincidencias devuelve vacío.
    #[test]
    fn test_ac_0037_02_prefix_no_match() {
        let mut index = InvertedIndex::new();
        index.insert(RecordId::new(), "gato");
        assert!(index.search_prefix("zzz", 10).is_empty());
        assert!(index.search_prefix("gatos", 10).is_empty());
        assert!(index.search_prefix("perro", 10).is_empty());
    }

    /// AC-0037-03 — prefijo vacío (y `k == 0`) devuelve vacío sin panics.
    #[test]
    fn test_ac_0037_03_empty_prefix() {
        let mut index = InvertedIndex::new();
        let id = RecordId::new();
        index.insert(id, "gato");
        assert!(index.search_prefix("", 10).is_empty());
        assert!(index.search_prefix("   ,,,   ", 10).is_empty());
        assert!(index.search_prefix("ga", 0).is_empty());
        assert!(InvertedIndex::new().search_prefix("ga", 10).is_empty());
    }

    /// AC-0037-04 — un prefijo igual a un término completo es consistente con
    /// `search` (mismos documentos y mismas puntuaciones).
    #[test]
    fn test_ac_0037_04_prefix_of_full_term_consistent() {
        let mut index = InvertedIndex::new();
        let dense = RecordId::new();
        let sparse = RecordId::new();
        index.insert(dense, "gato gato");
        index.insert(sparse, "gato");
        index.insert(RecordId::new(), "perro");

        let exact = index.search("gato", 10);
        assert_eq!(index.search_prefix("gato", 10), exact);
        assert_eq!(index.search_prefix("GATO", 10), exact);
        assert_eq!(index.search_prefix("gato", 1), index.search("gato", 1));
        assert_eq!(index.search_prefix("gato", 10).len(), 2);
    }

    /// AC-0037-05 — los tombstones no aparecen en los resultados del prefijo.
    #[test]
    fn test_ac_0037_05_prefix_excludes_deleted() {
        let mut index = InvertedIndex::new();
        let kept = RecordId::new();
        let removed = RecordId::new();
        index.insert(kept, "gato");
        index.insert(removed, "gato gato gato");
        index.remove(&removed);

        let hits = index.search_prefix("ga", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, kept);
        assert!(!hits.iter().any(|(id, _)| *id == removed));

        index.remove(&kept);
        assert!(index.search_prefix("ga", 10).is_empty());
        assert!(index.search_prefix("gato", 10).is_empty());
    }

    proptest! {
        /// La tokenización nunca entra en panic y es determinista.
        #[test]
        fn prop_tokenize_never_panics_and_is_deterministic(text in any::<String>()) {
            let tokens = tokenize(&text);
            for token in &tokens {
                prop_assert!(!token.is_empty());
                prop_assert_eq!(token, &token.to_lowercase());
            }
            prop_assert_eq!(tokens, tokenize(&text));
        }

        /// El ranking es determinista para un mismo corpus y consulta.
        #[test]
        fn prop_search_is_deterministic(
            docs in prop::collection::vec(any::<String>(), 0..6),
            query in any::<String>(),
        ) {
            let mut index = InvertedIndex::new();
            for text in &docs {
                index.insert(RecordId::new(), text);
            }
            prop_assert_eq!(index.search(&query, 10), index.search(&query, 10));
        }

        /// Un documento insertado se recupera por cualquiera de sus términos.
        #[test]
        fn prop_inserted_doc_recovers_by_any_term(
            text in any::<String>(),
            k in 1usize..10,
        ) {
            let id = RecordId::new();
            let mut index = InvertedIndex::new();
            index.insert(id, &text);
            for term in tokenize(&text) {
                let hits = index.search(&term, k);
                prop_assert!(
                    hits.iter().any(|(doc, _)| *doc == id),
                    "el término {:?} no recupera el documento", term
                );
            }
        }
    }

    proptest! {
        /// Soundness de `search_prefix` (SPEC-0037): todo documento devuelto
        /// está vivo, tiene algún término que comienza por el prefijo y el
        /// número de resultados respeta `k`.
        #[test]
        fn prop_search_prefix_is_sound(
            docs in prop::collection::vec(any::<String>(), 0..8),
            prefix in "[a-z]{1,4}",
            k in 0usize..12,
        ) {
            let mut index = InvertedIndex::new();
            let mut inserted: Vec<(RecordId, String)> = Vec::new();
            for text in &docs {
                let id = RecordId::new();
                index.insert(id, text);
                inserted.push((id, text.clone()));
            }

            let hits = index.search_prefix(&prefix, k);
            prop_assert!(hits.len() <= k, "resultados {} > k {}", hits.len(), k);
            for (id, _) in &hits {
                let document = inserted.iter().find(|(doc, _)| doc == id);
                prop_assert!(document.is_some(), "documento {id:?} no insertado");
                if let Some((_, text)) = document {
                    let matched = tokenize(text).iter().any(|term| term.starts_with(&prefix));
                    prop_assert!(
                        matched,
                        "el documento {id:?} no tiene término con prefijo {prefix:?}"
                    );
                }
            }
        }
    }
}
