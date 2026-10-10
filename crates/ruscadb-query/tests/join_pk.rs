//! Parser del INNER JOIN por PK (SPEC-0052): `FROM a JOIN b ON a.x = b.y`.
//!
//! Cubre el parseo válido (una sola igualdad cualificada), el roundtrip
//! `Display → parse` y los rechazos accionables (no-equi, multi-condición,
//! lados sin cualificar, self-join).

#![allow(clippy::expect_used)]

use ruscadb_query::{JoinClause, parse, parse_statement};

/// AC-0052-01 — `FROM a JOIN b ON a.x = b.y` puebla `Select::join`.
#[test]
// @spec AC-0052-01
fn test_ac_0052_01_parse_inner_join() {
    let select =
        parse("SELECT authors.name FROM authors JOIN books ON authors.id = books.author_id")
            .expect("parse");
    assert_eq!(select.from, "authors");
    let join = select.join.clone().expect("cláusula JOIN");
    assert_eq!(join.table, "books");
    assert_eq!(join.left.table, "authors");
    assert_eq!(join.left.column, "id");
    assert_eq!(join.right.table, "books");
    assert_eq!(join.right.column, "author_id");
    assert_eq!(
        select.to_string(),
        "SELECT authors.name FROM authors JOIN books ON authors.id = books.author_id"
    );
    assert_eq!(
        parse(&select.to_string()).expect("reparse"),
        select,
        "Display → parse es idempotente con JOIN"
    );
}

/// AC-0052-01 — el orden de los lados del `ON` se conserva tal cual.
#[test]
// @spec AC-0052-01
fn test_ac_0052_01_parse_join_reversed_sides() {
    let select =
        parse("SELECT * FROM authors JOIN books ON books.author_id = authors.id").expect("parse");
    let join = select.join.clone().expect("cláusula JOIN");
    assert_eq!(join.left.table, "books");
    assert_eq!(join.right.table, "authors");
    assert_eq!(
        JoinClause {
            table: "books".to_string(),
            left: ruscadb_query::ColumnRef {
                table: "books".to_string(),
                column: "author_id".to_string(),
            },
            right: ruscadb_query::ColumnRef {
                table: "authors".to_string(),
                column: "id".to_string(),
            },
        },
        join
    );
}

/// AC-0052-04 — el `ON` no-equi se rechaza con `ParseError` accionable.
#[test]
// @spec AC-0052-04
fn test_ac_0052_04_non_equi_parse_errors() {
    for operator in [">", "<", "!=", ">=", "<="] {
        let input = format!("SELECT * FROM a JOIN b ON a.x {operator} b.y");
        let error = parse(&input).expect_err("ON no-equi");
        let message = error.to_string();
        assert!(
            message.contains("igualdad") || message.contains('='),
            "error accionable para {input:?}: {message}"
        );
    }
}

/// AC-0052-04 — el `ON` multi-condición se rechaza con `ParseError`.
#[test]
// @spec AC-0052-04
fn test_ac_0052_04_multi_condition_parse_errors() {
    let error =
        parse("SELECT * FROM a JOIN b ON a.x = b.y AND a.z = b.w").expect_err("multi-condición");
    assert!(
        error.to_string().contains("JOIN"),
        "el error menciona JOIN: {error}"
    );
}

/// El `ON` exige ambos lados cualificados (`tabla.columna`).
#[test]
fn test_ac_0052_parse_unqualified_on_errors() {
    for input in [
        "SELECT * FROM a JOIN b ON x = b.y",
        "SELECT * FROM a JOIN b ON a.x = y",
        "SELECT * FROM a JOIN b ON a.x = 1",
    ] {
        let error = parse(input).expect_err("lado sin cualificar");
        assert!(
            error.to_string().contains("tabla.columna") || error.to_string().contains("JOIN"),
            "error accionable para {input:?}: {error}"
        );
    }
}

/// El self-join sin alias se rechaza (ambiguo, fuera de alcance SPEC-0052).
#[test]
fn test_ac_0052_parse_self_join_errors() {
    let error = parse("SELECT * FROM a JOIN a ON a.x = a.y").expect_err("self-join sin alias");
    assert!(
        error.to_string().contains("alias") || error.to_string().contains("self-join"),
        "error accionable: {error}"
    );
}

/// `SELECT` sin JOIN sigue sin poblar `join` (sin regresión, NF-0052-01).
#[test]
fn test_ac_0052_no_join_is_none() {
    let select = parse("SELECT a FROM t WHERE a = 1 LIMIT 2").expect("parse");
    assert_eq!(select.join, None);
    assert_eq!(
        parse_statement("SELECT a FROM t WHERE a = 1 LIMIT 2").expect("statement"),
        ruscadb_query::Statement::Select(select)
    );
}
