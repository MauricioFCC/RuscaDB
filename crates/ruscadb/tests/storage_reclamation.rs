//! Suite de aceptación de la reclamación de storage (SPEC-0085).
//!
//! Cubre los criterios AC-0085-01..03: reutilización de páginas liberadas por
//! `reap()`, persistencia de la free-list a través de la reapertura y GC de los
//! índices derivados (grafo/FTS) sin alterar los resultados de las filas vivas.
//!
//! El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
//! del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, Edge, EdgeSet, Record, RecordId, RecordMeta,
    ScalarMap, ScalarValue,
};

/// Tamaño del payload de las filas de reclamación.
///
/// Se elige para que **cada fila ocupe una página entera** (dos de estas filas
/// no caben en 4 KiB): así borrar una fila vacía su página, que el `reap()`
/// puede liberar, y la actualización in-place del borrado tiene hueco de sobra.
const PAYLOAD_BYTES: usize = 2000;

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
    let database = Database::open(DbConfig::new(&path, 256)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `t(a INT, payload TEXT)` (una fila por página con el payload).
///
/// Args:
///     database: Base abierta.
///     table: Nombre de la tabla.
fn create_payload_table(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
            vec![
                ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "payload".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Inserta `count` filas grandes (`a = 0..count`) y devuelve sus identificadores.
///
/// Args:
///     database: Base abierta.
///     table: Nombre de la tabla.
///     count: Número de filas a insertar.
///
/// Returns:
///     Los `RecordId` de las filas insertadas, en orden.
fn seed_payload_rows(database: &mut Database, table: &str, count: usize) -> Vec<RecordId> {
    let payload = "x".repeat(PAYLOAD_BYTES);
    (0..count)
        .map(|value| {
            let scalars = ScalarMap::from([
                ("a".to_string(), ScalarValue::Int(value as i64)),
                ("payload".to_string(), ScalarValue::Text(payload.clone())),
            ]);
            database.insert(table, scalars).expect("insert")
        })
        .collect()
}

/// Crea la tabla `nodes(id INT, name TEXT)` para el GC de derivados.
///
/// Args:
///     database: Base abierta.
///     table: Nombre de la tabla.
fn create_nodes_table(database: &mut Database, table: &str) {
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

/// Construye un registro `nodes` con aristas salientes.
///
/// Args:
///     id: Identificador estable del registro.
///     number: Valor de la columna `id`.
///     name: Texto indexado por FTS.
///     edges: Aristas salientes del nodo.
///
/// Returns:
///     El `Record` listo para `insert_record`.
fn node_record(id: RecordId, number: i64, name: &str, edges: Vec<RecordId>) -> Record {
    let out: Vec<Edge> = edges
        .into_iter()
        .map(|node| Edge {
            label: "link".to_string(),
            node,
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
        vector: None,
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Extrae el texto de la columna `name` de una fila.
///
/// Args:
///     row: Fila devuelta por un `SELECT`.
///
/// Returns:
///     El valor textual de `name`.
fn name_of(row: &ruscadb::Row) -> String {
    match row.get("name") {
        Some(ScalarValue::Text(value)) => value.clone(),
        other => panic!("se esperaba Text en 'name', se obtuvo {other:?}"),
    }
}

/// Extrae los nombres de un conjunto de filas, en su orden.
///
/// Args:
///     rows: Filas devueltas por el executor.
///
/// Returns:
///     La lista de valores de la columna `name`.
fn names(rows: &[ruscadb::Row]) -> Vec<String> {
    rows.iter().map(name_of).collect()
}

/// AC-0085-01 — `allocate` reutiliza las páginas liberadas por `reap()`.
///
/// Inserta filas (una por página), las borra todas, `reap()` las reclama y
/// vuelve a insertar el mismo volumen: el alto agua (`next_page`) no supera el
/// pico previo porque las páginas libres se reutilizan.
// @spec AC-0085-01
#[test]
fn test_ac_0085_01_allocate_reuses_freed_pages() {
    let (_dir, mut database) = open_test_db("ac85_01");
    create_payload_table(&mut database, "t");
    let ids = seed_payload_rows(&mut database, "t", 60);
    let peak = database.catalog().expect("catálogo").next_page();

    for id in &ids {
        assert!(database.delete("t", id).expect("delete"));
    }
    assert_eq!(database.reap().expect("reap"), 60);
    assert!(
        database.free_page_count() > 0,
        "el reap debe devolver páginas a la free-list"
    );

    // Reinserción del mismo volumen: reutiliza las páginas libres.
    seed_payload_rows(&mut database, "t", 60);
    assert!(
        database.catalog().expect("catálogo").next_page() <= peak,
        "el allocator no debe crecer respecto del pico previo (free-list reutilizada)"
    );
    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        60,
        "todas las filas reinseridas se leen"
    );
}

/// AC-0085-02 — la free-list sobrevive a cerrar y reabrir la base.
///
/// Tras liberar páginas con `reap()`, el cierre persiste la free-list en el
/// manifiesto; al reabrir, `free_page_count` se conserva, las filas vivas se
/// leen intactas y una reinserción vuelve a reutilizar sin crecer el fichero.
// @spec AC-0085-02
#[test]
fn test_ac_0085_02_freelist_survives_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac85_02.db");
    let (free_before, peak, live);
    {
        let mut database = Database::open(DbConfig::new(&path, 256)).expect("apertura");
        create_payload_table(&mut database, "t");
        seed_payload_rows(&mut database, "t", 40); // filas vivas
        let doomed = seed_payload_rows(&mut database, "t", 40); // grupo a borrar
        for id in &doomed {
            assert!(database.delete("t", id).expect("delete"));
        }
        assert_eq!(database.reap().expect("reap"), 40);

        free_before = database.free_page_count();
        peak = database.catalog().expect("catálogo").next_page();
        live = database.execute("SELECT * FROM t").expect("select").len();
        assert!(
            free_before > 0,
            "el reap debe liberar páginas del grupo borrado"
        );
        assert_eq!(live, 40, "las filas vivas no se tocan");
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, 256)).expect("reapertura");
    assert_eq!(
        database.free_page_count(),
        free_before,
        "la free-list persiste en el manifiesto y se recarga al reabrir"
    );
    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        40,
        "las filas vivas se leen intactas tras reabrir"
    );

    seed_payload_rows(&mut database, "t", 40);
    assert!(
        database.catalog().expect("catálogo").next_page() <= peak,
        "tras reabrir, la reinserción reutiliza la free-list persistida"
    );
}

/// AC-0085-03 — el GC de derivados compacta huérfanos sin perder filas vivas.
///
/// Inserta nodos con aristas y texto, borra un nodo hoja, ejecuta `reap()` y
/// verifica que: (1) las aristas/postings huérfanos se eliminan (los conteos
/// bajan) y (2) `TRAVERSE`/`MATCH` devuelven exactamente lo mismo que antes del
/// GC para las filas vivas.
// @spec AC-0085-03
#[test]
fn test_ac_0085_03_derived_indexes_compact_without_losing_live_results() {
    let (_dir, mut database) = open_test_db("ac85_03");
    create_nodes_table(&mut database, "nodes");

    let first = RecordId::new();
    let second = RecordId::new();
    let third = RecordId::new();
    let leaf = RecordId::new();
    let records = [
        node_record(first, 1, "gato alfa", vec![second, third]),
        node_record(second, 2, "gato beta", vec![leaf]),
        node_record(third, 3, "gato gamma", Vec::new()),
        node_record(leaf, 4, "gato delta", Vec::new()),
    ];
    for record in records {
        database
            .insert_record("nodes", record)
            .expect("insert_record");
    }

    let postings_before = database.fts_posting_count("nodes", "name");
    assert!(database.delete("nodes", &leaf).expect("delete de la hoja"));

    // Vista de las filas vivas ANTES del GC (ya sin la hoja por tombstone).
    let traverse_before = names(
        &database
            .execute("SELECT * FROM nodes TRAVERSE edges DEPTH 2")
            .expect("traverse antes del GC"),
    );
    let match_before = names(
        &database
            .execute("SELECT * FROM nodes WHERE MATCH(name, 'gato')")
            .expect("match antes del GC"),
    );
    let edges_before_gc = database.graph_edge_count("nodes");

    assert_eq!(database.reap().expect("reap"), 1, "la hoja se purga");

    let traverse_after = names(
        &database
            .execute("SELECT * FROM nodes TRAVERSE edges DEPTH 2")
            .expect("traverse tras el GC"),
    );
    let match_after = names(
        &database
            .execute("SELECT * FROM nodes WHERE MATCH(name, 'gato')")
            .expect("match tras el GC"),
    );

    assert_eq!(
        traverse_after, traverse_before,
        "TRAVERSE de filas vivas no cambia tras el GC"
    );
    assert_eq!(
        match_after, match_before,
        "MATCH de filas vivas no cambia tras el GC"
    );
    assert!(
        database.graph_edge_count("nodes") < edges_before_gc,
        "la arista huérfana de la hoja se elimina del CSR"
    );
    assert!(
        database.fts_posting_count("nodes", "name") < postings_before,
        "los postings huérfanos de la hoja se eliminan del índice invertido"
    );
}
