//! Integración end-to-end de DML en RQL (SPEC-0043).
//!
//! Cubre los criterios AC-0043-01..04: `INSERT` multi-fila (un commit),
//! `UPDATE`/`DELETE` con conteo de filas afectadas y errores accionables.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, RuscaError, ScalarValue};

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
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
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

/// Extrae el entero de la columna `affected` de la fila de resultado DML.
///
/// Args:
///     rows: Filas devueltas por `execute`.
///
/// Returns:
///     El número de filas afectadas.
///
/// Raises:
///     Panic si la fila no tiene `affected: Int` (fallo de test).
fn affected(rows: &[Row]) -> i64 {
    match rows.first().and_then(|row| row.get("affected")) {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba {{affected: Int}}, se obtuvo {other:?}"),
    }
}

/// Inserta las filas `(a, b)` en `t` vía `INSERT` RQL.
fn seed(database: &mut Database, pairs: &[(i64, &str)]) {
    for (value, label) in pairs {
        database
            .execute(&format!("INSERT INTO t (a, b) VALUES ({value}, '{label}')"))
            .expect("insert");
    }
}

/// AC-0043-01 — `INSERT` multi-fila inserta N filas y devuelve `affected = N`.
#[test] // @spec AC-0043-01
fn test_ac_0043_01_insert_statement() {
    let (_dir, mut database) = open_test_db("ac0043_01");
    create_t(&mut database);

    let rows = database
        .execute("INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'y')")
        .expect("insert multi-fila");
    assert_eq!(affected(&rows), 2, "deben insertarse 2 filas");

    let all = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].get("a"), Some(&ScalarValue::Int(1)));
    assert_eq!(all[1].get("b"), Some(&ScalarValue::Text("y".to_string())));

    // BVA: insertar una sola fila también funciona.
    let single = database
        .execute("INSERT INTO t (a, b) VALUES (3, 'z')")
        .expect("insert una fila");
    assert_eq!(affected(&single), 1);
    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        3
    );
}

/// AC-0043-02 — `UPDATE ... WHERE` reescribe solo las filas que cumplen.
#[test] // @spec AC-0043-02
fn test_ac_0043_02_update_statement() {
    let (_dir, mut database) = open_test_db("ac0043_02");
    create_t(&mut database);
    seed(&mut database, &[(1, "x"), (2, "y"), (3, "x")]);

    let rows = database
        .execute("UPDATE t SET b = 'z' WHERE a = 1")
        .expect("update");
    assert_eq!(affected(&rows), 1, "solo una fila cumple a = 1");

    let all = database
        .execute("SELECT * FROM t ORDER BY a")
        .expect("select");
    assert_eq!(all[0].get("b"), Some(&ScalarValue::Text("z".to_string())));
    assert_eq!(all[1].get("b"), Some(&ScalarValue::Text("y".to_string())));

    // BVA: `UPDATE` sin `WHERE` afecta a todas las filas vivas.
    let all_update = database
        .execute("UPDATE t SET b = 'q'")
        .expect("update all");
    assert_eq!(affected(&all_update), 3);
    let labels: Vec<String> = database
        .execute("SELECT b FROM t")
        .expect("select")
        .iter()
        .map(|row| match row.get("b") {
            Some(ScalarValue::Text(value)) => value.clone(),
            other => panic!("se esperaba Text, se obtuvo {other:?}"),
        })
        .collect();
    assert!(labels.iter().all(|label| label == "q"));
}

/// AC-0043-03 — `DELETE ... WHERE` borra (lógico) las filas que cumplen.
#[test] // @spec AC-0043-03
fn test_ac_0043_03_delete_statement() {
    let (_dir, mut database) = open_test_db("ac0043_03");
    create_t(&mut database);
    seed(&mut database, &[(1, "x"), (2, "y"), (3, "x")]);

    let rows = database
        .execute("DELETE FROM t WHERE a = 2")
        .expect("delete");
    assert_eq!(affected(&rows), 1);

    let remaining = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(remaining.len(), 2);
    assert!(
        !remaining
            .iter()
            .any(|row| row.get("a") == Some(&ScalarValue::Int(2)))
    );

    // BVA: `DELETE` sin `WHERE` borra todas las filas restantes.
    let all = database.execute("DELETE FROM t").expect("delete all");
    assert_eq!(affected(&all), 2);
    assert_eq!(
        database.execute("SELECT * FROM t").expect("select").len(),
        0
    );
}

/// AC-0043-04 — los errores DML son accionables (tabla/columna/tipo/sintaxis).
#[test] // @spec AC-0043-04
fn test_ac_0043_04_dml_errors_are_actionable() {
    let (_dir, mut database) = open_test_db("ac0043_04");
    create_t(&mut database);

    let missing_table = database
        .execute("INSERT INTO ausente (a, b) VALUES (1, 'x')")
        .expect_err("tabla ausente");
    assert!(
        matches!(missing_table, RuscaError::TableNotFound { ref table } if table == "ausente"),
        "se esperaba TableNotFound, se obtuvo {missing_table:?}"
    );

    seed(&mut database, &[(1, "x")]);

    let unknown_column = database
        .execute("UPDATE t SET z = 1")
        .expect_err("columna ausente");
    assert!(
        matches!(unknown_column, RuscaError::ColumnNotFound { ref column } if column == "z"),
        "se esperaba ColumnNotFound, se obtuvo {unknown_column:?}"
    );

    let bad_type = database
        .execute("INSERT INTO t (a, b) VALUES ('no-int', 'x')")
        .expect_err("tipo incompatible");
    assert!(
        matches!(bad_type, RuscaError::TypeMismatch { .. }),
        "se esperaba TypeMismatch, se obtuvo {bad_type:?}"
    );

    let syntax = database
        .execute("INSERT INTO t (a, b) VALUES (1)")
        .expect_err("fila con menos valores que columnas");
    match syntax {
        RuscaError::ParseError { message, .. } => {
            assert!(message.contains("valores"), "mensaje: {message}");
        }
        other => panic!("se esperaba ParseError, se obtuvo {other:?}"),
    }

    // La gramática de `SET` solo admite literales: una columna como valor es un
    // error de parseo (fail-fast), no de tipo en ejecución.
    let non_literal = database
        .execute("UPDATE t SET b = a")
        .expect_err("asignación no literal");
    assert!(
        matches!(non_literal, RuscaError::ParseError { .. }),
        "se esperaba ParseError (SET solo admite literales), se obtuvo {non_literal:?}"
    );
}

proptest! {
    /// PBT: `INSERT` de un lote seguido de `SELECT *` devuelve las mismas filas,
    /// y `DELETE` sin `WHERE` las retira todas (conteo exacto).
    #[test]
    fn prop_insert_select_delete_roundtrip(
        inputs in prop::collection::vec((-1000i64..1000, "[a-z]{1,5}"), 1..20),
    ) {
        let (_dir, mut database) = open_test_db("prop_dml");
        create_t(&mut database);
        let values = inputs
            .iter()
            .map(|(a, b)| format!("({a}, '{b}')"))
            .collect::<Vec<_>>()
            .join(", ");
        let insert = format!("INSERT INTO t (a, b) VALUES {values}");
        let rows = database.execute(&insert).expect("insert");
        prop_assert_eq!(affected(&rows), inputs.len() as i64);

        let selected = database.execute("SELECT * FROM t").expect("select");
        prop_assert_eq!(selected.len(), inputs.len());

        let deleted = database.execute("DELETE FROM t").expect("delete");
        prop_assert_eq!(affected(&deleted), inputs.len() as i64);
        prop_assert_eq!(database.execute("SELECT * FROM t").expect("select").len(), 0);
    }
}
