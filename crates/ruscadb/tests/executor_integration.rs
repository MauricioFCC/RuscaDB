//! Integración end-to-end de FTS/KNN/TRAVERSE en el executor (SPEC-0017).
//!
//! Cubre los criterios AC-0017-01..05 y las propiedades PBT (KNN frente a
//! fuerza bruta y `MATCH` recuperable por cualquier término).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, Edge, EdgeSet, Embedding, EmbeddingMeta, Metric,
    Record, RecordId, RecordMeta, RuscaError, ScalarMap, ScalarValue,
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
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura de la base");
    (dir, database)
}

/// Crea `items(id INT, name TEXT)`.
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

/// Construye escalares `(id, name)`.
fn scalars(id: i64, name: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ])
}

/// Crea un embedding L2 de la dimensión del vector.
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

/// Construye un `Record` completo (escalares + vector + aristas).
fn make_record(
    id: RecordId,
    scalars: ScalarMap,
    vector: Option<Vec<f64>>,
    edges: Vec<Edge>,
) -> Record {
    Record {
        id,
        scalars,
        doc: None,
        edges: EdgeSet {
            out: edges,
            incoming: Vec::new(),
        },
        vector: vector.as_deref().map(embedding),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Arista saliente hacia otro registro.
fn edge_to(node: RecordId) -> Edge {
    Edge {
        label: "link".to_string(),
        node,
    }
}

/// Extrae el valor entero de una columna `Int` de una fila.
fn int_of(row: &ruscadb::Row, column: &str) -> i64 {
    match row.get(column) {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en '{column}', se obtuvo {other:?}"),
    }
}

/// Extrae el valor de texto de una columna de una fila.
fn text_of(row: &ruscadb::Row, column: &str) -> String {
    match row.get(column) {
        Some(ScalarValue::Text(value)) => value.clone(),
        other => panic!("se esperaba Text en '{column}', se obtuvo {other:?}"),
    }
}

/// Distancia euclídea en `f32` (oráculo de fuerza bruta para los PBT de KNN).
fn l2(left: &[f64], right: &[f64]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (*a as f32 - *b as f32).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// AC-0017-01 — `MATCH` full-text end-to-end ordenado por BM25.
#[test]
fn test_ac_0017_01_match_full_text_end_to_end() {
    let (_dir, mut database) = open_test_db("ac17_01");
    database
        .create_table(
            "docs",
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
    let documents = [
        (1_i64, "el gato duerme en el sofa"),
        (2, "el perro corre en el parque"),
        (3, "gato gato gato"),
        (4, "una casa azul"),
    ];
    for (id, titulo) in documents {
        let row = ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("titulo".to_string(), ScalarValue::Text(titulo.to_string())),
        ]);
        database.insert("docs", row).expect("insert");
    }

    let rows = database
        .execute("SELECT * FROM docs WHERE MATCH(titulo, 'gato')")
        .expect("MATCH end-to-end");

    assert_eq!(rows.len(), 2, "solo los documentos con 'gato'");
    assert_eq!(int_of(&rows[0], "id"), 3, "BM25: el más denso primero");
    assert_eq!(int_of(&rows[1], "id"), 1);
    assert!(!rows.iter().any(|row| int_of(row, "id") == 2));

    // `MATCH` compuesto con un filtro escalar mediante `AND`.
    let composed = database
        .execute("SELECT * FROM docs WHERE id >= 1 AND MATCH(titulo, 'gato')")
        .expect("WHERE AND MATCH");
    assert_eq!(composed.len(), 2);
    assert_eq!(int_of(&composed[0], "id"), 3, "se conserva el orden BM25");
}

/// AC-0017-02 — `KNN` vectorial end-to-end por el índice HNSW.
#[test]
fn test_ac_0017_02_knn_vector_end_to_end() {
    let (_dir, mut database) = open_test_db("ac17_02");
    create_items(&mut database, "items");
    let data = [
        (1_i64, "origen", vec![0.0, 0.0, 0.0]),
        (2, "cerca", vec![0.1, 0.1, 0.1]),
        (3, "lejos", vec![9.0, 9.0, 9.0]),
        (4, "medio", vec![3.0, 3.0, 3.0]),
    ];
    for (id, name, vector) in data {
        let record = make_record(RecordId::new(), scalars(id, name), Some(vector), Vec::new());
        database
            .insert_record("items", record)
            .expect("insert_record");
    }

    let rows = database
        .execute("SELECT * FROM items KNN embedding <|2|> [0.1, 0.2, 0.3]")
        .expect("KNN end-to-end");

    assert_eq!(rows.len(), 2);
    assert_eq!(text_of(&rows[0], "name"), "cerca");
    assert_eq!(text_of(&rows[1], "name"), "origen");

    // BVA de `k`: `k = 0` no devuelve nada y `k > N` devuelve todos.
    let none = database
        .execute("SELECT * FROM items KNN embedding <|0|> [0.1, 0.2, 0.3]")
        .expect("KNN k=0");
    assert!(none.is_empty());
    let all = database
        .execute("SELECT * FROM items KNN embedding <|10|> [0.1, 0.2, 0.3]")
        .expect("KNN k>N");
    assert_eq!(all.len(), 4);
    assert_eq!(text_of(&all[0], "name"), "cerca");
}

/// AC-0017-03 — `TRAVERSE` de grafo end-to-end respetando `DEPTH`.
#[test]
fn test_ac_0017_03_traverse_graph_end_to_end() {
    let (_dir, mut database) = open_test_db("ac17_03");
    create_items(&mut database, "nodes");
    let first = RecordId::new();
    let second = RecordId::new();
    let third = RecordId::new();
    let isolated = RecordId::new();
    let records = [
        make_record(first, scalars(1, "a"), None, vec![edge_to(second)]),
        make_record(second, scalars(2, "b"), None, vec![edge_to(third)]),
        make_record(third, scalars(3, "c"), None, Vec::new()),
        make_record(isolated, scalars(4, "d"), None, Vec::new()),
    ];
    for (record, expected) in records.into_iter().zip([first, second, third, isolated]) {
        let stored = database
            .insert_record("nodes", record)
            .expect("insert_record");
        assert_eq!(
            stored, expected,
            "insert_record conserva el id del registro"
        );
    }

    let deep = database
        .execute("SELECT * FROM nodes TRAVERSE edges DEPTH 2")
        .expect("TRAVERSE end-to-end");
    let names: Vec<String> = deep.iter().map(|row| text_of(row, "name")).collect();
    assert_eq!(names, ["a", "b", "c"], "el nodo aislado 'd' queda fuera");

    let shallow = database
        .execute("SELECT * FROM nodes TRAVERSE edges DEPTH 1")
        .expect("TRAVERSE DEPTH 1");
    let shallow_names: Vec<String> = shallow.iter().map(|row| text_of(row, "name")).collect();
    assert_eq!(shallow_names, ["a", "b"], "DEPTH acota la profundidad");
}

/// AC-0017-04 — combinación de `WHERE` + `KNN`/`TRAVERSE`/`MATCH` + `LIMIT`.
#[test]
fn test_ac_0017_04_combined_clauses() {
    let (_dir, mut database) = open_test_db("ac17_04");
    create_items(&mut database, "items");
    let data = [
        (0_i64, "n0", vec![0.0, 0.0, 0.0]),
        (1, "n1", vec![1.0, 1.0, 1.0]),
        (2, "n2", vec![2.0, 2.0, 2.0]),
        (3, "n3", vec![9.0, 9.0, 9.0]),
    ];
    for (id, name, vector) in data {
        let record = make_record(RecordId::new(), scalars(id, name), Some(vector), Vec::new());
        database
            .insert_record("items", record)
            .expect("insert_record");
    }

    let rows = database
        .execute("SELECT name FROM items WHERE id > 0 KNN embedding <|2|> [0.0, 0.0, 0.0] LIMIT 1")
        .expect("WHERE + KNN + LIMIT");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        text_of(&rows[0], "name"),
        "n1",
        "el más cercano tras filtrar"
    );

    database
        .create_table(
            "docs",
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
    for (id, titulo) in [(1_i64, "gato"), (2, "gato gato gato")] {
        let row = ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("titulo".to_string(), ScalarValue::Text(titulo.to_string())),
        ]);
        database.insert("docs", row).expect("insert");
    }
    let matched = database
        .execute("SELECT id FROM docs WHERE MATCH(titulo, 'gato') LIMIT 1")
        .expect("MATCH + LIMIT");
    assert_eq!(matched.len(), 1);
    assert_eq!(
        int_of(&matched[0], "id"),
        2,
        "MATCH + LIMIT deja el top BM25"
    );
}

/// AC-0017-05 — errores accionables para KNN/TRAVERSE/MATCH sin datos.
#[test]
fn test_ac_0017_05_integration_errors_are_actionable() {
    let (_dir, mut database) = open_test_db("ac17_05");
    create_items(&mut database, "plain");
    database
        .insert("plain", scalars(1, "sin vector ni aristas"))
        .expect("insert");

    let missing_vector = database
        .execute("SELECT * FROM plain KNN embedding <|1|> [0.1]")
        .expect_err("KNN sin vectores");
    assert!(
        matches!(missing_vector, RuscaError::MissingVector { ref table, ref column }
            if table == "plain" && column == "embedding"),
        "se esperaba MissingVector, se obtuvo {missing_vector:?}"
    );
    assert!(missing_vector.to_string().contains("embedding"));

    let missing_graph = database
        .execute("SELECT * FROM plain TRAVERSE edges DEPTH 1")
        .expect_err("TRAVERSE sin aristas");
    assert!(
        matches!(missing_graph, RuscaError::MissingGraph { ref table, ref column }
            if table == "plain" && column == "edges"),
        "se esperaba MissingGraph, se obtuvo {missing_graph:?}"
    );

    let missing_text = database
        .execute("SELECT * FROM plain WHERE MATCH(id, 'x')")
        .expect_err("MATCH sobre columna no textual");
    assert!(
        matches!(missing_text, RuscaError::MissingTextColumn { ref table, ref column }
            if table == "plain" && column == "id"),
        "se esperaba MissingTextColumn, se obtuvo {missing_text:?}"
    );

    let missing_table = database
        .execute("SELECT * FROM ausente KNN embedding <|1|> [0.1]")
        .expect_err("tabla ausente");
    assert!(
        matches!(missing_table, RuscaError::TableNotFound { ref table } if table == "ausente"),
        "se esperaba TableNotFound, se obtuvo {missing_table:?}"
    );

    create_items(&mut database, "vectors");
    let record = make_record(
        RecordId::new(),
        scalars(1, "tres dimensiones"),
        Some(vec![0.1, 0.2, 0.3]),
        Vec::new(),
    );
    database
        .insert_record("vectors", record)
        .expect("insert_record");
    let mismatch = database
        .execute("SELECT * FROM vectors KNN embedding <|1|> [0.1, 0.2]")
        .expect_err("dimensión incompatible");
    assert!(
        matches!(mismatch, RuscaError::DimensionMismatch { .. }),
        "se esperaba DimensionMismatch, se obtuvo {mismatch:?}"
    );
}

/// FR-0017-02 — los índices se reconstruyen al reabrir la base (durabilidad).
#[test]
fn test_ac_0017_06_indexes_survive_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("reopen.db");
    let first = RecordId::new();
    let second = RecordId::new();
    {
        let mut database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
        create_items(&mut database, "mixed");
        let records = [
            make_record(
                first,
                scalars(1, "gato duerme"),
                Some(vec![0.0, 0.0, 0.0]),
                vec![edge_to(second)],
            ),
            make_record(
                second,
                scalars(2, "perro corre"),
                Some(vec![9.0, 9.0, 9.0]),
                Vec::new(),
            ),
        ];
        for record in records {
            database
                .insert_record("mixed", record)
                .expect("insert_record");
        }
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, 128)).expect("reapertura");
    let matched = database
        .execute("SELECT * FROM mixed WHERE MATCH(name, 'gato')")
        .expect("MATCH tras reopen");
    assert_eq!(matched.len(), 1);
    assert_eq!(int_of(&matched[0], "id"), 1);

    let nearest = database
        .execute("SELECT * FROM mixed KNN embedding <|1|> [0.0, 0.0, 0.0]")
        .expect("KNN tras reopen");
    assert_eq!(nearest.len(), 1);
    assert_eq!(int_of(&nearest[0], "id"), 1);

    let reachable = database
        .execute("SELECT * FROM mixed TRAVERSE edges DEPTH 1")
        .expect("TRAVERSE tras reopen");
    assert_eq!(reachable.len(), 2);
    assert_eq!(int_of(&reachable[0], "id"), 1);
}

proptest! {
    /// PBT: con `N` pequeño, los `k` vecinos del índice coinciden (por
    /// distancia) con la fuerza bruta.
    #[test]
    fn prop_knn_matches_bruteforce(
        raw_vectors in prop::collection::vec(prop::collection::vec(0u32..40, 3), 1..6),
        raw_query in prop::collection::vec(0u32..40, 3),
    ) {
        let vectors: Vec<Vec<f64>> = raw_vectors
            .iter()
            .map(|vector| vector.iter().map(|value| f64::from(*value) / 2.0).collect())
            .collect();
        let query: Vec<f64> = raw_query.iter().map(|value| f64::from(*value) / 2.0).collect();

        let (_dir, mut database) = open_test_db("prop_knn");
        create_items(&mut database, "items");
        for (index, vector) in vectors.iter().enumerate() {
            let record = make_record(
                RecordId::new(),
                scalars(index as i64, &format!("v{index}")),
                Some(vector.clone()),
                Vec::new(),
            );
            database.insert_record("items", record).expect("insert_record");
        }

        let query_clause = format!(
            "SELECT * FROM items KNN embedding <|{}|> [{}, {}, {}]",
            vectors.len(),
            query[0],
            query[1],
            query[2]
        );
        let rows = database.execute(&query_clause).expect("KNN");
        prop_assert_eq!(rows.len(), vectors.len());

        let mut expected: Vec<(usize, f32)> = vectors
            .iter()
            .enumerate()
            .map(|(index, vector)| (index, l2(vector, &query)))
            .collect();
        expected.sort_by(|left, right| left.1.total_cmp(&right.1));
        let got: Vec<f32> = rows
            .iter()
            .map(|row| l2(&vectors[int_of(row, "id") as usize], &query))
            .collect();
        prop_assert_eq!(got.len(), expected.len());
        for (observed, (_, reference)) in got.iter().zip(expected.iter()) {
            prop_assert!(
                (observed - reference).abs() < 1e-4,
                "distancia {observed} != oráculo {reference}"
            );
        }
    }

    /// PBT: un documento indexado se recupera por cualquiera de sus términos.
    #[test]
    fn prop_match_recovers_any_term(words in prop::collection::vec("[a-z]{1,5}", 1..5)) {
        let document = words.join(" ");
        let (_dir, mut database) = open_test_db("prop_match");
        database
            .create_table(
                "docs",
                vec![
                    ColumnDef { name: "id".to_string(), col_type: ColumnType::Int },
                    ColumnDef { name: "titulo".to_string(), col_type: ColumnType::Text },
                ],
            )
            .expect("create_table");
        let row = ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(7)),
            ("titulo".to_string(), ScalarValue::Text(document.clone())),
        ]);
        database.insert("docs", row).expect("insert");

        for term in ruscadb_fts::tokenize(&document) {
            let query = format!("SELECT * FROM docs WHERE MATCH(titulo, '{term}')");
            let rows = database.execute(&query).expect("MATCH");
            prop_assert!(
                rows.iter().any(|row| int_of(row, "id") == 7),
                "el término {term:?} no recupera el documento"
            );
        }
    }
}
