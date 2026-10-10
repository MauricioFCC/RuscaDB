//! Snapshots de renderizado RQL con insta (SPEC-0061, auditoría #6).
//!
//! La dependencia `insta` estaba declarada pero sin uso (gate fantasma).
//! Estos snapshots fijan el `Display` canónico del IR (roundtrips de las
//! features nuevas) y los mensajes de error accionables: cualquier cambio de
//! formato rompe el snapshot a propósito y exige revisión explícita.

#![allow(clippy::expect_used)]

use ruscadb_query::parse_statement;

/// GROUP BY + HAVING con roundtrip canónico (SPEC-0051).
#[test]
fn snapshot_group_by_having_roundtrip() {
    let statement =
        parse_statement("SELECT b, COUNT(*) FROM t GROUP BY b HAVING COUNT(*) > 1").expect("parse");
    insta::assert_snapshot!("group_by_having", format!("{statement}"));
}

/// JOIN + ORDER BY cualificado con roundtrip canónico (SPEC-0052).
#[test]
fn snapshot_join_order_by_roundtrip() {
    let statement =
        parse_statement("SELECT * FROM a JOIN b ON a.x = b.y ORDER BY a.z").expect("parse");
    insta::assert_snapshot!("join_order_by", format!("{statement}"));
}

/// EXPLAIN sobre KNN (plan textual estable).
#[test]
fn snapshot_explain_knn() {
    let statement =
        parse_statement("EXPLAIN SELECT * FROM t KNN embedding <|2|> [0.1, 0.2]").expect("parse");
    insta::assert_snapshot!("explain_knn", format!("{statement}"));
}

/// Error accionable de HAVING sin GROUP BY (mensaje + posición estables).
#[test]
fn snapshot_having_without_group_by_error() {
    let error =
        parse_statement("SELECT COUNT(*) FROM t HAVING COUNT(*) > 1").expect_err("debe fallar");
    insta::assert_snapshot!("having_error", format!("{error}"));
}
