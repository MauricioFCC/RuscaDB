//! # ruscadb-fvs
//!
//! **Filtrado vectorial híbrido (FVS)** de RuscaDB: elige la estrategia de
//! filtrado por selectividad (`post-filtering`, `iFVS` in-filter o
//! `pre-filtering`) para maximizar QPS-recall en búsquedas ANN con predicado.
//! Especificación: `specs/filtered_vector_search.md` (SPEC-0021).
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §5.4 y evidencia iFVS (arXiv:2607.22922).
//!
//! # Propiedades
//!
//! - **Sonido**: ninguna estrategia devuelve ids fuera de `allowed`.
//! - **Exactitud**: `PreFilter` e `InFilter` coinciden con la fuerza bruta
//!   restringida al filtro.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use ruscadb_core::{Metric, RuscaError};
use ruscadb_vector::distance;

/// Umbral de selectividad a partir del cual se usa `PostFilter` (`s >= 0.6`).
pub const POST_THRESHOLD: f32 = 0.6;

/// Umbral de selectividad a partir del cual se usa `InFilter` (`s >= 0.05`).
pub const PRE_THRESHOLD: f32 = 0.05;

/// Factor de sobre-muestreo del `PostFilter` (top-`k * OVERSAMPLE`).
const OVERSAMPLE: usize = 4;

/// Estrategia de filtrado vectorial elegida por selectividad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FvsStrategy {
    /// Pre-filtra candidatos (puntúa solo `allowed`) y hace top-k exacto.
    PreFilter,
    /// iFVS: aplica el filtro durante el recorrido (top-k exacto in-filter).
    InFilter,
    /// Top-k del corpus completo y luego filtra por `allowed`.
    PostFilter,
}

/// Selectividad `s = |filtro| / total`; `0.0` si el corpus está vacío.
///
/// Args:
///     filter_len: Número de ids del filtro.
///     total: Número de vectores del corpus.
///
/// Returns:
///     La selectividad en `[0, 1]`, o `0.0` si `total == 0`.
pub fn selectivity(filter_len: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        filter_len as f32 / total as f32
    }
}

/// Elige la estrategia FVS según la selectividad.
///
/// Args:
///     selectivity: Selectividad `s` del filtro.
///
/// Returns:
///     `PostFilter` si `s >= POST_THRESHOLD`; `InFilter` si
///     `s >= PRE_THRESHOLD`; `PreFilter` en otro caso.
pub fn choose_strategy(selectivity: f32) -> FvsStrategy {
    if selectivity >= POST_THRESHOLD {
        FvsStrategy::PostFilter
    } else if selectivity >= PRE_THRESHOLD {
        FvsStrategy::InFilter
    } else {
        FvsStrategy::PreFilter
    }
}

/// Corpus vectorial plano (fuerza bruta) con una métrica fija.
pub struct VectorSet {
    /// Métrica de distancia del espacio.
    pub metric: Metric,
    entries: Vec<(u64, Vec<f32>)>,
}

impl VectorSet {
    /// Crea un corpus vacío para la métrica dada.
    ///
    /// Args:
    ///     metric: Métrica de distancia del espacio.
    ///
    /// Returns:
    ///     Un `VectorSet` sin entradas.
    pub fn new(metric: Metric) -> Self {
        Self {
            metric,
            entries: Vec::new(),
        }
    }

    /// Inserta un vector con su id, validando dimensiones uniformes.
    ///
    /// Args:
    ///     id: Identificador del vector.
    ///     vector: Valores `f32` del vector.
    ///
    /// Returns:
    ///     `Ok(())` si la inserción tuvo éxito.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si `vector` no tiene la dimensión
    ///     del primer vector insertado.
    pub fn insert(&mut self, id: u64, vector: Vec<f32>) -> Result<(), RuscaError> {
        if let Some((_, first)) = self.entries.first() {
            if first.len() != vector.len() {
                return Err(RuscaError::DimensionMismatch {
                    expected: first.len(),
                    actual: vector.len(),
                });
            }
        }
        self.entries.push((id, vector));
        Ok(())
    }

    /// Número de vectores del corpus.
    ///
    /// Returns:
    ///     La cantidad de entradas.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Indica si el corpus está vacío.
    ///
    /// Returns:
    ///     `true` si no hay entradas.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Vector asociado a `id`, si existe.
    ///
    /// Args:
    ///     id: Identificador buscado.
    ///
    /// Returns:
    ///     Los valores del vector, o `None` si `id` no está en el corpus.
    pub fn get(&self, id: u64) -> Option<&[f32]> {
        self.entries
            .iter()
            .find(|(entry_id, _)| *entry_id == id)
            .map(|(_, vector)| vector.as_slice())
    }
}

/// Ordena `(id, distancia)` por distancia ascendente y desempate por `id`.
///
/// Args:
///     scores: Pares `(id, distancia)` a ordenar en sitio.
fn sort_scores(scores: &mut [(u64, f32)]) {
    scores.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
}

/// Top-k exacto restringido al filtro.
///
/// Puntúa únicamente los ids de `allowed` (pre-filter) y devuelve el top-k
/// exacto; es también el resultado de referencia de iFVS (in-filter).
///
/// Args:
///     set: Corpus vectorial.
///     query: Vector de consulta.
///     k: Número máximo de resultados.
///     allowed: Ids permitidos por el predicado.
///
/// Returns:
///     Hasta `k` pares `(id, distancia)` ordenados; vacío si `k == 0`.
///
/// Errors:
///     [`RuscaError::DimensionMismatch`] si `query` no coincide con la
///     dimensión del corpus.
fn exact_top_k(
    set: &VectorSet,
    query: &[f32],
    k: usize,
    allowed: &BTreeSet<u64>,
) -> Result<Vec<(u64, f32)>, RuscaError> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let mut scores: Vec<(u64, f32)> = Vec::with_capacity(allowed.len());
    for &id in allowed {
        if let Some(vector) = set.get(id) {
            scores.push((id, distance(set.metric, query, vector)?));
        }
    }
    sort_scores(&mut scores);
    scores.truncate(k);
    Ok(scores)
}

/// Top-`k * OVERSAMPLE` del corpus completo, filtrado luego por `allowed`.
///
/// Args:
///     set: Corpus vectorial.
///     query: Vector de consulta.
///     k: Número máximo de resultados finales.
///     allowed: Ids permitidos por el predicado.
///
/// Returns:
///     Hasta `k` pares `(id, distancia)` sonoros (siempre en `allowed`);
///     vacío si `k == 0`.
///
/// Errors:
///     [`RuscaError::DimensionMismatch`] si `query` no coincide con la
///     dimensión del corpus.
fn post_top_k(
    set: &VectorSet,
    query: &[f32],
    k: usize,
    allowed: &BTreeSet<u64>,
) -> Result<Vec<(u64, f32)>, RuscaError> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let oversample = k.saturating_mul(OVERSAMPLE);
    let mut scores: Vec<(u64, f32)> = Vec::with_capacity(set.len());
    for (id, vector) in &set.entries {
        scores.push((*id, distance(set.metric, query, vector)?));
    }
    sort_scores(&mut scores);
    scores.truncate(oversample);
    Ok(scores
        .into_iter()
        .filter(|(id, _)| allowed.contains(id))
        .take(k)
        .collect())
}

/// Búsqueda filtrada con la estrategia elegida.
///
/// Args:
///     set: Corpus vectorial.
///     query: Vector de consulta (misma dimensión que el corpus).
///     k: Número máximo de resultados.
///     allowed: Ids permitidos por el predicado.
///     strategy: Estrategia FVS a aplicar.
///
/// Returns:
///     Hasta `k` pares `(id, distancia)` ordenados por distancia ascendente y
///     desempate estable por `id`. `PreFilter` e `InFilter` son exactos;
///     `PostFilter` es sonido (nunca devuelve ids fuera de `allowed`).
///
/// Errors:
///     [`RuscaError::DimensionMismatch`] si `query` no coincide con la
///     dimensión del corpus.
pub fn search_filtered(
    set: &VectorSet,
    query: &[f32],
    k: usize,
    allowed: &BTreeSet<u64>,
    strategy: FvsStrategy,
) -> Result<Vec<(u64, f32)>, RuscaError> {
    match strategy {
        FvsStrategy::PreFilter | FvsStrategy::InFilter => exact_top_k(set, query, k, allowed),
        FvsStrategy::PostFilter => post_top_k(set, query, k, allowed),
    }
}

/// Búsqueda filtrada eligiendo la estrategia por selectividad.
///
/// Args:
///     set: Corpus vectorial.
///     query: Vector de consulta.
///     k: Número máximo de resultados.
///     allowed: Ids permitidos por el predicado.
///
/// Returns:
///     El resultado de [`search_filtered`] con la estrategia de
///     [`choose_strategy`] aplicada sobre [`selectivity`].
///
/// Errors:
///     [`RuscaError::DimensionMismatch`] si `query` no coincide con la
///     dimensión del corpus.
pub fn search_auto(
    set: &VectorSet,
    query: &[f32],
    k: usize,
    allowed: &BTreeSet<u64>,
) -> Result<Vec<(u64, f32)>, RuscaError> {
    let strategy = choose_strategy(selectivity(allowed.len(), set.len()));
    search_filtered(set, query, k, allowed, strategy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use rstest::rstest;
    use std::ops::Range;

    /// Construye un corpus desde entradas `(id, vector)`.
    fn build(metric: Metric, entries: &[(u64, Vec<f32>)]) -> VectorSet {
        let mut set = VectorSet::new(metric);
        for (id, vector) in entries {
            set.insert(*id, vector.clone()).expect("insert");
        }
        set
    }

    /// Conjunto de ids del rango semiabierto `range`.
    fn ids(range: Range<u64>) -> BTreeSet<u64> {
        range.collect()
    }

    /// Ordena `(id, distancia)` por distancia asc y desempate estable por id.
    fn sorted(mut scored: Vec<(u64, f32)>) -> Vec<(u64, f32)> {
        scored.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        scored
    }

    /// Oráculo de fuerza bruta restringido al filtro (top-k exacto).
    fn brute_force_filtered(
        metric: Metric,
        entries: &[(u64, Vec<f32>)],
        query: &[f32],
        k: usize,
        allowed: &BTreeSet<u64>,
    ) -> Vec<(u64, f32)> {
        let scored: Vec<(u64, f32)> = entries
            .iter()
            .filter(|(id, _)| allowed.contains(id))
            .map(|(id, vector)| (*id, distance(metric, query, vector).expect("distance")))
            .collect();
        sorted(scored).into_iter().take(k).collect()
    }

    /// Ajusta `values` a `dim` rellenando con ceros o truncando.
    fn fit(values: &[f32], dim: usize) -> Vec<f32> {
        let mut vector = values.to_vec();
        vector.resize(dim, 0.0);
        vector.truncate(dim);
        vector
    }

    /// Corpus compartido de las pruebas de aceptación.
    fn sample_entries() -> Vec<(u64, Vec<f32>)> {
        vec![
            (10, vec![0.0, 0.0]),
            (20, vec![1.0, 0.0]),
            (30, vec![0.0, 1.0]),
            (40, vec![5.0, 5.0]),
            (50, vec![10.0, 10.0]),
        ]
    }

    /// AC-0021-01 — selección de estrategia por umbrales (frontera 0.6 / 0.05).
    #[rstest]
    #[case(1.0, FvsStrategy::PostFilter)]
    #[case(0.75, FvsStrategy::PostFilter)]
    #[case(0.60, FvsStrategy::PostFilter)]
    #[case(0.599, FvsStrategy::InFilter)]
    #[case(0.30, FvsStrategy::InFilter)]
    #[case(0.05, FvsStrategy::InFilter)]
    #[case(0.049, FvsStrategy::PreFilter)]
    #[case(0.0, FvsStrategy::PreFilter)]
    fn test_ac_0021_01_strategy_by_selectivity(
        #[case] selectivity: f32,
        #[case] expected: FvsStrategy,
    ) {
        assert_eq!(choose_strategy(selectivity), expected);
    }

    /// AC-0021-01 — la selectividad es `|filtro| / total` y `0.0` sin corpus.
    #[test]
    fn test_ac_0021_01_selectivity_ratio() {
        assert_eq!(selectivity(0, 0), 0.0);
        assert!((selectivity(3, 10) - 0.3).abs() < 1e-6);
        assert_eq!(selectivity(10, 10), 1.0);
        assert_eq!(selectivity(0, 10), 0.0);
    }

    /// AC-0021-02 — el pre-filtering es el top-k exacto restringido al filtro.
    #[test]
    fn test_ac_0021_02_pre_filter_is_exact() {
        let entries = sample_entries();
        let set = build(Metric::L2, &entries);
        let allowed = ids(20..50); // {20, 30, 40}
        let query = [0.9, 0.0];

        let got =
            search_filtered(&set, &query, 2, &allowed, FvsStrategy::PreFilter).expect("search");
        let expected = brute_force_filtered(Metric::L2, &entries, &query, 2, &allowed);
        assert_eq!(got, expected);
        assert_eq!(
            got.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![20, 30]
        );
    }

    /// Desempate estable por `id` ante distancias idénticas.
    #[test]
    fn test_sort_ties_break_by_id_stable() {
        // Insertados fuera de orden: si se pierde el desempate, gana [2, 1].
        let entries = vec![(2, vec![1.0, 0.0]), (1, vec![-1.0, 0.0])];
        let set = build(Metric::L2, &entries);
        let allowed = ids(1..3);
        let got = search_filtered(&set, &[0.0, 0.0], 2, &allowed, FvsStrategy::PreFilter)
            .expect("search");
        assert_eq!(
            got.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(got[0].1, got[1].1);
    }

    /// AC-0021-03 — el post-filtering nunca devuelve ids fuera del filtro.
    #[test]
    fn test_ac_0021_03_post_filter_is_sound() {
        // Los vecinos más cercanos NO están permitidos.
        let entries = vec![
            (1, vec![0.0, 0.0]),
            (2, vec![0.1, 0.0]),
            (3, vec![0.2, 0.0]),
            (4, vec![0.3, 0.0]),
            (10, vec![9.0, 0.0]),
            (11, vec![9.5, 0.0]),
        ];
        let set = build(Metric::L2, &entries);

        // A) k=1 => sobre-muestreo 4; {10, 11} quedan fuera => vacío.
        let allowed = ids(10..12);
        let got = search_filtered(&set, &[0.0, 0.0], 1, &allowed, FvsStrategy::PostFilter)
            .expect("search");
        assert!(got.iter().all(|(id, _)| allowed.contains(id)));
        assert!(got.is_empty());

        // B) Si el permitido cae dentro del sobre-muestreo, se devuelve.
        let entries2 = vec![
            (1, vec![0.0, 0.0]),
            (2, vec![0.1, 0.0]),
            (10, vec![0.2, 0.0]),
            (3, vec![5.0, 0.0]),
            (4, vec![6.0, 0.0]),
        ];
        let set2 = build(Metric::L2, &entries2);
        let allowed2 = ids(10..11);
        let got2 = search_filtered(&set2, &[0.0, 0.0], 1, &allowed2, FvsStrategy::PostFilter)
            .expect("search");
        let expected = distance(Metric::L2, &[0.0, 0.0], &[0.2, 0.0]).expect("distance");
        assert_eq!(got2, vec![(10, expected)]);

        // C) Resultado no vacío y sonoro con vecinos excluidos por delante.
        let allowed3 = BTreeSet::from([4u64, 11]);
        let got3 = search_filtered(&set, &[0.0, 0.0], 2, &allowed3, FvsStrategy::PostFilter)
            .expect("search");
        let d4 = distance(Metric::L2, &[0.0, 0.0], &[0.3, 0.0]).expect("distance");
        let d11 = distance(Metric::L2, &[0.0, 0.0], &[9.5, 0.0]).expect("distance");
        assert_eq!(got3, vec![(4, d4), (11, d11)]);
        assert!(got3.iter().all(|(id, _)| allowed3.contains(id)));
    }

    /// AC-0021-04 — iFVS coincide con el pre-filtering (exacto).
    #[test]
    fn test_ac_0021_04_ifvs_matches_pre_filter() {
        let entries = sample_entries();
        let set = build(Metric::Cosine, &entries);
        let allowed = ids(10..50); // {10, 20, 30, 40}
        let query = [0.7, 0.7];
        for k in 0..=entries.len() + 1 {
            let pre =
                search_filtered(&set, &query, k, &allowed, FvsStrategy::PreFilter).expect("pre");
            let in_filter =
                search_filtered(&set, &query, k, &allowed, FvsStrategy::InFilter).expect("in");
            assert_eq!(in_filter, pre, "k={k}");
        }
    }

    /// AC-0021-05 — fronteras: filtro vacío, filtro total, k=0 y k>N.
    #[test]
    fn test_ac_0021_05_boundary_filters() {
        let entries = vec![(1, vec![0.0]), (2, vec![1.0]), (3, vec![2.0])];
        let set = build(Metric::L2, &entries);
        let empty: BTreeSet<u64> = BTreeSet::new();
        let all = ids(1..4);
        let query = [0.0];

        let strategies = [
            FvsStrategy::PreFilter,
            FvsStrategy::InFilter,
            FvsStrategy::PostFilter,
        ];
        for strategy in strategies {
            // Filtro vacío => vacío.
            assert!(
                search_filtered(&set, &query, 2, &empty, strategy)
                    .expect("search")
                    .is_empty()
            );
            // k == 0 => vacío.
            assert!(
                search_filtered(&set, &query, 0, &all, strategy)
                    .expect("search")
                    .is_empty()
            );
        }

        // Filtro total con k <= N: exacto y completo.
        let full = search_filtered(&set, &query, 3, &all, FvsStrategy::PreFilter).expect("search");
        assert_eq!(
            full.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        // k > N se acota a N sin panics.
        let capped =
            search_filtered(&set, &query, 99, &all, FvsStrategy::PreFilter).expect("search");
        assert_eq!(capped.len(), 3);
        let post =
            search_filtered(&set, &query, 99, &all, FvsStrategy::PostFilter).expect("search");
        assert_eq!(post.len(), 3);
        assert_eq!(selectivity(0, 0), 0.0);
    }

    /// `search_auto` elige la estrategia por selectividad y es exacto en
    /// selectividades bajas (pre-filter).
    #[test]
    fn test_search_auto_selects_strategy() {
        let entries = sample_entries();
        let set = build(Metric::L2, &entries);
        let query = [0.0, 0.0];
        let allowed = ids(10..11); // s = 0.2 => InFilter
        let auto = search_auto(&set, &query, 1, &allowed).expect("auto");
        let explicit =
            search_filtered(&set, &query, 1, &allowed, FvsStrategy::InFilter).expect("explicit");
        assert_eq!(auto, explicit);
    }

    /// `get`/`len`/`is_empty` y validación de dimensiones.
    #[test]
    fn test_vector_set_accessors_and_dimension_guard() {
        let mut set = VectorSet::new(Metric::L2);
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.get(7), None);

        set.insert(7, vec![1.0, 2.0]).expect("insert");
        assert!(!set.is_empty());
        assert_eq!(set.len(), 1);
        assert_eq!(set.get(7), Some([1.0, 2.0].as_slice()));
        assert!(matches!(
            set.insert(8, vec![1.0, 2.0, 3.0]),
            Err(RuscaError::DimensionMismatch {
                expected: 2,
                actual: 3
            })
        ));
    }

    /// Una consulta con dimensión distinta devuelve `DimensionMismatch`.
    #[test]
    fn test_search_dimension_mismatch() {
        let set = build(Metric::L2, &[(1, vec![0.0, 0.0])]);
        let allowed = ids(1..2);
        let result = search_filtered(&set, &[0.0, 0.0, 0.0], 1, &allowed, FvsStrategy::PreFilter);
        assert!(matches!(result, Err(RuscaError::DimensionMismatch { .. })));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// Propiedad: PreFilter == fuerza bruta filtrada (oráculo exacto).
        #[test]
        fn prop_pre_filter_matches_brute_force(
            rows in proptest::collection::vec(proptest::collection::vec(-5.0f32..5.0, 1..5), 1..12),
            mask in proptest::collection::vec(any::<bool>(), 1..12),
            query_raw in proptest::collection::vec(-5.0f32..5.0, 1..5),
            k in 0usize..6,
        ) {
            let dim = rows[0].len();
            let entries: Vec<(u64, Vec<f32>)> = rows
                .iter()
                .enumerate()
                .map(|(index, row)| (index as u64 + 1, fit(row, dim)))
                .collect();
            let set = build(Metric::L2, &entries);
            let allowed: BTreeSet<u64> = entries
                .iter()
                .enumerate()
                .filter(|(index, _)| mask.get(*index).copied().unwrap_or(false))
                .map(|(_, (id, _))| *id)
                .collect();
            let query = fit(&query_raw, dim);

            let got =
                search_filtered(&set, &query, k, &allowed, FvsStrategy::PreFilter).expect("pre");
            let expected = brute_force_filtered(Metric::L2, &entries, &query, k, &allowed);
            prop_assert_eq!(got, expected);
        }

        /// Propiedad: todas las estrategias son sonoras y devuelven a lo sumo k.
        #[test]
        fn prop_all_strategies_are_sound(
            rows in proptest::collection::vec(proptest::collection::vec(-5.0f32..5.0, 1..5), 1..12),
            mask in proptest::collection::vec(any::<bool>(), 1..12),
            query_raw in proptest::collection::vec(-5.0f32..5.0, 1..5),
            k in 0usize..8,
        ) {
            let dim = rows[0].len();
            let entries: Vec<(u64, Vec<f32>)> = rows
                .iter()
                .enumerate()
                .map(|(index, row)| (index as u64 + 1, fit(row, dim)))
                .collect();
            let set = build(Metric::L2, &entries);
            let allowed: BTreeSet<u64> = entries
                .iter()
                .enumerate()
                .filter(|(index, _)| mask.get(*index).copied().unwrap_or(false))
                .map(|(_, (id, _))| *id)
                .collect();
            let query = fit(&query_raw, dim);

            for strategy in [
                FvsStrategy::PreFilter,
                FvsStrategy::InFilter,
                FvsStrategy::PostFilter,
            ] {
                let got = search_filtered(&set, &query, k, &allowed, strategy).expect("search");
                prop_assert!(got.len() <= k);
                prop_assert!(got.iter().all(|(id, _)| allowed.contains(id)));
                for pair in got.windows(2) {
                    prop_assert!(pair[0].1 <= pair[1].1);
                }
            }
        }
    }
}
