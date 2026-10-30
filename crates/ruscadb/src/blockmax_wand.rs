//! Re-export de Block-Max WAND (SPEC-0177): la implementacion vive en
//! `ruscadb-fts` y el indice invertido la usa en `search_wand`.

pub use ruscadb_fts::{BlockMaxWand, PostingList};
#[cfg(test)]
mod tests {
    use super::{BlockMaxWand, PostingList};

    /// AC-0177-01: `top_k` ordena por score y desempata por documento.
    #[test]
    // @spec AC-0177-01
    fn test_ac_0177_01_topk() {
        let wand = BlockMaxWand::new();
        let ranked = wand.top_k(&[(1, 0.5), (2, 0.9), (3, 0.1)], 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].0, 2);

        let ties = wand.top_k(&[(9, 1.0), (4, 1.0)], 2);
        assert_eq!(ties, vec![(4, 1.0), (9, 1.0)], "desempate por doc");
        assert!(wand.top_k(&[(1, 1.0)], 0).is_empty());
    }

    /// AC-0177-02: la poda no cambia el resultado frente a fuerza bruta.
    #[test]
    // @spec AC-0177-02
    fn test_ac_0177_02_prune() {
        // Dos terminos: "a" en docs pares, "b" en docs multiplos de 3.
        let docs_a: Vec<u64> = (0..200).map(|i| i * 2).collect();
        let scores_a: Vec<f32> = (0..200).map(|i| 1.0 + (i % 7) as f32 * 0.1).collect();
        let docs_b: Vec<u64> = (0..140).map(|i| i * 3).collect();
        let scores_b: Vec<f32> = (0..140).map(|i| 0.5 + (i % 5) as f32 * 0.2).collect();

        let lists = vec![
            PostingList::with_block_size(docs_a, scores_a, 8),
            PostingList::with_block_size(docs_b, scores_b, 8),
        ];
        let wand = BlockMaxWand::with_block_size(8);
        for k in [1usize, 5, 10, 25] {
            assert_eq!(
                wand.search(&lists, k),
                wand.brute_force(&lists, k),
                "k={k} debe ser exacto"
            );
        }

        // Listas vacias y k=0 son no-op.
        assert!(wand.search(&[], 3).is_empty());
        assert!(wand.search(&lists, 0).is_empty());
        assert!(PostingList::new(vec![], vec![]).is_empty());
        assert_eq!(PostingList::new(vec![1], vec![1.0]).block_max(0), 1.0);
    }

    /// Una sola lista y listas disjuntas: la poda sigue siendo exacta.
    #[test]
    fn search_handles_single_and_disjoint_lists() {
        let wand = BlockMaxWand::new();
        let single = PostingList::new(vec![1, 2, 3], vec![0.5, 2.0, 1.0]);
        let one = std::slice::from_ref(&single);
        assert_eq!(wand.search(one, 2), vec![(2, 2.0), (3, 1.0)]);
        assert_eq!(wand.search(one, 2), wand.brute_force(one, 2));

        let left = PostingList::new(vec![1, 3], vec![1.0, 1.0]);
        let right = PostingList::new(vec![2, 4], vec![2.0, 2.0]);
        let lists = vec![left, right];
        assert_eq!(wand.search(&lists, 4), wand.brute_force(&lists, 4));
        assert_eq!(wand.search(&lists, 1), vec![(2, 2.0)], "desempate por doc");
    }

    /// Listas desalineadas se recortan a la longitud comun.
    #[test]
    fn posting_list_truncates_misaligned_input() {
        let list = PostingList::with_block_size(vec![1, 2, 3], vec![1.0], 2);
        assert_eq!(list.len(), 1);
        assert_eq!(list.block_max(0), 1.0);
        assert!(PostingList::with_block_size(vec![], vec![1.0], 0).is_empty());
    }
}
