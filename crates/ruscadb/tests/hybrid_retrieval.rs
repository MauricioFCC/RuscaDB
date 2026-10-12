//! Tests de aceptación de la búsqueda híbrida RRF + re-scoring MaxSim (SPEC-0063).
//!
//! Cubre los criterios AC-0063-01..05: priorización de la intersección
//! texto∩vector por fusión RRF, degradación graceful a BM25/KNN puros, reorden
//! determinista por un oráculo manual de overlap (late interaction) y la
//! aditividad de la API frente a `execute`.
//!
//! Estos tests son RED por diseño: la API de SPEC-0063 aún no existe en la
//! fachada `ruscadb`, por lo que la compilación falla al resolver los símbolos
//! `Database::hybrid_search`, `HybridWeights`, `HybridHit` y `HybridStage`.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, HybridHit,
    HybridStage, HybridWeights, Metric, Record, RecordId, RecordMeta, Row, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Dimensión de los vectores de prueba (coincide con las queries KNN).
const VECTOR_DIM: usize = 3;

/// Número de vecinos solicitado en las consultas KNN de comparación.
const KNN_K: usize = 2;

/// Vector de consulta de las pruebas vectoriales.
const QUERY_VECTOR: [f32; VECTOR_DIM] = [0.1, 0.2, 0.3];

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

/// Extrae el valor entero de una columna `Int` de una fila.
///
/// Args:
///     row: Fila devuelta por `execute`.
///     column: Nombre de la columna entera.
///
/// Returns:
///     El valor entero de la columna.
fn int_of(row: &Row, column: &str) -> i64 {
    match row.get(column) {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en '{column}', se obtuvo {other:?}"),
    }
}

/// Extrae el valor textual de una columna de una fila.
///
/// Args:
///     row: Fila devuelta por `execute`.
///     column: Nombre de la columna textual.
///
/// Returns:
///     El valor textual de la columna.
fn text_of(row: &Row, column: &str) -> String {
    match row.get(column) {
        Some(ScalarValue::Text(value)) => value.clone(),
        other => panic!("se esperaba Text en '{column}', se obtuvo {other:?}"),
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

/// Resuelve el texto sembrado a partir del `RecordId` de un hit.
///
/// Args:
///     texts: Mapeo `(RecordId, texto)` capturado al sembrar la tabla.
///     id: `RecordId` devuelto por `hybrid_search`.
///
/// Returns:
///     El texto asociado al registro.
fn text_of_record(texts: &[(RecordId, String)], id: RecordId) -> String {
    texts
        .iter()
        .find(|(record_id, _)| *record_id == id)
        .map(|(_, text)| text.clone())
        .unwrap_or_else(|| panic!("RecordId {id} ausente del mapeo sembrado"))
}

/// Tokeniza en minúsculas separando por espacios (oráculo del test).
///
/// Args:
///     text: Texto a tokenizar.
///
/// Returns:
///     La lista de términos en minúsculas.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Cuenta cuántos términos de la consulta aparecen en el documento (oráculo AC-04).
///
/// Args:
///     query_tokens: Términos de la consulta de texto.
///     text: Texto del registro candidato.
///
/// Returns:
///     El número de términos de la consulta presentes en el documento.
fn overlap(query_tokens: &[String], text: &str) -> usize {
    let document = tokenize(text);
    query_tokens
        .iter()
        .filter(|term| document.contains(term))
        .count()
}

/// AC-0063-01 — la fusión RRF prioriza la intersección texto∩vector.
///
/// Un subconjunto de ids aparece en ambas top-listas (BM25 y KNN); el hit
/// presente en las dos vías debe quedar por encima de los hits de una sola.
#[test]
// @spec AC-0063-01
fn test_ac_0063_01_rrf_prioritizes_overlap() {
    let (_dir, mut database) = open_test_db("ac0063_01");
    create_text_table(&mut database, "docs");

    // id 1: en AMBAS listas (texto denso + vector más cercano).
    // id 2: solo texto. id 3: solo vector. id 4: en ninguna.
    let data: [(i64, &str, [f64; VECTOR_DIM]); 4] = [
        (1, "gato gato gato", [0.0, 0.0, 0.0]),
        (2, "gato", [9.0, 9.0, 9.0]),
        (3, "perro", [0.5, 0.5, 0.5]),
        (4, "perro", [8.0, 8.0, 8.0]),
    ];
    let mut ids: Vec<(RecordId, i64)> = Vec::new();
    for (scalar_id, titulo, vector) in data {
        let record = vector_record(RecordId::new(), scalars_text(scalar_id, titulo), &vector);
        let record_id = database
            .insert_record("docs", record)
            .expect("insert_record");
        ids.push((record_id, scalar_id));
    }

    // Given: id 1 encabeza tanto la lista BM25 como la lista KNN.
    let text_rows = database
        .execute("SELECT * FROM docs WHERE MATCH(titulo, 'gato')")
        .expect("MATCH");
    let text_ids: Vec<i64> = text_rows.iter().map(|row| int_of(row, "id")).collect();
    assert_eq!(text_ids[0], 1, "id 1 encabeza la lista de texto (BM25)");

    let knn_rows = database
        .execute("SELECT * FROM docs KNN embedding <|2|> [0.1, 0.2, 0.3]")
        .expect("KNN");
    let knn_ids: Vec<i64> = knn_rows.iter().map(|row| int_of(row, "id")).collect();
    assert_eq!(knn_ids[0], 1, "id 1 encabeza la lista vectorial (KNN)");

    // When: búsqueda híbrida con ambas señales.
    let hits: Vec<HybridHit> = database
        .hybrid_search(
            "docs",
            Some("gato"),
            Some(&QUERY_VECTOR[..]),
            2,
            HybridWeights::default(),
        )
        .expect("hybrid_search");

    // Then: la intersección queda por encima de los hits de una sola vía.
    assert_eq!(hits.len(), 2, "devuelve los k hits solicitados");
    assert_eq!(
        scalar_id_of(&ids, hits[0].id),
        1,
        "la intersección texto∩vector va primero"
    );
    assert_eq!(
        hits[0].stage,
        HybridStage::Rrf,
        "la etapa servida es la fusión RRF"
    );
    assert!(
        hits[0].score > hits[1].score,
        "el score fusionado del overlap supera al de una sola vía"
    );
}

/// AC-0063-02 — una tabla solo-FTS degrada a BM25 puro sin error.
///
/// Con `vector_query` presente pero sin índice vectorial, el orden de ids debe
/// ser idéntico al de `SELECT ... WHERE MATCH(...)`.
#[test]
// @spec AC-0063-02
fn test_ac_0063_02_text_only_degrades_to_bm25() {
    let (_dir, mut database) = open_test_db("ac0063_02");
    create_text_table(&mut database, "docs");

    let documents = [
        (1_i64, "el gato duerme en el sofa"),
        (2, "el perro corre en el parque"),
        (3, "gato gato gato"),
        (4, "una casa azul"),
    ];
    let mut ids: Vec<(RecordId, i64)> = Vec::new();
    for (scalar_id, titulo) in documents {
        let record_id = database
            .insert("docs", scalars_text(scalar_id, titulo))
            .expect("insert");
        ids.push((record_id, scalar_id));
    }

    // Oráculo: el orden BM25 de la consulta de texto equivalente.
    let expected = database
        .execute("SELECT * FROM docs WHERE MATCH(titulo, 'gato')")
        .expect("MATCH");
    let expected_ids: Vec<i64> = expected.iter().map(|row| int_of(row, "id")).collect();
    assert_eq!(expected_ids, [3, 1], "BM25: el documento más denso primero");

    // When: consulta híbrida con vector_query no vacío sobre una tabla sin vectores.
    let hits: Vec<HybridHit> = database
        .hybrid_search(
            "docs",
            Some("gato"),
            Some(&QUERY_VECTOR[..]),
            4,
            HybridWeights::default(),
        )
        .expect("la ausencia de la vía vectorial no debe ser un error");

    // Then: degrada a BM25 puro, con el mismo orden y la etapa Bm25Only.
    let got_ids: Vec<i64> = hits.iter().map(|hit| scalar_id_of(&ids, hit.id)).collect();
    assert_eq!(got_ids, expected_ids, "degrada al orden BM25 puro");
    assert!(
        hits.iter().all(|hit| hit.stage == HybridStage::Bm25Only),
        "todos los hits deben etiquetarse como Bm25Only"
    );
}

/// AC-0063-03 — una tabla solo-HNSW degrada a KNN puro sin error.
///
/// Con `text_query` presente pero sin columna TEXT indexada, el orden de ids
/// debe ser idéntico al del KNN clásico sobre el mismo vector.
#[test]
// @spec AC-0063-03
fn test_ac_0063_03_vector_only_degrades_to_knn() {
    let (_dir, mut database) = open_test_db("ac0063_03");
    create_id_table(&mut database, "items");

    let data: [(i64, [f64; VECTOR_DIM]); 4] = [
        (1, [0.0, 0.0, 0.0]),
        (2, [0.1, 0.1, 0.1]),
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

    // Oráculo: el orden del KNN clásico sobre el mismo vector de consulta.
    let expected = database
        .execute("SELECT * FROM items KNN embedding <|2|> [0.1, 0.2, 0.3]")
        .expect("KNN");
    let expected_ids: Vec<i64> = expected.iter().map(|row| int_of(row, "id")).collect();
    assert_eq!(expected_ids, [2, 1], "KNN clásico: el más cercano primero");

    // When: consulta híbrida con text_query sobre una tabla sin índice de texto.
    let hits: Vec<HybridHit> = database
        .hybrid_search(
            "items",
            Some("gato"),
            Some(&QUERY_VECTOR[..]),
            2,
            HybridWeights::default(),
        )
        .expect("la ausencia de la vía textual no debe ser un error");

    // Then: degrada a KNN puro, con el mismo orden y la etapa KnnOnly.
    let got_ids: Vec<i64> = hits.iter().map(|hit| scalar_id_of(&ids, hit.id)).collect();
    assert_eq!(got_ids, expected_ids, "degrada al orden KNN puro");
    assert!(
        hits.iter().all(|hit| hit.stage == HybridStage::KnnOnly),
        "todos los hits deben etiquetarse como KnnOnly"
    );
}

/// AC-0063-04 — el re-scoring MaxSim reordena según el oráculo de overlap.
///
/// Sobre el top de la fusión RRF se aplica un oráculo manual determinista de
/// relevancia latente (overlap de tokens query↔documento). El orden final debe
/// seguir ese oráculo (2 reordena sobre 3 y 1) y ser reproducible.
#[test]
// @spec AC-0063-04
fn test_ac_0063_04_maxsim_rescore_reorders_candidates() {
    let (_dir, mut database) = open_test_db("ac0063_04");
    create_text_table(&mut database, "docs");

    // El vector más cercano NO es el más relevante por texto: RRF y el oráculo
    // de overlap discrepan, de modo que el re-scoring es observable.
    let data: [(i64, &str, [f64; VECTOR_DIM]); 3] = [
        (1, "alfa", [0.0, 0.0, 0.0]),
        (2, "alfa beta gamma", [9.0, 9.0, 9.0]),
        (3, "alfa beta", [5.0, 5.0, 5.0]),
    ];
    let mut ids: Vec<(RecordId, i64)> = Vec::new();
    let mut texts: Vec<(RecordId, String)> = Vec::new();
    for (scalar_id, titulo, vector) in data {
        let record = vector_record(RecordId::new(), scalars_text(scalar_id, titulo), &vector);
        let record_id = database
            .insert_record("docs", record)
            .expect("insert_record");
        ids.push((record_id, scalar_id));
        texts.push((record_id, titulo.to_string()));
    }

    let query_text = "alfa beta gamma";
    let query_vector: [f32; VECTOR_DIM] = [0.0, 0.0, 0.0];
    let hits: Vec<HybridHit> = database
        .hybrid_search(
            "docs",
            Some(query_text),
            Some(&query_vector[..]),
            3,
            HybridWeights::default(),
        )
        .expect("hybrid_search");

    // Given: el top-k_rerank contiene los tres candidatos.
    assert_eq!(hits.len(), 3, "el top reúne a todos los candidatos");

    // Then: la etapa final es MaxSim (late interaction).
    assert_eq!(
        hits[0].stage,
        HybridStage::MaxSim,
        "el top-k se re-puntúa con MaxSim"
    );

    // Oráculo del test: ordenar por overlap(query_tokens, texto) descendente.
    let query_tokens = tokenize(query_text);
    let mut oracle: Vec<(i64, usize)> = hits
        .iter()
        .map(|hit| {
            let text = text_of_record(&texts, hit.id);
            (scalar_id_of(&ids, hit.id), overlap(&query_tokens, &text))
        })
        .collect();
    oracle.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    let oracle_ids: Vec<i64> = oracle.iter().map(|(scalar_id, _)| *scalar_id).collect();

    let got_ids: Vec<i64> = hits.iter().map(|hit| scalar_id_of(&ids, hit.id)).collect();
    assert_eq!(
        got_ids, oracle_ids,
        "el orden final sigue el oráculo de overlap (relevancia latente)"
    );
    assert_eq!(
        got_ids,
        [2, 3, 1],
        "overlap 3 > 2 > 1 reordena los candidatos respecto a RRF"
    );
}

/// AC-0063-05 — la API híbrida es aditiva y no regresiona `execute`.
///
/// Con la API híbrida presente (tipos re-exportados) pero sin invocar
/// `hybrid_search`, las queries RQL existentes (`SELECT`+`MATCH` y
/// `SELECT`+`KNN`) devuelven exactamente los mismos resultados.
#[test]
// @spec AC-0063-05
fn test_ac_0063_05_api_is_additive_no_execute_regression() {
    let (_dir, mut database) = open_test_db("ac0063_05");
    create_text_table(&mut database, "mixed");

    let data: [(i64, &str, [f64; VECTOR_DIM]); 3] = [
        (1, "gato duerme", [0.0, 0.0, 0.0]),
        (2, "gato gato gato", [9.0, 9.0, 9.0]),
        (3, "perro corre", [0.1, 0.1, 0.1]),
    ];
    for (scalar_id, titulo, vector) in data {
        let record = vector_record(RecordId::new(), scalars_text(scalar_id, titulo), &vector);
        database
            .insert_record("mixed", record)
            .expect("insert_record");
    }

    let match_query = "SELECT * FROM mixed WHERE MATCH(titulo, 'gato')";
    let knn_query = format!("SELECT * FROM mixed KNN embedding <|{KNN_K}|> [0.1, 0.2, 0.3]");

    // Estado "antes": resultados canónicos de execute (sin ninguna llamada híbrida).
    let match_before = database.execute(match_query).expect("MATCH antes");
    let knn_before = database.execute(&knn_query).expect("KNN antes");

    // La API híbrida está presente (tipos re-exportados) pero NO se invoca.
    let weights = HybridWeights::default();
    assert_eq!(
        (weights.text, weights.vector),
        (1.0, 1.0),
        "los pesos híbridos por defecto son iguales"
    );

    // Estado "después": la mera presencia de la API no altera `execute`.
    let match_after = database.execute(match_query).expect("MATCH después");
    let knn_after = database.execute(&knn_query).expect("KNN después");

    assert_eq!(
        match_after, match_before,
        "SELECT + MATCH no regresiona con la API híbrida presente"
    );
    assert_eq!(
        knn_after, knn_before,
        "SELECT + KNN no regresiona con la API híbrida presente"
    );

    let matched_ids: Vec<i64> = match_before.iter().map(|row| int_of(row, "id")).collect();
    assert_eq!(matched_ids, [2, 1], "BM25 conserva su orden canónico");

    let nearest_names: Vec<String> = knn_before
        .iter()
        .map(|row| text_of(row, "titulo"))
        .collect();
    assert_eq!(
        nearest_names,
        ["perro corre", "gato duerme"],
        "KNN conserva su orden canónico"
    );
}
