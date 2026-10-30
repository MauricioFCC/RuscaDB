//! Block-Max WAND para ranking de texto (SPEC-0177).
//!
//! WAND acota el score maximo alcanzable por documento y descarta bloques
//! enteros cuando su cota no supera el umbral del top-k (el score del k-esimo
//! resultado). Es el algoritmo que usa Lucene para `top_k` sobre posting lists
//! largas: exacto, pero sin puntuar documentos que no pueden entrar en el
//! corte.
//!
//! Las posting lists llegan ordenadas por documento; cada bloque de
//! `block_size` entradas precalcula su maximo.

/// Lista de postings de un termino.
#[derive(Debug, Clone, PartialEq)]
pub struct PostingList {
    /// Identificadores de documento (ascendentes).
    pub doc_ids: Vec<u64>,
    /// Score de cada documento (misma longitud que `doc_ids`).
    pub scores: Vec<f32>,
    /// Entradas por bloque.
    pub block_size: usize,
}

impl PostingList {
    /// Crea una posting list con bloques de 64.
    ///
    /// Args:
    ///     doc_ids: Documentos ascendentes.
    ///     scores: Scores alineados con `doc_ids`.
    ///
    /// Returns:
    ///     Lista (se recorta a la longitud comun si hay desalineacion).
    pub fn new(doc_ids: Vec<u64>, scores: Vec<f32>) -> Self {
        Self::with_block_size(doc_ids, scores, 64)
    }

    /// Crea una posting list con tamano de bloque explicito.
    ///
    /// Args:
    ///     doc_ids: Documentos ascendentes.
    ///     scores: Scores alineados.
    ///     block_size: Entradas por bloque (minimo 1).
    ///
    /// Returns:
    ///     Lista normalizada.
    pub fn with_block_size(doc_ids: Vec<u64>, scores: Vec<f32>, block_size: usize) -> Self {
        let len = doc_ids.len().min(scores.len());
        let mut doc_ids = doc_ids;
        let mut scores = scores;
        doc_ids.truncate(len);
        scores.truncate(len);
        Self {
            doc_ids,
            scores,
            block_size: block_size.max(1),
        }
    }

    /// Score maximo del bloque que contiene `position`.
    ///
    /// Args:
    ///     position: Indice dentro de la lista.
    ///
    /// Returns:
    ///     Maximo del bloque, o `f32::NEG_INFINITY` si esta vacio.
    pub fn block_max(&self, position: usize) -> f32 {
        if self.scores.is_empty() {
            return f32::NEG_INFINITY;
        }
        let start = (position / self.block_size) * self.block_size;
        let end = (start + self.block_size).min(self.scores.len());
        self.scores[start..end]
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max)
    }

    /// Numero de entradas.
    ///
    /// Returns:
    ///     Longitud de la lista.
    pub fn len(&self) -> usize {
        self.doc_ids.len()
    }

    /// Indica si la lista esta vacia.
    ///
    /// Returns:
    ///     `true` sin postings.
    pub fn is_empty(&self) -> bool {
        self.doc_ids.is_empty()
    }
}

/// Motor Block-Max WAND.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockMaxWand {
    /// Tamano de bloque usado al puntuar (informativo).
    pub block_size: usize,
}

impl Default for BlockMaxWand {
    fn default() -> Self {
        Self { block_size: 64 }
    }
}

impl BlockMaxWand {
    /// Crea el motor.
    ///
    /// Returns:
    ///     Motor con bloques de 64.
    pub fn new() -> Self {
        Self::default()
    }

    /// Crea el motor con tamano de bloque explicito.
    ///
    /// Args:
    ///     block_size: Entradas por bloque (minimo 1).
    ///
    /// Returns:
    ///     Motor configurado.
    pub fn with_block_size(block_size: usize) -> Self {
        Self {
            block_size: block_size.max(1),
        }
    }

    /// Top-k exacto de pares `(documento, score)`.
    ///
    /// Args:
    ///     scores: Pares ya puntuados.
    ///     k: Resultados deseados.
    ///
    /// Returns:
    ///     Los `k` mayores por score (desempate por documento ascendente).
    pub fn top_k(&self, scores: &[(u64, f32)], k: usize) -> Vec<(u64, f32)> {
        let mut ranked = scores.to_vec();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(k);
        ranked
    }

    /// Top-k con Block-Max WAND sobre varias posting lists.
    ///
    /// Args:
    ///     lists: Posting lists ordenadas por documento.
    ///     k: Resultados deseados.
    ///
    /// Returns:
    ///     Top-k exacto ordenado por score descendente y documento ascendente.
    pub fn search(&self, lists: &[PostingList], k: usize) -> Vec<(u64, f32)> {
        if k == 0 || lists.is_empty() {
            return Vec::new();
        }
        let mut cursors = vec![0usize; lists.len()];
        let mut results: Vec<(u64, f32)> = Vec::with_capacity(k);
        let mut threshold = f32::NEG_INFINITY;

        // Documento minimo entre las listas activas.
        while let Some(document) = lists
            .iter()
            .zip(&cursors)
            .filter(|(list, cursor)| **cursor < list.len())
            .map(|(list, cursor)| list.doc_ids[*cursor])
            .min()
        {
            // Cota superior conservadora: maximo del bloque actual de cada lista.
            let upper_bound: f32 = lists
                .iter()
                .zip(&cursors)
                .filter(|(list, cursor)| **cursor < list.len())
                .map(|(list, cursor)| list.block_max(*cursor))
                .sum();

            if results.len() == k && upper_bound <= threshold {
                // Ningun documento de estos bloques puede entrar en el top-k:
                // salta al bloque siguiente en las listas que apuntan al doc.
                for (list, cursor) in lists.iter().zip(cursors.iter_mut()) {
                    if *cursor < list.len() && list.doc_ids[*cursor] == document {
                        let next_block = (*cursor / list.block_size + 1) * list.block_size;
                        *cursor = next_block.min(list.len());
                    }
                }
                continue;
            }

            let mut score = 0.0f32;
            for (list, cursor) in lists.iter().zip(cursors.iter_mut()) {
                if *cursor < list.len() && list.doc_ids[*cursor] == document {
                    score += list.scores[*cursor];
                    *cursor += 1;
                }
            }

            if score > 0.0 {
                results.push((document, score));
                results.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                results.truncate(k);
                if results.len() == k {
                    threshold = results.last().map(|entry| entry.1).unwrap_or(threshold);
                }
            }
        }

        results
    }

    /// Version de fuerza bruta (oraculo de los tests).
    ///
    /// Args:
    ///     lists: Posting lists.
    ///     k: Resultados deseados.
    ///
    /// Returns:
    ///     Top-k exacto sin poda.
    pub fn brute_force(&self, lists: &[PostingList], k: usize) -> Vec<(u64, f32)> {
        use std::collections::BTreeMap;
        let mut totals: BTreeMap<u64, f32> = BTreeMap::new();
        for list in lists {
            for (&document, &score) in list.doc_ids.iter().zip(&list.scores) {
                *totals.entry(document).or_insert(0.0) += score;
            }
        }
        let mut ranked: Vec<(u64, f32)> = totals.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(k);
        ranked
    }
}
