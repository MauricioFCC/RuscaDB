//! # ruscadb-query
//!
//! Query engine de RuscaDB: **RQL** (lexer + parser recursive-descent) y su IR
//! tipado. Especificación: `specs/query_language.md` (SPEC-0005).
//!
//! La integración con DataFusion/sqlparser-rs (dialecto completo, grafos y
//! vectores) se planifica como evolución posterior (ADR-001/ADR-007).

#![forbid(unsafe_code)]

pub mod ast;
pub mod lexer;
mod parser;

pub use ast::{CompareOp, Expr, Projection, Select};
pub use parser::parse;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use rstest::rstest;
    use ruscadb_core::RuscaError;

    /// AC-0005-01 — `SELECT` con proyección de columnas.
    #[test]
    fn test_ac_0005_01_parse_simple_select() {
        let query = parse("SELECT a, b FROM t").expect("parse");
        assert_eq!(
            query.projection,
            Projection::Columns(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(query.from, "t");
        assert_eq!(query.filter, None);
        assert_eq!(query.limit, None);
    }

    /// AC-0005-02 — `WHERE` con `AND` de dos comparaciones.
    #[test]
    fn test_ac_0005_02_parse_where_and() {
        let query = parse("SELECT * FROM t WHERE a = 1 AND b < 2").expect("parse");
        let expected = Expr::And(
            Box::new(Expr::Compare {
                left: Box::new(Expr::Column("a".to_string())),
                op: CompareOp::Eq,
                right: Box::new(Expr::Int(1)),
            }),
            Box::new(Expr::Compare {
                left: Box::new(Expr::Column("b".to_string())),
                op: CompareOp::Lt,
                right: Box::new(Expr::Int(2)),
            }),
        );
        assert_eq!(query.filter, Some(expected));
    }

    /// AC-0005-03 — `LIMIT` numérico.
    #[test]
    fn test_ac_0005_03_parse_limit() {
        let query = parse("SELECT * FROM t LIMIT 10").expect("parse");
        assert_eq!(query.limit, Some(10));
    }

    /// AC-0005-04 — entrada inválida devuelve `ParseError` con posición exacta.
    #[test]
    fn test_ac_0005_04_parse_error_has_position() {
        let error = parse("SELECT FROM").unwrap_err();
        match error {
            RuscaError::ParseError { position, .. } => assert_eq!(position, 7),
            other => panic!("se esperaba ParseError, se obtuvo {other:?}"),
        }
    }

    /// Todos los operadores de comparación se parsean correctamente.
    #[rstest]
    #[case("a = 1", CompareOp::Eq)]
    #[case("a != 1", CompareOp::NotEq)]
    #[case("a < 1", CompareOp::Lt)]
    #[case("a <= 1", CompareOp::LtEq)]
    #[case("a > 1", CompareOp::Gt)]
    #[case("a >= 1", CompareOp::GtEq)]
    fn test_parse_all_compare_ops(#[case] predicate: &str, #[case] op: CompareOp) {
        let query = parse(&format!("SELECT * FROM t WHERE {predicate}")).expect("parse");
        match query.filter {
            Some(Expr::Compare { op: parsed, .. }) => assert_eq!(parsed, op),
            other => panic!("se esperaba Compare, se obtuvo {other:?}"),
        }
    }

    /// Literales flotantes y de texto.
    #[test]
    fn test_parse_float_and_text_literals() {
        let query = parse("SELECT * FROM t WHERE a = 1.5 AND b = 'x'").expect("parse");
        let expected = Expr::And(
            Box::new(Expr::Compare {
                left: Box::new(Expr::Column("a".to_string())),
                op: CompareOp::Eq,
                right: Box::new(Expr::Float(1.5)),
            }),
            Box::new(Expr::Compare {
                left: Box::new(Expr::Column("b".to_string())),
                op: CompareOp::Eq,
                right: Box::new(Expr::Text("x".to_string())),
            }),
        );
        assert_eq!(query.filter, Some(expected));
    }

    /// Un número con dos puntos decimales es un error léxico.
    #[test]
    fn test_number_with_two_dots_is_error() {
        assert!(parse("SELECT * FROM t WHERE a = 1.2.3").is_err());
    }

    /// Un flotante entero conserva el punto decimal en el roundtrip.
    #[test]
    fn test_integral_float_roundtrip() {
        let query = parse("SELECT * FROM t WHERE a = 2.0").expect("parse");
        let text = query.to_string();
        assert!(
            text.contains("2.0"),
            "el texto debe conservar el punto decimal: {text}"
        );
        assert_eq!(parse(&text).expect("reparse"), query);
    }

    /// Un `!` suelto (sin `=`) es un error léxico.
    #[test]
    fn test_lone_bang_is_error() {
        assert!(parse("SELECT * FROM t WHERE a ! 1").is_err());
    }

    /// Una cadena sin cerrar es un error léxico.
    #[test]
    fn test_unterminated_string_is_error() {
        assert!(parse("SELECT * FROM t WHERE a = 'x").is_err());
    }

    /// Tokens sobrantes tras la consulta son un error.
    #[test]
    fn test_trailing_tokens_are_error() {
        assert!(parse("SELECT * FROM t extra").is_err());
    }

    /// Los mensajes de error nombran la palabra clave esperada.
    #[test]
    fn test_error_message_names_expected_keyword() {
        let from_error = parse("SELECT a, b t").unwrap_err();
        match from_error {
            RuscaError::ParseError { message, .. } => assert!(message.contains("FROM")),
            other => panic!("{other:?}"),
        }
        let select_error = parse("foo").unwrap_err();
        match select_error {
            RuscaError::ParseError { message, .. } => assert!(message.contains("SELECT")),
            other => panic!("{other:?}"),
        }
    }

    /// AC-0005-05 — roundtrip `Display → parse`.
    #[test]
    fn test_ac_0005_05_display_parse_roundtrip() {
        let query = parse("SELECT a, b FROM t WHERE a = 1 AND b < 2 LIMIT 5").expect("parse");
        let text = query.to_string();
        let reparsed = parse(&text).expect("reparse");
        assert_eq!(reparsed, query);
    }

    proptest! {
        /// Propiedad: el parser nunca entra en pánico ante texto arbitrario.
        #[test]
        fn prop_parse_never_panics(input in "\\PC{0,64}") {
            let _ = parse(&input);
        }

        /// Metamórfica: `Display` seguido de `parse` reconstruye el IR.
        #[test]
        fn prop_display_parse_roundtrip(
            columns in prop::collection::vec("[a-z]{1,8}", 1..4),
            table in "[a-z]{1,8}",
            limit in prop::option::of(0u64..1000),
        ) {
            let select = Select {
                projection: Projection::Columns(columns),
                from: table,
                filter: None,
                limit,
            };
            let text = select.to_string();
            let reparsed = parse(&text).expect("reparse");
            prop_assert_eq!(reparsed, select);
        }
    }
}
