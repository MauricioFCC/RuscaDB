//! Suite de aceptación de la compactación de índices derivados (SPEC-0101).
//!
//! Cubre AC-0101-01..04: tras `Database::reap` los índices derivados (HNSW
//! vectorial, CSR de grafo e invertido/FTS) se reconstruyen desde las filas
//! vivas del heap, de modo que las filas purgadas no dejan entradas huérfanas;
//! la compactación es idempotente y no altera los resultados de las filas vivas.
//!
//! El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
//! del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, Edge, EdgeSet, Embedding, EmbeddingMeta, Metric,
    Record, RecordId, RecordMeta, Row, ScalarMap, ScalarValue,
};

/// Capacidad del buffer pool de los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Abre una base temporal de pruebas con pool amplio.
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
    (dir, database)
}

/// Crea la tabla `nodes(id INT, name TEXT)` (texto indexable por FTS).
///
/// Args:
///     database: Base abierta.
///     table: Nombre de la tabla.
fn create_nodes(database: &mut Database, table: &str) {
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

/// Construye un embedding L2 de dimensión 3.
///
/// Args:
///     values: Componentes del vector.
///
/// Returns:
///     El embedding listo para un registro.
fn embedding(values: [f64; 3]) -> Embedding {
    let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
    Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "test".to_string(),
            dim: 3,
            metric: Metric::L2,
        },
    )
    .expect("embedding válido")
}

/// Construye un nodo con escalares `(id, name)`, vector y aristas salientes.
///
/// Args:
///     id: Identificador estable del registro.
///     number: Valor de la columna `id`.
///     name: Texto indexado por FTS.
///     vector: Embedding del nodo.
///     edges: Destinos de las aristas salientes.
///
/// Returns:
///     El registro listo para `insert_record`.
fn node(id: RecordId, number: i64, name: &str, vector: [f64; 3], edges: Vec<RecordId>) -> Record {
    let out: Vec<Edge> = edges
        .into_iter()
        .map(|target| Edge {
            label: "link".to_string(),
            node: target,
        })
        .collect();
    Record {
        id,
        scalars: ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(number)),
            ("name".to_string(), ScalarValue::Text(name.to_string())),
        ]),
        doc: None,
        edges: EdgeSet {
            out,
            incoming: Vec::new(),
        },
        vector: Some(embedding(vector)),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Extrae el entero de la columna `id` de una fila.
///
/// Args:
///     row: Fila devuelta por `execute`.
///
/// Returns:
///     El valor entero de la columna `id`.
fn id_of(row: &Row) -> i64 {
    match row.get("id") {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en 'id', se obtuvo {other:?}"),
    }
}

/// Ids escalares de un conjunto de filas, en su orden.
///
/// Args:
///     rows: Filas devueltas por `execute`.
///
/// Returns:
///     La lista de valores de la columna `id`.
fn ids(rows: &[Row]) -> Vec<i64> {
    rows.iter().map(id_of).collect()
}

/// Siembra `nodes` con cuatro filas: `a -> b`, `b -> d`, `c` aislada.
///
/// Todas llevan vector y texto; `d` (perro) es la fila que el test borra. La
/// arista `b -> d` es la que queda huérfana tras purgar `d`.
///
/// Args:
///     database: Base abierta.
///     table: Nombre de la tabla.
///
/// Returns:
///     Los `RecordId` de `a`, `b`, `c` y `d`, en ese orden.
fn seed_graph(database: &mut Database, table: &str) -> (RecordId, RecordId, RecordId, RecordId) {
    let a = RecordId::new();
    let b = RecordId::new();
    let c = RecordId::new();
    let d = RecordId::new();
    let records = [
        node(a, 1, "gato alfa", [0.0, 0.0, 0.0], vec![b]),
        node(b, 2, "gato beta", [0.1, 0.1, 0.1], vec![d]),
        node(c, 3, "gato gamma", [9.0, 9.0, 9.0], Vec::new()),
        node(d, 4, "perro delta", [5.0, 5.0, 5.0], Vec::new()),
    ];
    for record in records {
        database
            .insert_record(table, record)
            .expect("insert_record");
    }
    (a, b, c, d)
}

/// Vista observable de la tabla: `SELECT`, `KNN`, `MATCH` y `TRAVERSE` (ids).
///
/// Args:
///     database: Base abierta.
///     table: Nombre de la tabla.
///
/// Returns:
///     La tupla `(select, knn, match, traverse)` con los ids de cada consulta.
fn snapshot_results(
    database: &mut Database,
    table: &str,
) -> (Vec<i64>, Vec<i64>, Vec<i64>, Vec<i64>) {
    let select = ids(&database
        .execute(&format!("SELECT * FROM {table}"))
        .expect("SELECT"));
    let knn = ids(&database
        .execute(&format!(
            "SELECT * FROM {table} KNN embedding <|10|> [0.0, 0.0, 0.0]"
        ))
        .expect("KNN"));
    let matched = ids(&database
        .execute(&format!("SELECT * FROM {table} WHERE MATCH(name, 'gato')"))
        .expect("MATCH"));
    let traversed = ids(&database
        .execute(&format!("SELECT * FROM {table} TRAVERSE edges DEPTH 2"))
        .expect("TRAVERSE"));
    (select, knn, matched, traversed)
}

/// AC-0101-01 — `reap()` reconstruye los índices derivados desde filas vivas.
///
/// Inserta filas vectoriales/texto con aristas, borra `d` (destino de la arista
/// `b -> d`) y ejecuta `reap()`: el HNSW, el CSR y el invertido dejan de tener
/// rastros de la fila purgada y KNN/MATCH/TRAVERSE devuelven solo lo vivo.
// @spec AC-0101-01
#[test]
fn test_ac_0101_01_reap_rebuilds_derived_indexes_from_live_rows() {
    let (_dir, mut database) = open_test_db("ac101_01");
    create_nodes(&mut database, "nodes");
    let (_a, _b, _c, d) = seed_graph(&mut database, "nodes");

    // Estado inicial: cuatro vectores y dos aristas (a -> b, b -> d).
    assert_eq!(database.vector_index_len("nodes"), 4, "cuatro vectores");
    assert_eq!(database.graph_edge_count("nodes"), 2, "dos aristas");

    assert!(database.delete("nodes", &d).expect("delete de `d`"));

    // La vista de filas vivas ya excluye `d` por tombstone, pero el índice
    // HNSW todavía conserva su vector huérfano (el gap que cierra SPEC-0101).
    let before = snapshot_results(&mut database, "nodes");
    assert!(
        !before.0.contains(&4)
            && !before.1.contains(&4)
            && !before.2.contains(&4)
            && !before.3.contains(&4),
        "la fila borrada no aparece en ninguna consulta"
    );
    assert_eq!(
        database.vector_index_len("nodes"),
        4,
        "el HNSW aún retiene el vector de la fila borrada (pre-compactación)"
    );

    assert_eq!(database.reap().expect("reap"), 1, "se purga `d`");

    // Tras el rebuild, los conteos reflejan solo filas vivas.
    assert_eq!(
        database.vector_index_len("nodes"),
        3,
        "el vector huérfano de `d` se elimina del HNSW"
    );
    assert_eq!(
        database.graph_edge_count("nodes"),
        1,
        "la arista huérfana b -> d se elimina del CSR"
    );
    assert_eq!(
        database.fts_doc_count("nodes", "name"),
        3,
        "el índice invertido solo indexa filas vivas"
    );

    let after = snapshot_results(&mut database, "nodes");
    assert_eq!(
        after, before,
        "los resultados de las filas vivas no cambian tras compactar"
    );
    assert!(
        !after.0.contains(&4)
            && !after.1.contains(&4)
            && !after.2.contains(&4)
            && !after.3.contains(&4),
        "la fila purgada no reaparece en ninguna consulta"
    );
}

/// AC-0101-02 — `compact_derived_indexes` es idempotente.
///
/// Tras `reap()`, invocar la compactación dos veces no cambia los resultados de
/// SELECT/KNN/MATCH/TRAVERSE ni los conteos, y un `reap()` posterior es no-op.
// @spec AC-0101-02
#[test]
fn test_ac_0101_02_compaction_is_idempotent() {
    let (_dir, mut database) = open_test_db("ac101_02");
    create_nodes(&mut database, "nodes");
    let (_a, _b, _c, d) = seed_graph(&mut database, "nodes");
    assert!(database.delete("nodes", &d).expect("delete de `d`"));
    assert_eq!(database.reap().expect("reap"), 1);

    let first = snapshot_results(&mut database, "nodes");
    let vectors = database.vector_index_len("nodes");
    let edges = database.graph_edge_count("nodes");
    let docs = database.fts_doc_count("nodes", "name");

    database.compact_derived_indexes().expect("compact #1");
    database.compact_derived_indexes().expect("compact #2");

    assert_eq!(
        snapshot_results(&mut database, "nodes"),
        first,
        "compactar de nuevo no cambia los resultados"
    );
    assert_eq!(
        database.vector_index_len("nodes"),
        vectors,
        "vectores estables"
    );
    assert_eq!(
        database.graph_edge_count("nodes"),
        edges,
        "aristas estables"
    );
    assert_eq!(
        database.fts_doc_count("nodes", "name"),
        docs,
        "postings estables"
    );
    assert_eq!(database.reap().expect("reap repetido"), 0, "reap es no-op");
}

/// AC-0101-03 — `reap()` sin filas borradas no altera los resultados.
///
/// Con filas vivas y sin borrados, `reap()` devuelve 0 y las consultas y los
/// conteos quedan idénticos (no hay nada que compactar).
// @spec AC-0101-03
#[test]
fn test_ac_0101_03_reap_without_deletes_preserves_results() {
    let (_dir, mut database) = open_test_db("ac101_03");
    create_nodes(&mut database, "nodes");
    seed_graph(&mut database, "nodes");

    let before = snapshot_results(&mut database, "nodes");
    let vectors = database.vector_index_len("nodes");
    let edges = database.graph_edge_count("nodes");

    assert_eq!(database.reap().expect("reap sin borrados"), 0);

    assert_eq!(
        snapshot_results(&mut database, "nodes"),
        before,
        "sin purga, los resultados no cambian"
    );
    assert_eq!(database.vector_index_len("nodes"), vectors);
    assert_eq!(database.graph_edge_count("nodes"), edges);
}

/// AC-0101-04 — compactar sin borrados preserva los resultados (cero regresión).
///
/// Sobre filas vivas (vector/texto/aristas), `compact_derived_indexes` deja las
/// consultas y los conteos idénticos: el rebuild no pierde ni altera nada vivo.
// @spec AC-0101-04
#[test]
fn test_ac_0101_04_compaction_preserves_live_results() {
    let (_dir, mut database) = open_test_db("ac101_04");
    create_nodes(&mut database, "nodes");
    seed_graph(&mut database, "nodes");

    let before = snapshot_results(&mut database, "nodes");
    assert_eq!(database.vector_index_len("nodes"), 4);

    database
        .compact_derived_indexes()
        .expect("compact sin borrados");

    assert_eq!(
        snapshot_results(&mut database, "nodes"),
        before,
        "cero regresión: los resultados vivos son idénticos"
    );
    assert_eq!(
        database.vector_index_len("nodes"),
        4,
        "sin borrados no se pierde ningún vector"
    );
    assert_eq!(
        database.graph_edge_count("nodes"),
        2,
        "las aristas vivas se conservan"
    );
}
