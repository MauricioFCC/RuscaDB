//! Tests de aceptación de **varios índices secundarios por tabla** (SPEC-0104).
//!
//! Cubre AC-0104-01 (varios índices y planner que usa el índice de la columna
//! pedida), AC-0104-02 (mantenimiento en INSERT/UPDATE/DELETE), AC-0104-03
//! (duplicar una columna falla ruidoso) y AC-0104-04 (persistencia del catálogo
//! multi-índice v3 tras reabrir). El códec y la compatibilidad con el formato
//! legacy v1/v2 (AC-0104-05..08) viven en las pruebas unitarias de `catalog.rs`.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarMap, ScalarValue};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 64;

/// Abre una base temporal de pruebas.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
    (dir, database)
}

/// Crea la tabla `t(a INT, b TEXT, c INT)`.
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
                ColumnDef {
                    name: "c".to_string(),
                    col_type: ColumnType::Int,
                },
            ],
        )
        .expect("create_table");
}

/// Escalares de una fila `(a, b, c)`.
fn scalars(a: i64, b: &str, c: i64) -> ScalarMap {
    ScalarMap::from([
        ("a".to_string(), ScalarValue::Int(a)),
        ("b".to_string(), ScalarValue::Text(b.to_string())),
        ("c".to_string(), ScalarValue::Int(c)),
    ])
}

/// Extrae la columna `a` de una fila como `i64`.
fn column_a(row: &Row) -> i64 {
    match row.get("a") {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en 'a', se obtuvo {other:?}"),
    }
}

/// AC-0104-01 — con dos índices el planner usa el de la columna pedida y las
/// consultas por cada columna devuelven lo correcto.
#[test]
// @spec AC-0104-01
fn test_ac_0104_01_multiple_indexes_are_used_per_column() {
    let (_dir, mut database) = open_test_db("ac0104_01");
    create_t(&mut database);
    for (a, b, c) in [(1, "uno", 10), (2, "dos", 20), (3, "tres", 30)] {
        database.insert("t", scalars(a, b, c)).expect("insert");
    }
    database.create_index("t", "a").expect("índice a");
    database.create_index("t", "b").expect("índice b");
    database.create_index("t", "c").expect("índice c");

    // Cada `WHERE` sobre una columna indexada usa su propio IndexScan.
    for (filter, column) in [("a = 2", "a"), ("b = 'dos'", "b"), ("c = 20", "c")] {
        let plan = database
            .explain(&format!("EXPLAIN SELECT * FROM t WHERE {filter}"))
            .expect("explain");
        assert!(
            plan.iter().any(|line| line.contains("IndexScan")),
            "el plan de '{filter}' debe usar IndexScan (columna {column}): {plan:?}"
        );
        let rows = database
            .execute(&format!("SELECT * FROM t WHERE {filter}"))
            .expect("select");
        assert_eq!(rows.len(), 1, "una sola coincidencia para {filter}");
        assert_eq!(column_a(&rows[0]), 2, "la fila correcta para {filter}");
    }

    // Un `WHERE` sin columna indexada sigue en FullScan (cero regresión).
    let _ = database
        .create_index("t", "c")
        .expect_err("duplicar 'c' debe fallar ruidoso");
    let plan = database
        .explain("EXPLAIN SELECT * FROM t")
        .expect("explain sin filtro");
    assert!(
        plan.iter().any(|line| line.contains("FullScan")),
        "sin filtro el plan sigue en FullScan: {plan:?}"
    );
}

/// AC-0104-02 — INSERT/UPDATE/DELETE mantienen todos los índices consistentes.
#[test]
// @spec AC-0104-02
fn test_ac_0104_02_dml_keeps_every_index_consistent() {
    let (_dir, mut database) = open_test_db("ac0104_02");
    create_t(&mut database);
    database.create_index("t", "a").expect("índice a");
    database.create_index("t", "c").expect("índice c");

    database.insert("t", scalars(1, "uno", 10)).expect("insert");
    database.insert("t", scalars(2, "dos", 20)).expect("insert");

    // UPDATE cambia ambas columnas indexadas.
    database
        .execute("UPDATE t SET a = 5, c = 50 WHERE a = 1")
        .expect("update");
    let old_a = database
        .execute("SELECT * FROM t WHERE a = 1")
        .expect("select a=1");
    assert!(old_a.is_empty(), "el valor viejo de 'a' sale del índice");
    let old_c = database
        .execute("SELECT * FROM t WHERE c = 10")
        .expect("select c=10");
    assert!(old_c.is_empty(), "el valor viejo de 'c' sale del índice");
    let new_pair = database
        .execute("SELECT * FROM t WHERE a = 5")
        .expect("select a=5");
    assert_eq!(new_pair.len(), 1, "el valor nuevo de 'a' está indexado");
    assert_eq!(
        new_pair[0].get("c"),
        Some(&ScalarValue::Int(50)),
        "la fila conserva 'c' actualizado"
    );

    // DELETE retira la entrada de ambos índices.
    database
        .execute("DELETE FROM t WHERE a = 5")
        .expect("delete");
    for filter in ["a = 5", "c = 50"] {
        let rows = database
            .execute(&format!("SELECT * FROM t WHERE {filter}"))
            .expect("select tras delete");
        assert!(
            rows.is_empty(),
            "'{filter}' no debe encontrar la fila borrada"
        );
    }
    let remaining = database.execute("SELECT * FROM t").expect("select total");
    assert_eq!(remaining.len(), 1, "solo queda la fila no borrada");
    assert_eq!(
        remaining[0].get("a"),
        Some(&ScalarValue::Int(2)),
        "la fila restante es la intacta"
    );
}

/// AC-0104-03 — duplicar la misma columna falla ruidoso; otra columna crea otro.
#[test]
// @spec AC-0104-03
fn test_ac_0104_03_duplicate_column_index_is_actionable() {
    let (_dir, mut database) = open_test_db("ac0104_03");
    create_t(&mut database);
    database.create_index("t", "a").expect("primer índice");

    let error = database
        .create_index("t", "a")
        .expect_err("duplicar la columna debe fallar");
    assert!(
        error.to_string().contains("ya tiene un índice"),
        "mensaje accionable: {error}"
    );
    assert!(
        error.to_string().contains("'a'"),
        "el mensaje nombra la columna: {error}"
    );

    // Otra columna sí crea un segundo índice (SPEC-0104).
    database.create_index("t", "c").expect("segundo índice");
    let catalog = database.catalog().expect("catálogo");
    let table = catalog.get("t").expect("tabla");
    assert_eq!(table.indexes.len(), 2, "dos índices en la tabla");
    assert_eq!(table.index_columns(), vec!["a", "c"]);
}

/// AC-0104-04 — el catálogo multi-índice (v3) persiste y responde tras reabrir.
#[test]
// @spec AC-0104-04
fn test_ac_0104_04_multiple_indexes_survive_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac0104_04.db");
    {
        let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
        create_t(&mut database);
        database.create_index("t", "a").expect("índice a");
        database.create_index("t", "b").expect("índice b");
        for (a, b, c) in [(1, "uno", 10), (2, "dos", 20)] {
            database.insert("t", scalars(a, b, c)).expect("insert");
        }
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("reapertura");
    let catalog = database.catalog().expect("catálogo");
    let table = catalog.get("t").expect("tabla");
    assert_eq!(
        table.index_columns(),
        vec!["a", "b"],
        "ambos índices deben recargarse del catálogo"
    );
    // Ambos siguen respondiendo por igualdad tras la reapertura.
    let rows = database
        .execute("SELECT * FROM t WHERE b = 'dos'")
        .expect("select por b");
    assert_eq!(rows.len(), 1, "el índice de 'b' sigue activo");
    assert_eq!(column_a(&rows[0]), 2);
}
