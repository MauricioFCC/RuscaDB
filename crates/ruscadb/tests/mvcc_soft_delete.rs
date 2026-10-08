//! Integración end-to-end del borrado lógico MVCC (SPEC-0022).
//!
//! Cubre los criterios AC-0022-01..05 y una propiedad PBT: borrar una fila
//! nunca la deja en los resultados ni corrompe los índices.

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
    let path = dir.path().join(format!("{tag}.data"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `t(a INT, b TEXT)`.
fn create_t(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![
                ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "b".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Inserta una fila `(a, b)` en `t` (auto-commit) y devuelve su id.
fn insert_row(database: &mut Database, a: i64, b: &str) -> RecordId {
    let scalars = ScalarMap::from([
        ("a".to_string(), ScalarValue::Int(a)),
        ("b".to_string(), ScalarValue::Text(b.to_string())),
    ]);
    database.insert("t", scalars).expect("insert")
}

/// Extrae el valor entero de una columna `Int` de una fila.
fn int_of(row: &ruscadb::Row, column: &str) -> i64 {
    match row.get(column) {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en '{column}', se obtuvo {other:?}"),
    }
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

/// AC-0022-01 — tras el borrado la fila desaparece de `SELECT *` y del conteo.
#[test]
fn test_ac_0022_01_delete_hides_row() {
    let (_dir, mut database) = open_test_db("ac22_01");
    create_t(&mut database);
    let first = insert_row(&mut database, 1, "x");
    let _second = insert_row(&mut database, 2, "x");
    let _third = insert_row(&mut database, 3, "y");

    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        3
    );

    let deleted = database.delete("t", &first).expect("delete");
    assert!(deleted, "borrar una fila viva devuelve true");

    let rows = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 2, "la fila borrada sale del conteo");
    assert!(
        !rows.iter().any(|row| int_of(row, "a") == 1),
        "la fila borrada no aparece en SELECT *"
    );
}

/// AC-0022-02 — el borrado (auto-commit) sobrevive a cerrar y reabrir.
#[test]
fn test_ac_0022_02_delete_survives_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac22_02.data");
    let deleted_id;
    {
        let mut database = Database::open(DbConfig::new(&path, 128)).expect("open");
        create_t(&mut database);
        deleted_id = insert_row(&mut database, 7, "borrame");
        insert_row(&mut database, 8, "vive");
        assert!(database.delete("t", &deleted_id).expect("delete"));
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, 128)).expect("reapertura");
    let rows = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(rows.len(), 1, "solo sobrevive la fila viva");
    assert_eq!(int_of(&rows[0], "a"), 8);
    // El id borrado sigue ausente: borrarlo de nuevo es no-op.
    assert!(
        !database
            .delete("t", &deleted_id)
            .expect("delete idempotente"),
        "un id ya borrado no vuelve a borrarse"
    );
}

/// AC-0022-03 — un snapshot anterior al borrado sigue viendo la fila (MVCC).
#[test]
fn test_ac_0022_03_snapshot_before_delete_still_sees_row() {
    let (_dir, mut database) = open_test_db("ac22_03");
    create_t(&mut database);
    let target = insert_row(&mut database, 42, "visible");

    let before = database.snapshot();
    assert!(database.delete("t", &target).expect("delete"));

    let old = database
        .execute_at("SELECT * FROM t", &before)
        .expect("snapshot anterior");
    assert_eq!(old.len(), 1, "el snapshot anterior al borrado ve la fila");
    assert_eq!(int_of(&old[0], "a"), 42);

    let current = database
        .execute("SELECT * FROM t")
        .expect("snapshot actual");
    assert!(current.is_empty(), "el snapshot actual ya no la ve");
}

/// AC-0022-04 — borrar un id ausente (o repetido) es no-op idempotente.
#[test]
fn test_ac_0022_04_delete_missing_is_noop() {
    let (_dir, mut database) = open_test_db("ac22_04");
    create_t(&mut database);

    let ghost = RecordId::new();
    assert!(
        !database.delete("t", &ghost).expect("delete ausente"),
        "un id ausente devuelve false sin pánico"
    );

    // Doble borrado: la segunda vez es no-op.
    let only = insert_row(&mut database, 1, "x");
    assert!(database.delete("t", &only).expect("primer delete"));
    assert!(
        !database.delete("t", &only).expect("segundo delete"),
        "borrar dos veces la misma fila es idempotente"
    );

    // Borrar la última fila deja la tabla vacía.
    let last = insert_row(&mut database, 2, "y");
    assert!(database.delete("t", &last).expect("delete última"));
    assert!(
        database
            .execute("SELECT * FROM t")
            .expect("select")
            .is_empty()
    );

    // Tabla inexistente: error accionable, no pánico.
    let missing = database.delete("ausente", &RecordId::new());
    assert!(
        matches!(missing, Err(RuscaError::TableNotFound { ref table }) if table == "ausente"),
        "se esperaba TableNotFound, se obtuvo {missing:?}"
    );
}

/// AC-0022-05 — la fila borrada no aparece en KNN, MATCH ni TRAVERSE.
#[test]
fn test_ac_0022_05_deleted_row_excluded_from_indexes() {
    let (_dir, mut database) = open_test_db("ac22_05");
    database
        .create_table(
            "items",
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

    let root = RecordId::new();
    let child = RecordId::new();
    let sibling = RecordId::new();
    let records = [
        make_record(
            root,
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(1)),
                ("name".to_string(), ScalarValue::Text("gato".to_string())),
            ]),
            Some(vec![0.0, 0.0, 0.0]),
            vec![edge_to(child)],
        ),
        make_record(
            child,
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(2)),
                ("name".to_string(), ScalarValue::Text("perro".to_string())),
            ]),
            Some(vec![9.0, 9.0, 9.0]),
            Vec::new(),
        ),
        make_record(
            sibling,
            ScalarMap::from([
                ("id".to_string(), ScalarValue::Int(3)),
                (
                    "name".to_string(),
                    ScalarValue::Text("gato gato".to_string()),
                ),
            ]),
            Some(vec![1.0, 1.0, 1.0]),
            Vec::new(),
        ),
    ];
    for record in records {
        database
            .insert_record("items", record)
            .expect("insert_record");
    }

    // Precondición: el nodo raíz aparece por los tres índices.
    assert!(
        database
            .execute("SELECT * FROM items WHERE MATCH(name, 'gato')")
            .expect("MATCH previo")
            .iter()
            .any(|row| int_of(row, "id") == 1)
    );

    assert!(
        database.delete("items", &root).expect("delete"),
        "el nodo raíz existe"
    );

    let matched = database
        .execute("SELECT * FROM items WHERE MATCH(name, 'gato')")
        .expect("MATCH");
    assert!(
        !matched.iter().any(|row| int_of(row, "id") == 1),
        "la fila borrada no aparece en MATCH"
    );
    assert_eq!(matched.len(), 1, "el otro documento 'gato gato' sigue vivo");

    let nearest = database
        .execute("SELECT * FROM items KNN embedding <|10|> [0.0, 0.0, 0.0]")
        .expect("KNN");
    assert!(
        !nearest.iter().any(|row| int_of(row, "id") == 1),
        "la fila borrada no aparece en KNN"
    );
    assert_eq!(nearest.len(), 2, "los otros dos vectores siguen indexados");

    let reachable = database
        .execute("SELECT * FROM items TRAVERSE edges DEPTH 2")
        .expect("TRAVERSE");
    assert!(
        !reachable.iter().any(|row| int_of(row, "id") == 1),
        "la fila borrada no aparece en TRAVERSE"
    );
    assert_eq!(
        reachable.len(),
        1,
        "el hijo sigue siendo alcanzable como nodo vivo"
    );
    assert_eq!(int_of(&reachable[0], "id"), 2);
}

/// AC-0022-05 (coherencia fina) — el índice secundario no conserva entradas de
/// la fila borrada (se comprueba por la API de búsqueda del índice).
#[test]
fn test_ac_0022_06_secondary_index_entry_removed() {
    let (_dir, mut database) = open_test_db("ac22_06");
    create_t(&mut database);
    database.create_index("t", "a").expect("create_index");
    let victim = insert_row(&mut database, 10, "x");
    insert_row(&mut database, 20, "y");

    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a = 10")
            .expect("lookup antes")
            .len(),
        1,
        "la fila viva está indexada antes del borrado"
    );

    assert!(database.delete("t", &victim).expect("delete"));

    assert!(
        database
            .execute("SELECT * FROM t WHERE a = 10")
            .expect("lookup víctima")
            .is_empty(),
        "la entrada del índice secundario de la fila borrada debe eliminarse"
    );
    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a = 20")
            .expect("lookup superviviente")
            .len(),
        1,
        "la entrada de la fila viva permanece"
    );
}

/// AC-0022-05 (coherencia fina) — con claves duplicadas solo se retira la
/// entrada de la fila borrada, no la de la fila viva con la misma clave.
#[test]
fn test_ac_0022_07_secondary_index_duplicate_keys() {
    let (_dir, mut database) = open_test_db("ac22_07");
    create_t(&mut database);
    database.create_index("t", "a").expect("create_index");
    let first = insert_row(&mut database, 10, "x");
    insert_row(&mut database, 10, "y");

    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a = 10")
            .expect("lookup antes")
            .len(),
        2
    );

    assert!(database.delete("t", &first).expect("delete"));

    assert_eq!(
        database
            .execute("SELECT * FROM t WHERE a = 10")
            .expect("lookup después")
            .len(),
        1,
        "el duplicado vivo conserva su entrada en el índice"
    );
    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        1
    );
}

/// AC-0022-05 (coherencia fina) — borrar una fila con índice secundario y valor
/// `NULL` en la columna indexada es un no-op seguro (no hay entrada que quitar).
#[test]
fn test_ac_0022_08_secondary_index_null_value_is_noop() {
    let (_dir, mut database) = open_test_db("ac22_08");
    create_t(&mut database);
    database.create_index("t", "a").expect("create_index");
    let scalars = ScalarMap::from([
        ("a".to_string(), ScalarValue::Null),
        ("b".to_string(), ScalarValue::Text("x".to_string())),
    ]);
    let id = database.insert("t", scalars).expect("insert con NULL");

    assert!(
        database.delete("t", &id).expect("delete con índice y NULL"),
        "la fila existía y debe borrarse"
    );
    assert!(
        database
            .execute("SELECT * FROM t")
            .expect("select")
            .is_empty()
    );
}

proptest! {
    /// PBT: borrar una fila nunca la deja en `SELECT *` ni en el índice, y el
    /// resto de filas permanece consultable (el índice no se corrompe).
    #[test]
    fn prop_deleted_row_never_reappears(
        values in prop::collection::vec(0i64..1_000, 2..24),
        victim in 0usize..24,
    ) {
        let (_dir, mut database) = open_test_db("ac22_prop");
        create_t(&mut database);
        database.create_index("t", "a").expect("create_index");

        let mut ids = Vec::new();
        for (position, value) in values.iter().enumerate() {
            // El índice secundario indexa `a`; valores únicos para localizar la víctima.
            let id = insert_row(&mut database, *value * 1_000 + position as i64, "x");
            ids.push(id);
        }
        let index = victim % ids.len();
        let victim_id = ids[index];
        let victim_value = values[index] * 1_000 + index as i64;

        let deleted = database.delete("t", &victim_id).expect("delete");
        prop_assert!(deleted);

        let rows = database.execute("SELECT * FROM t").expect("select");
        prop_assert_eq!(rows.len(), values.len() - 1);
        prop_assert!(!rows.iter().any(|row| int_of(row, "a") == victim_value));

        // El índice sigue siendo coherente: buscar la clave borrada devuelve
        // vacío y buscar una clave viva la recupera.
        let gone = database
            .execute(&format!("SELECT * FROM t WHERE a = {victim_value}"))
            .expect("lookup borrado");
        prop_assert!(gone.is_empty());

        let survivor = (index + 1) % ids.len();
        let survivor_value = values[survivor] * 1_000 + survivor as i64;
        let found = database
            .execute(&format!("SELECT * FROM t WHERE a = {survivor_value}"))
            .expect("lookup vivo");
        prop_assert_eq!(found.len(), 1);

        // Borrar de nuevo es no-op (idempotencia).
        prop_assert!(!database.delete("t", &victim_id).expect("doble delete"));
    }
}
