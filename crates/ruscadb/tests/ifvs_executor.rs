//! Integración end-to-end de iFVS en el executor (SPEC-0048).
//!
//! Cubre AC-0048-01..05: el `KNN` con `WHERE` usa `search_auto_indexed`
//! (iFVS sobre HNSW, SPEC-0047) eligiendo pre/in/post por selectividad. `pre` e
//! `in` son exactos; `post` es sonoro y recalcula exacto si no cubre `k`. Sin
//! filtro, el `KNN` conserva el comportamiento previo. Incluye un PBT con
//! oráculo de fuerza bruta filtrada y BVA (filtro vacío, filtro total,
//! `k = 0`, `k > N`).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, ScalarMap, ScalarValue,
};

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.data"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base abierta.
///     table: Nombre de la tabla a crear.
fn create_items(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Inserta una fila con vector en `items` (auto-commit) y devuelve su id.
///
/// Args:
///     database: Base abierta.
///     id: Valor del escalar `id` (filtro `WHERE`).
///     vector: Vector `f64` del embedding.
///
/// Returns:
///     El `RecordId` asignado.
fn insert_vector(database: &mut Database, id: i64, vector: &[f64]) -> RecordId {
    let floats: Vec<f32> = vector.iter().map(|value| *value as f32).collect();
    let record = Record {
        id: RecordId::new(),
        scalars: ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("name".to_string(), ScalarValue::Text(format!("v{id}"))),
        ]),
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(
            Embedding::new(
                floats,
                EmbeddingMeta {
                    model_id: "test".to_string(),
                    dim: vector.len(),
                    metric: Metric::L2,
                },
            )
            .expect("embedding válido"),
        ),
        blob: None,
        meta: RecordMeta::default(),
    };
    database
        .insert_record("items", record)
        .expect("insert_record")
}

/// Inserta todas las entradas `(id, vector)` en `items`.
///
/// Args:
///     database: Base abierta.
///     entries: Filas `(id, vector)` a insertar en orden.
fn insert_entries(database: &mut Database, entries: &[(i64, Vec<f64>)]) {
    for (id, vector) in entries {
        insert_vector(database, *id, vector);
    }
}

/// Extrae el valor entero de la columna `id`.
///
/// Args:
///     row: Fila proyectada.
///
/// Returns:
///     El valor `Int` de `id`.
fn int_of(row: &ruscadb::Row) -> i64 {
    match row.get("id") {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en 'id', se obtuvo {other:?}"),
    }
}

/// Distancia euclídea en `f32` (oráculo de fuerza bruta).
///
/// Args:
///     left: Primer vector.
///     right: Segundo vector.
///
/// Returns:
///     La distancia L2.
fn l2(left: &[f64], right: &[f64]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (*a as f32 - *b as f32).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// Oráculo: ids del top-k exacto restringido al filtro `id >= cutoff`.
///
/// Args:
///     entries: Corpus `(id, vector)`.
///     query: Vector de consulta.
///     cutoff: Umbral escalar del filtro (`i64::MIN` = sin filtro).
///     k: Número máximo de resultados.
///
/// Returns:
///     Los ids del top-k restringido, ordenados por `(distancia, id)`.
fn brute_force_filtered(
    entries: &[(i64, Vec<f64>)],
    query: &[f64],
    cutoff: i64,
    k: usize,
) -> Vec<i64> {
    let mut scored: Vec<(i64, f32)> = entries
        .iter()
        .filter(|(id, _)| *id >= cutoff)
        .map(|(id, vector)| (*id, l2(vector, query)))
        .collect();
    // Mismo desempate que FVS: por distancia y luego por id.
    scored.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    scored.into_iter().take(k).map(|(id, _)| id).collect()
}

/// AC-0048-01 — un `KNN` con filtro moderado (`InFilter`, iFVS) devuelve el
/// top-k exacto restringido al filtro.
#[test]
fn test_ac_0048_01_knn_filter_uses_ifvs_exact() {
    let (_dir, mut database) = open_test_db("ac48_01");
    create_items(&mut database, "items");
    let entries: Vec<(i64, Vec<f64>)> = (0..48)
        .map(|id| (id, vec![id as f64, ((id * 5) % 17) as f64]))
        .collect();
    insert_entries(&mut database, &entries);

    // Selectividad s = 18/48 = 0.375: dentro de [0.05, 0.6) => InFilter (iFVS).
    let cutoff = 30_i64;
    let query = vec![10.0, 8.0];
    let query_clause = format!("[{}, {}]", query[0], query[1]);
    for k in [1_usize, 3, 5, 10] {
        let rows = database
            .execute(&format!(
                "SELECT * FROM items WHERE id >= {cutoff} KNN embedding <|{k}|> {query_clause}"
            ))
            .expect("KNN+WHERE");
        let got: Vec<i64> = rows.iter().map(int_of).collect();
        assert_eq!(
            got,
            brute_force_filtered(&entries, &query, cutoff, k),
            "k={k}: el KNN filtrado con iFVS debe ser exacto"
        );
        assert!(
            got.iter().all(|id| *id >= cutoff),
            "todos los ids respetan el filtro"
        );
    }
}

/// AC-0048-02 — un filtro muy selectivo (`s < 0.05` => `PreFilter` y `s = 0.05`
/// => `InFilter`) devuelve exactamente el top-k restringido al filtro.
#[test]
fn test_ac_0048_02_selective_filter_exact() {
    let (_dir, mut database) = open_test_db("ac48_02");
    create_items(&mut database, "items");
    let entries: Vec<(i64, Vec<f64>)> = (0..40)
        .map(|id| (id, vec![id as f64, ((id * 3) % 11) as f64]))
        .collect();
    insert_entries(&mut database, &entries);

    let query = vec![2.5, 6.0];
    let query_clause = format!("[{}, {}]", query[0], query[1]);
    // s = 2/40 = 0.05 (InFilter) y s = 1/40 = 0.025 (PreFilter): ambos exactos.
    for (cutoff, k) in [(38_i64, 3_usize), (39, 2), (39, 5)] {
        let rows = database
            .execute(&format!(
                "SELECT * FROM items WHERE id >= {cutoff} KNN embedding <|{k}|> {query_clause}"
            ))
            .expect("KNN+WHERE selectivo");
        let got: Vec<i64> = rows.iter().map(int_of).collect();
        assert_eq!(
            got,
            brute_force_filtered(&entries, &query, cutoff, k),
            "cutoff={cutoff} k={k}: filtro selectivo exacto"
        );
        assert!(got.iter().all(|id| *id >= cutoff));
    }
}

/// AC-0048-03 — un filtro total (`s = 1.0` => `PostFilter`) coincide con el
/// `KNN` sin filtro.
#[test]
fn test_ac_0048_03_full_filter_matches_unfiltered() {
    let (_dir, mut database) = open_test_db("ac48_03");
    create_items(&mut database, "items");
    let entries: Vec<(i64, Vec<f64>)> = (0..16)
        .map(|id| (id, vec![id as f64, (id * id) as f64]))
        .collect();
    insert_entries(&mut database, &entries);

    for k in [1_usize, 4, 16, 99] {
        let filtered = database
            .execute(&format!(
                "SELECT * FROM items WHERE id >= 0 KNN embedding <|{k}|> [0.0, 0.0]"
            ))
            .expect("filtro total");
        let unfiltered = database
            .execute(&format!(
                "SELECT * FROM items KNN embedding <|{k}|> [0.0, 0.0]"
            ))
            .expect("sin filtro");
        let filtered_ids: Vec<i64> = filtered.iter().map(int_of).collect();
        let unfiltered_ids: Vec<i64> = unfiltered.iter().map(int_of).collect();
        assert_eq!(
            filtered_ids, unfiltered_ids,
            "k={k}: el filtro total debe coincidir con el KNN sin filtro"
        );
        assert_eq!(
            filtered_ids,
            brute_force_filtered(&entries, &[0.0, 0.0], 0, k),
            "k={k}: el filtro total es el top-k exacto"
        );
    }
}

/// AC-0048-04 — fronteras: filtro vacío => vacío sin panics; `k = 0` => vacío;
/// `k > N` => acotado a `N`.
#[test]
fn test_ac_0048_04_empty_filter() {
    let (_dir, mut database) = open_test_db("ac48_04");
    create_items(&mut database, "items");
    let entries: Vec<(i64, Vec<f64>)> = (0..8).map(|id| (id, vec![id as f64, 0.0])).collect();
    insert_entries(&mut database, &entries);

    // Filtro que no selecciona nada => vacío, sin panics.
    let empty = database
        .execute("SELECT * FROM items WHERE id > 100 KNN embedding <|3|> [0.0, 0.0]")
        .expect("filtro vacío");
    assert!(empty.is_empty());

    // Filtro vacío con k = 0 => vacío.
    let empty_zero = database
        .execute("SELECT * FROM items WHERE id > 100 KNN embedding <|0|> [0.0, 0.0]")
        .expect("filtro vacío k=0");
    assert!(empty_zero.is_empty());

    // k = 0 con filtro total => vacío.
    let zero = database
        .execute("SELECT * FROM items WHERE id >= 0 KNN embedding <|0|> [0.0, 0.0]")
        .expect("k=0");
    assert!(zero.is_empty());

    // k > N con filtro total => N filas.
    let big = database
        .execute("SELECT * FROM items WHERE id >= 0 KNN embedding <|99|> [0.0, 0.0]")
        .expect("k>N");
    assert_eq!(big.len(), entries.len());
}

/// AC-0048-05 — el `KNN` sin `WHERE` conserva el comportamiento previo.
#[test]
fn test_ac_0048_05_knn_without_filter_unchanged() {
    let (_dir, mut database) = open_test_db("ac48_05");
    create_items(&mut database, "items");
    let entries: Vec<(i64, Vec<f64>)> = (0..6)
        .map(|id| (id, vec![id as f64, (id % 2) as f64]))
        .collect();
    insert_entries(&mut database, &entries);

    let query = vec![0.25, 0.0];
    let query_clause = format!("[{}, {}]", query[0], query[1]);
    // Sin WHERE: comportamiento previo (top-k exacto del índice).
    let all = database
        .execute(&format!(
            "SELECT * FROM items KNN embedding <|6|> {query_clause}"
        ))
        .expect("KNN sin filtro");
    let got: Vec<i64> = all.iter().map(int_of).collect();
    assert_eq!(got, brute_force_filtered(&entries, &query, i64::MIN, 6));

    let top = database
        .execute(&format!(
            "SELECT * FROM items KNN embedding <|2|> {query_clause}"
        ))
        .expect("KNN sin filtro k=2");
    let top_ids: Vec<i64> = top.iter().map(int_of).collect();
    assert_eq!(top_ids, brute_force_filtered(&entries, &query, i64::MIN, 2));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// PBT: el `KNN` con `WHERE` (filtros moderados/selectivos) coincide con la
    /// fuerza bruta filtrada para corpus pequeños donde iFVS es exacto.
    #[test]
    fn prop_knn_with_filter_matches_bruteforce(
        raw_vectors in prop::collection::vec(prop::collection::vec(0u32..1000, 2), 4..20),
        raw_query in prop::collection::vec(0u32..1000, 2),
        cutoff_percent in 40u32..100,
        k in 0usize..8,
    ) {
        let vectors: Vec<Vec<f64>> = raw_vectors
            .iter()
            .map(|vector| vector.iter().map(|value| f64::from(*value) / 2.0).collect())
            .collect();
        let query: Vec<f64> = raw_query
            .iter()
            .map(|value| f64::from(*value) / 2.0)
            .collect();

        // Se exige distancias distintas: con empates el orden lo resuelve HNSW
        // de forma no determinista frente al oráculo.
        let mut distances: Vec<f32> = vectors.iter().map(|vector| l2(vector, &query)).collect();
        distances.sort_by(|a, b| a.total_cmp(b));
        prop_assume!(distances.windows(2).all(|pair| pair[0] != pair[1]));

        let count = vectors.len();
        // cutoff_percent >= 40 => s = |allowed| / count <= 0.6 (moderado/selectivo).
        let cutoff = ((count as u64 * u64::from(cutoff_percent)) / 100) as i64;
        let entries: Vec<(i64, Vec<f64>)> = vectors
            .iter()
            .enumerate()
            .map(|(index, vector)| (index as i64, vector.clone()))
            .collect();

        let (_dir, mut database) = open_test_db("ac48_prop");
        create_items(&mut database, "items");
        insert_entries(&mut database, &entries);

        let sql = format!(
            "SELECT * FROM items WHERE id >= {cutoff} KNN embedding <|{k}|> [{}, {}]",
            query[0], query[1]
        );
        let rows = database.execute(&sql).expect("KNN+WHERE");
        let got: Vec<i64> = rows.iter().map(int_of).collect();
        let expected = brute_force_filtered(&entries, &query, cutoff, k);
        prop_assert_eq!(got, expected, "cutoff={} k={}", cutoff, k);
    }
}
