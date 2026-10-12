//! Tests de aceptación de integración del reranker late-interaction real
//! (SPEC-0072, ColBERT-style) sobre la fachada `ruscadb`.
//!
//! Cubre únicamente los criterios observables desde la API pública:
//!
//! - **AC-0072-03** — `hybrid_search` re-puntúa con MaxSim: el stage final es
//!   `MaxSim` y los scores son **continuos** (no el conteo entero de overlap).
//! - **AC-0072-05** — la degradación `Bm25Only`/`KnnOnly` es idéntica a la
//!   anterior (cero regresión) y no invoca MaxSim.
//!
//! Los AC-0072-01/02/04 (cobertura, repetición y determinismo de `maxsim`) son
//! unit tests internos porque `maxsim`/`token_embedding` son `pub(crate)` y no
//! forman parte de la superficie pública; los escribe el equipo en `hybrid.rs`.
//!
//! AC-0072-03 está **RED por diseño**: la implementación vigente sirve el proxy
//! de overlap (`overlap_terms`), cuyos scores son el conteo **entero** de
//! términos compartidos. Hasta que `rescore_or_keep` proyecte los tokens a
//! vectores densos y calcule `Σ_i max_j cos(q_i, d_j)`, la aserción de score
//! continuo falla (los scores son `3.0`, `2.0`, `1.0`).

// El oráculo de los tests puede fallar ruidosamente (equivalente a
// `allow-expect-in-tests` del workspace, que no alcanza a targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, HybridHit,
    HybridStage, HybridWeights, Metric, Record, RecordId, RecordMeta, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Dimensión de los vectores de prueba (coincide con las queries KNN).
const VECTOR_DIM: usize = 3;

/// Vector de consulta vectorial: cercano a los documentos SIN términos de la
/// consulta y lejano a los que SÍ los cubren (fuerza que RRF y cobertura
/// discrepen, condición de AC-0072-03).
const QUERY_VECTOR: [f32; VECTOR_DIM] = [0.1, 0.2, 0.3];

/// Consulta textual de AC-0072-03 (3 términos: `alfa`, `beta`, `gamma`).
const QUERY_TEXT: &str = "alfa beta gamma";

/// Número de términos de `QUERY_TEXT`: cota superior de los scores MaxSim.
const QUERY_TOKEN_COUNT: f32 = 3.0;

/// Número de hits solicitados en AC-0072-03.
const K_RERANK: usize = 3;

/// Número de hits solicitados en las consultas de degradación (AC-0072-05).
const K_DEGRADED: usize = 2;

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database =
        Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura de la base");
    (dir, database)
}

/// Crea `docs(id INT, titulo TEXT)`: columna textual indexable por FTS.
///
/// Args:
///     database: Base destino.
///     table: Nombre de la tabla a crear.
fn create_text_table(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "titulo".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Crea `items(id INT)`: sin columna TEXT indexada (solo vía vectorial/HNSW).
///
/// Args:
///     database: Base destino.
///     table: Nombre de la tabla a crear.
fn create_id_table(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
            vec![ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
}

/// Construye escalares `(id, titulo)`.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     titulo: Valor textual de la columna `titulo`.
///
/// Returns:
///     El mapa de escalares listo para un registro.
fn scalars_text(id: i64, titulo: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("titulo".to_string(), ScalarValue::Text(titulo.to_string())),
    ])
}

/// Construye escalares `(id)` para tablas sin columna textual.
///
/// Args:
///     id: Valor entero de la columna `id`.
///
/// Returns:
///     El mapa de escalares listo para un registro.
fn scalars_id(id: i64) -> ScalarMap {
    ScalarMap::from([("id".to_string(), ScalarValue::Int(id))])
}

/// Crea un embedding L2 de la dimensión del vector.
///
/// Args:
///     values: Componentes del vector en `f64`.
///
/// Returns:
///     El embedding L2 construido.
fn embedding(values: &[f64]) -> Embedding {
    let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
    Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "test".to_string(),
            dim: values.len(),
            metric: Metric::L2,
        },
    )
    .expect("embedding válido")
}

/// Construye un `Record` con escalares y vector (sin aristas).
///
/// Args:
///     id: Identificador del registro.
///     scalars: Escalares del registro.
///     vector: Componentes del embedding.
///
/// Returns:
///     El registro listo para `insert_record`.
fn vector_record(id: RecordId, scalars: ScalarMap, vector: &[f64]) -> Record {
    Record {
        id,
        scalars,
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(embedding(vector)),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Resuelve el `id` escalar sembrado a partir del `RecordId` de un hit.
///
/// Args:
///     ids: Mapeo `(RecordId, id_escalar)` capturado al sembrar la tabla.
///     id: `RecordId` devuelto por `hybrid_search`.
///
/// Returns:
///     El `id` escalar correspondiente.
fn scalar_id_of(ids: &[(RecordId, i64)], id: RecordId) -> i64 {
    ids.iter()
        .find(|(record_id, _)| *record_id == id)
        .map(|(_, scalar_id)| *scalar_id)
        .unwrap_or_else(|| panic!("RecordId {id} ausente del mapeo sembrado"))
}

/// Determina si un score es un entero exacto.
///
/// El proxy de overlap vigente produce conteos enteros de términos; los scores
/// MaxSim continuos (cosenos sumados) caen, en general, fuera de los enteros.
///
/// Args:
///     score: Score de un hit híbrido.
///
/// Returns:
///     `true` si `score` no tiene parte fraccionaria.
fn is_integer_score(score: f32) -> bool {
    score.is_finite() && (score - score.trunc()).abs() < f32::EPSILON
}

/// AC-0072-03 — `hybrid_search` re-puntúa con MaxSim (stage y scores continuos).
///
/// Siembra 4 filas donde el orden por RRF **difiere** del orden por cobertura
/// de términos de la consulta:
///
/// - `id 1` cubre los 3 términos pero lleva el vector **más lejano**.
/// - `id 2` cubre 2 términos y lleva el vector **más cercano** (gana en RRF).
/// - `id 3` cubre 1 término con vector intermedio.
/// - `id 4` no cubre ninguno y solo aparece en la vía vectorial.
///
/// El seed se elige para que el top-3 de RRF sea `[id 2, id 1, id 3]` mientras
/// que el orden por cobertura es `[id 1, id 2, id 3]`; así el re-scoring
/// **reordena** y el stage debe etiquetarse `MaxSim`. El reranker real (hash
/// embeddings + `Σ_i max_j cos`) produce scores continuos en `[0, 3]`; el proxy
/// de overlap produce `3.0`, `2.0`, `1.0` → esta aserción es RED hoy.
#[test]
// @spec AC-0072-03
fn test_ac_0072_03_hybrid_uses_maxsim_scores() {
    let (_dir, mut database) = open_test_db("ac0072_03");
    create_text_table(&mut database, "docs");

    // id 1: 3 términos, vector LEJANO. id 2: 2 términos, vector CERCANO.
    // id 3: 1 término, vector intermedio. id 4: 0 términos, solo vía vectorial.
    let data: [(i64, &str, [f64; VECTOR_DIM]); 4] = [
        (1, QUERY_TEXT, [9.0, 9.0, 9.0]),
        (2, "alfa beta", [0.1, 0.2, 0.3]),
        (3, "alfa", [7.0, 7.0, 7.0]),
        (4, "zzz", [8.0, 8.0, 8.0]),
    ];
    let mut ids: Vec<(RecordId, i64)> = Vec::new();
    for (scalar_id, titulo, vector) in data {
        let record = vector_record(RecordId::new(), scalars_text(scalar_id, titulo), &vector);
        let record_id = database
            .insert_record("docs", record)
            .expect("insert_record");
        ids.push((record_id, scalar_id));
    }

    // When: búsqueda híbrida con ambas señales y un vector cercano a los
    // documentos SIN términos (RRF y cobertura discrepan).
    let hits: Vec<HybridHit> = database
        .hybrid_search(
            "docs",
            Some(QUERY_TEXT),
            Some(&QUERY_VECTOR[..]),
            K_RERANK,
            HybridWeights::default(),
        )
        .expect("hybrid_search");

    // Then (1): el re-scoring late-interaction reordena y etiqueta MaxSim.
    assert_eq!(hits.len(), K_RERANK, "devuelve los k hits solicitados");
    assert_eq!(
        hits[0].stage,
        HybridStage::MaxSim,
        "el re-scoring MaxSim debe reordenar el top-k"
    );
    assert_eq!(
        scalar_id_of(&ids, hits[0].id),
        1,
        "el documento que cubre los 3 términos pasa primero tras MaxSim"
    );

    // Then (2): los scores MaxSim son CONTINUOS (cosenos sumados en [0, n]),
    // no el conteo entero de términos del proxy de overlap. El límite superior
    // admite el epsilon de la aritmética f32 (cosenos que suman 1±ulp).
    const FLOAT_EPSILON: f32 = 1e-4;
    assert!(
        hits.iter()
            .all(|hit| (-FLOAT_EPSILON..=QUERY_TOKEN_COUNT + FLOAT_EPSILON).contains(&hit.score)),
        "los scores MaxSim viven en [0, n_tokens] (± epsilon f32): {:?}",
        hits.iter().map(|hit| hit.score).collect::<Vec<_>>()
    );
    assert!(
        hits.iter().any(|hit| !is_integer_score(hit.score)),
        "al menos un score MaxSim debe ser no entero (continuo): {:?}",
        hits.iter().map(|hit| hit.score).collect::<Vec<_>>()
    );
}

/// AC-0072-05(a) — tabla solo-FTS degrada a BM25 puro sin invocar MaxSim.
///
/// Con `vector_query` presente pero sin índice vectorial, el stage de todos
/// los hits debe ser `Bm25Only` (cero regresión respecto a SPEC-0063).
#[test]
// @spec AC-0072-05
fn test_ac_0072_05_degradation_unchanged() {
    let (_dir, mut database) = open_test_db("ac0072_05a");
    create_text_table(&mut database, "docs");

    // Registros SIN vector: la vía vectorial no puede existir.
    let documents = [
        (1_i64, "el gato duerme en el sofa"),
        (2, "el perro corre en el parque"),
        (3, "gato gato gato"),
        (4, "una casa azul"),
    ];
    let mut texts: Vec<(RecordId, i64)> = Vec::new();
    for (scalar_id, titulo) in documents {
        let record_id = database
            .insert("docs", scalars_text(scalar_id, titulo))
            .expect("insert");
        texts.push((record_id, scalar_id));
    }
    assert_eq!(texts.len(), 4, "se sembraron los 4 documentos de texto");

    // When: híbrida con vector_query sobre una tabla sin índice vectorial.
    let hits: Vec<HybridHit> = database
        .hybrid_search(
            "docs",
            Some("gato"),
            Some(&QUERY_VECTOR[..]),
            K_DEGRADED,
            HybridWeights::default(),
        )
        .expect("la ausencia de la vía vectorial no debe ser un error");

    // Then: degrada a BM25 puro (sin MaxSim).
    assert!(!hits.is_empty(), "BM25 devuelve al menos un hit");
    assert!(
        hits.iter().all(|hit| hit.stage == HybridStage::Bm25Only),
        "todos los hits deben etiquetarse como Bm25Only: {:?}",
        hits.iter().map(|hit| hit.stage).collect::<Vec<_>>()
    );
}

/// AC-0072-05(b) — tabla sin columna FTS degrada a KNN puro sin invocar MaxSim.
///
/// Con `text_query` presente pero sin columna TEXT indexada, el stage de todos
/// los hits debe ser `KnnOnly` (cero regresión respecto a SPEC-0063).
#[test]
// @spec AC-0072-05
fn test_ac_0072_05_b_no_fts_degrades_to_knn() {
    let (_dir, mut database) = open_test_db("ac0072_05b");
    create_id_table(&mut database, "items");

    let data: [(i64, [f64; VECTOR_DIM]); 4] = [
        (1, [0.0, 0.0, 0.0]),
        (2, [0.1, 0.2, 0.3]),
        (3, [9.0, 9.0, 9.0]),
        (4, [3.0, 3.0, 3.0]),
    ];
    let mut ids: Vec<(RecordId, i64)> = Vec::new();
    for (scalar_id, vector) in data {
        let record = vector_record(RecordId::new(), scalars_id(scalar_id), &vector);
        let record_id = database
            .insert_record("items", record)
            .expect("insert_record");
        ids.push((record_id, scalar_id));
    }
    assert_eq!(ids.len(), 4, "se sembraron los 4 items vectoriales");

    // When: híbrida con text_query sobre una tabla sin índice textual.
    let hits: Vec<HybridHit> = database
        .hybrid_search(
            "items",
            Some("gato"),
            Some(&QUERY_VECTOR[..]),
            K_DEGRADED,
            HybridWeights::default(),
        )
        .expect("la ausencia de la vía textual no debe ser un error");

    // Then: degrada a KNN puro (sin MaxSim).
    assert!(!hits.is_empty(), "KNN devuelve al menos un hit");
    assert!(
        hits.iter().all(|hit| hit.stage == HybridStage::KnnOnly),
        "todos los hits deben etiquetarse como KnnOnly: {:?}",
        hits.iter().map(|hit| hit.stage).collect::<Vec<_>>()
    );
}
