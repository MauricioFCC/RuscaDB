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

pub use ast::{
    AggFunc, Aggregate, CompareOp, Explain, Expr, KnnClause, OrderBy, Projection, Select,
    Statement, TraverseClause,
};
pub use parser::{parse, parse_statement};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Keyword;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use rstest::rstest;
    use ruscadb_core::RuscaError;

    /// Palabras reservadas de RQL: no pueden usarse como identificadores.
    const RESERVED_IDENTIFIERS: [&str; 21] = [
        "select", "from", "where", "and", "limit", "knn", "traverse", "depth", "explain", "match",
        "order", "by", "asc", "desc", "group", "count", "sum", "avg", "min", "max", "as",
    ];

    /// Estrategia de identificadores que evita las palabras reservadas.
    ///
    /// Returns:
    ///     Generador de nombres `[a-z]{1,8}` que no son palabras clave.
    fn ident_strategy() -> impl Strategy<Value = String> {
        "[a-z]{1,8}".prop_filter("identificador reservado", |name| {
            !RESERVED_IDENTIFIERS.contains(&name.as_str())
        })
    }

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

    /// Un número con dos puntos decimales es un error léxico en el segundo `.`.
    #[test]
    fn test_number_with_two_dots_is_error() {
        let error = parse("SELECT * FROM t WHERE a = 1.2.3").unwrap_err();
        match error {
            RuscaError::ParseError { message, position } => {
                assert!(
                    message.contains("carácter inesperado"),
                    "mensaje: {message}"
                );
                assert_eq!(position, 29, "el segundo '.' está en la posición 29");
            }
            other => panic!("{other:?}"),
        }
    }

    /// Un entero fuera del rango de `i64` es error (sin overflow silencioso).
    #[test]
    fn test_integer_overflow_is_error() {
        assert!(parse("SELECT * FROM t WHERE a = 99999999999999999999999999").is_err());
        assert!(parse("SELECT * FROM t WHERE a = 9223372036854775807").is_ok());
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
            columns in prop::collection::vec(ident_strategy(), 1..4),
            table in ident_strategy(),
            limit in prop::option::of(0u64..1000),
        ) {
            let select = Select {
                projection: Projection::Columns(columns),
                aggregates: vec![],
                group_by: vec![],
                from: table,
                filter: None,
                knn: None,
                traverse: None,
                order_by: None,
                limit,
            };
            let text = select.to_string();
            let reparsed = parse(&text).expect("reparse");
            prop_assert_eq!(reparsed, select);
        }
    }

    /// AC-0015-01 — parseo de `KNN`.
    #[test]
    fn test_ac_0015_01_parse_knn() {
        let statement = parse_statement("SELECT * FROM docs KNN embedding <|5|> [0.1, 0.2, 0.3]")
            .expect("parse");
        let Statement::Select(select) = statement else {
            panic!("se esperaba Statement::Select");
        };
        assert_eq!(
            select.knn,
            Some(KnnClause {
                column: "embedding".to_string(),
                k: 5,
                query: vec![0.1, 0.2, 0.3],
            })
        );
        assert_eq!(select.traverse, None);
        assert_eq!(select.limit, None);
    }

    /// AC-0015-02 — parseo de `TRAVERSE`.
    #[test]
    fn test_ac_0015_02_parse_traverse() {
        let statement =
            parse_statement("SELECT * FROM nodes TRAVERSE edges DEPTH 3").expect("parse");
        let Statement::Select(select) = statement else {
            panic!("se esperaba Statement::Select");
        };
        assert_eq!(
            select.traverse,
            Some(TraverseClause {
                column: "edges".to_string(),
                depth: 3,
            })
        );
        assert_eq!(select.knn, None);
    }

    /// AC-0015-03 — parseo de `EXPLAIN` que envuelve el `Select` interno.
    #[test]
    fn test_ac_0015_03_parse_explain() {
        let statement = parse_statement("EXPLAIN SELECT * FROM t WHERE a = 1").expect("parse");
        let Statement::Explain(explain) = statement else {
            panic!("se esperaba Statement::Explain");
        };
        assert_eq!(explain.inner.from, "t");
        assert!(matches!(explain.inner.filter, Some(Expr::Compare { .. })));
        assert_eq!(
            parse_statement(&explain.to_string()).expect("reparse"),
            Statement::Explain(explain)
        );
        assert!(
            parse("EXPLAIN SELECT * FROM t").is_err(),
            "parse (solo SELECT) debe rechazar EXPLAIN"
        );
    }

    /// AC-0015-04 — roundtrip `Display → parse` con KNN/TRAVERSE (orden canónico).
    #[test]
    fn test_ac_0015_04_display_parse_roundtrip_extensions() {
        let query = parse(
            "SELECT a, b FROM t WHERE a = 1 KNN emb <|3|> [1, 2.5] TRAVERSE edges DEPTH 2 LIMIT 7",
        )
        .expect("parse");
        let text = query.to_string();
        assert_eq!(
            text,
            "SELECT a, b FROM t WHERE a = 1 KNN emb <|3|> [1.0, 2.5] TRAVERSE edges DEPTH 2 LIMIT 7"
        );
        assert_eq!(parse(&text).expect("reparse"), query);
    }

    /// AC-0015-05 — formas mal formadas devuelven `ParseError`.
    #[test]
    fn test_ac_0015_05_malformed_extensions_are_errors() {
        let cases = [
            "SELECT * FROM t KNN embedding <|x|> []",
            "SELECT * FROM t KNN embedding <||> []",
            "SELECT * FROM t KNN embedding <|3|> [1, 2",
            "SELECT * FROM t KNN embedding <|3|> [x]",
            "SELECT * FROM t TRAVERSE edges",
            "SELECT * FROM t TRAVERSE edges DEPTH x",
            "SELECT * FROM t TRAVERSE edges DEPTH",
            "SELECT * FROM t TRAVERSE edges DEPTH 70000",
        ];
        for input in cases {
            let error = parse_statement(input).unwrap_err();
            assert!(
                matches!(error, RuscaError::ParseError { .. }),
                "input: {input} -> {error:?}"
            );
        }
        let missing_depth = parse_statement("SELECT * FROM t TRAVERSE edges").unwrap_err();
        match missing_depth {
            RuscaError::ParseError { message, .. } => assert!(message.contains("DEPTH")),
            other => panic!("{other:?}"),
        }
    }

    /// Un vector de consulta vacío es válido.
    #[test]
    fn test_knn_empty_vector_is_valid() {
        let select = parse("SELECT * FROM t KNN embedding <|0|> []").expect("parse");
        assert_eq!(select.knn.expect("knn").query, Vec::<f64>::new());
    }

    /// Un `|` suelto (sin `>`) es un error léxico de la familia `|>`.
    #[test]
    fn test_lone_pipe_is_error() {
        let error = parse("SELECT * FROM t WHERE a | 1").unwrap_err();
        match error {
            RuscaError::ParseError { message, .. } => {
                assert!(message.contains("|>"), "mensaje: {message}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// `as_str` cubre las nuevas palabras clave.
    #[test]
    fn test_extension_keywords_as_str() {
        assert_eq!(Keyword::Knn.as_str(), "KNN");
        assert_eq!(Keyword::Traverse.as_str(), "TRAVERSE");
        assert_eq!(Keyword::Depth.as_str(), "DEPTH");
        assert_eq!(Keyword::Explain.as_str(), "EXPLAIN");
        assert_eq!(Keyword::Match.as_str(), "MATCH");
    }

    /// `MATCH(col, 'texto')` se parsea como predicado de `WHERE`.
    #[test]
    fn test_parse_match_predicate() {
        let query = parse("SELECT * FROM t WHERE MATCH(titulo, 'gato negro')").expect("parse");
        assert_eq!(
            query.filter,
            Some(Expr::Match {
                column: "titulo".to_string(),
                query: "gato negro".to_string(),
            })
        );
    }

    /// `MATCH` es componible con comparaciones mediante `AND`.
    #[test]
    fn test_parse_match_composes_with_and() {
        let query = parse("SELECT * FROM t WHERE a > 1 AND MATCH(titulo, 'gato')").expect("parse");
        let expected = Expr::And(
            Box::new(Expr::Compare {
                left: Box::new(Expr::Column("a".to_string())),
                op: CompareOp::Gt,
                right: Box::new(Expr::Int(1)),
            }),
            Box::new(Expr::Match {
                column: "titulo".to_string(),
                query: "gato".to_string(),
            }),
        );
        assert_eq!(query.filter, Some(expected));
    }

    /// `MATCH` sobrevive al roundtrip `Display → parse`.
    #[test]
    fn test_match_display_parse_roundtrip() {
        let query =
            parse("SELECT titulo FROM t WHERE MATCH(titulo, 'gato') LIMIT 3").expect("parse");
        assert_eq!(
            query.to_string(),
            "SELECT titulo FROM t WHERE MATCH(titulo, 'gato') LIMIT 3"
        );
        assert_eq!(parse(&query.to_string()).expect("reparse"), query);
    }

    /// Formas mal formadas de `MATCH` devuelven `ParseError` con posición.
    #[test]
    fn test_malformed_match_is_error() {
        let cases = [
            "SELECT * FROM t WHERE MATCH titulo, 'gato')",
            "SELECT * FROM t WHERE MATCH(titulo 'gato')",
            "SELECT * FROM t WHERE MATCH(titulo, gato)",
            "SELECT * FROM t WHERE MATCH(titulo, 'gato'",
        ];
        for input in cases {
            let error = parse(input).unwrap_err();
            assert!(
                matches!(error, RuscaError::ParseError { .. }),
                "input: {input} -> {error:?}"
            );
        }
    }

    /// AC-0040-01 — parseo de `GROUP BY` + agregados en la proyección y roundtrip.
    #[test]
    fn test_ac_0040_01_parse_group_by() {
        let select = parse("SELECT a, COUNT(*) FROM t GROUP BY a").expect("parse");
        assert_eq!(
            select.group_by,
            vec!["a".to_string()],
            "GROUP BY a debe poblar group_by"
        );
        assert_eq!(
            select.projection,
            Projection::Columns(vec!["a".to_string()])
        );
        assert_eq!(
            select.aggregates,
            vec![Aggregate {
                func: AggFunc::CountStar,
                column: None,
                alias: None,
            }]
        );
        let canonical = "SELECT a, COUNT(*) FROM t GROUP BY a";
        assert_eq!(select.to_string(), canonical);
        assert_eq!(parse(canonical).expect("reparse"), select);
    }

    /// AC-0040-01 (extensión) — agregados variados, orden canónico y alias.
    #[test]
    fn test_ac_0040_01_parse_aggregates_and_alias() {
        let query = parse(
            "SELECT b, SUM(a), AVG(a), MIN(a), MAX(a), COUNT(a) FROM t GROUP BY b ORDER BY b DESC LIMIT 3",
        )
        .expect("parse");
        assert_eq!(query.group_by, vec!["b".to_string()]);
        assert_eq!(query.aggregates.len(), 5);
        assert_eq!(query.aggregates[0].func, AggFunc::Sum);
        assert_eq!(query.aggregates[0].column.as_deref(), Some("a"));
        assert_eq!(query.limit, Some(3));
        let canonical = "SELECT b, SUM(a), AVG(a), MIN(a), MAX(a), COUNT(a) FROM t GROUP BY b ORDER BY b DESC LIMIT 3";
        assert_eq!(query.to_string(), canonical);
        assert_eq!(parse(canonical).expect("reparse"), query);

        let aliased = parse("SELECT COUNT(*) AS total FROM t").expect("parse");
        assert_eq!(aliased.aggregates[0].alias.as_deref(), Some("total"));
        assert_eq!(aliased.to_string(), "SELECT COUNT(*) AS total FROM t");
        assert_eq!(parse(&aliased.to_string()).expect("reparse"), aliased);
    }

    /// Formas mal formadas de agregados y `GROUP BY` devuelven `ParseError`.
    #[test]
    fn test_ac_0040_01_malformed_aggregates_are_errors() {
        let cases = [
            "SELECT SUM(*) FROM t",
            "SELECT COUNT(a FROM t",
            "SELECT COUNT FROM t",
            "SELECT a FROM t GROUP BY",
            "SELECT a FROM t GROUP a",
        ];
        for input in cases {
            let error = parse(input).unwrap_err();
            assert!(
                matches!(error, RuscaError::ParseError { .. }),
                "input: {input} -> {error:?}"
            );
        }
    }

    /// AC-0036-01 — parseo de `ORDER BY <col> [ASC|DESC]` y roundtrip canónico.
    #[test]
    fn test_ac_0036_01_parse_order_by() {
        let ascending = parse("SELECT * FROM t ORDER BY a").expect("parse");
        assert_eq!(
            ascending.order_by,
            Some(OrderBy {
                column: "a".to_string(),
                desc: false,
            })
        );

        let explicit = parse("SELECT * FROM t ORDER BY a ASC").expect("parse");
        assert_eq!(explicit.order_by, ascending.order_by);

        let descending = parse("SELECT a FROM t ORDER BY a DESC LIMIT 2").expect("parse");
        assert_eq!(
            descending.order_by,
            Some(OrderBy {
                column: "a".to_string(),
                desc: true,
            })
        );
        assert_eq!(descending.limit, Some(2));

        let canonical = parse("SELECT a FROM t WHERE a > 1 ORDER BY a DESC LIMIT 3")
            .expect("parse")
            .to_string();
        assert_eq!(
            canonical,
            "SELECT a FROM t WHERE a > 1 ORDER BY a DESC LIMIT 3"
        );
        assert_eq!(
            parse(&canonical).expect("reparse").to_string(),
            canonical,
            "Display → parse debe ser idempotente con ORDER BY"
        );
    }

    /// `ORDER BY` aparece tras `TRAVERSE` y antes de `LIMIT` (orden canónico).
    #[test]
    fn test_order_by_canonical_position() {
        let query = parse("SELECT * FROM nodes TRAVERSE edges DEPTH 2 ORDER BY a DESC LIMIT 5")
            .expect("parse");
        assert_eq!(
            query.to_string(),
            "SELECT * FROM nodes TRAVERSE edges DEPTH 2 ORDER BY a DESC LIMIT 5"
        );
    }

    /// Formas mal formadas de `ORDER BY` devuelven `ParseError` con posición.
    #[test]
    fn test_malformed_order_by_is_error() {
        let cases = [
            "SELECT * FROM t ORDER a",
            "SELECT * FROM t ORDER BY",
            "SELECT * FROM t ORDER BY a DESC DESC",
            "SELECT * FROM t ORDER BY a LIMIT",
        ];
        for input in cases {
            let error = parse(input).unwrap_err();
            assert!(
                matches!(error, RuscaError::ParseError { .. }),
                "input: {input} -> {error:?}"
            );
        }
    }

    proptest! {
        /// Metamórfica: roundtrip `Display → parse` con cláusulas aleatorias.
        #[test]
        fn prop_display_parse_roundtrip_with_extensions(
            columns in prop::collection::vec(ident_strategy(), 1..4),
            table in ident_strategy(),
            knn_column in ident_strategy(),
            k in 0u64..1000,
            query in prop::collection::vec(0.0f64..1000.0, 0..4),
            traverse_column in ident_strategy(),
            depth in 0u16..1000,
            limit in prop::option::of(0u64..1000),
        ) {
            let select = Select {
                projection: Projection::Columns(columns),
                aggregates: vec![],
                group_by: vec![],
                from: table,
                filter: None,
                knn: Some(KnnClause { column: knn_column, k, query }),
                traverse: Some(TraverseClause { column: traverse_column, depth }),
                order_by: None,
                limit,
            };
            let text = select.to_string();
            let reparsed = parse(&text).expect("reparse");
            prop_assert_eq!(reparsed, select);
        }

        /// Metamórfica: roundtrip `Display → parse_statement` para `EXPLAIN`.
        #[test]
        fn prop_explain_display_parse_roundtrip(
            table in ident_strategy(),
            filter_column in ident_strategy(),
            filter_value in 0i64..1000,
        ) {
            let select = Select {
                projection: Projection::All,
                aggregates: vec![],
                group_by: vec![],
                from: table,
                filter: Some(Expr::Compare {
                    left: Box::new(Expr::Column(filter_column)),
                    op: CompareOp::Eq,
                    right: Box::new(Expr::Int(filter_value)),
                }),
                knn: None,
                traverse: None,
                order_by: None,
                limit: None,
            };
            let statement = Statement::Explain(Explain { inner: Box::new(select) });
            let text = statement.to_string();
            let reparsed = parse_statement(&text).expect("reparse");
            prop_assert_eq!(reparsed, statement);
        }

        /// Metamórfica: roundtrip `Display → parse` de `MATCH` con texto arbitrario.
        #[test]
        fn prop_match_display_parse_roundtrip(
            table in ident_strategy(),
            column in ident_strategy(),
            query in "[a-zA-Z0-9 ]{0,24}",
        ) {
            let select = Select {
                projection: Projection::All,
                aggregates: vec![],
                group_by: vec![],
                from: table,
                filter: Some(Expr::Match { column, query }),
                knn: None,
                traverse: None,
                order_by: None,
                limit: None,
            };
            let text = select.to_string();
            let reparsed = parse(&text).expect("reparse");
            prop_assert_eq!(reparsed, select);
        }

        /// Metamórfica: roundtrip `Display → parse` con `ORDER BY` aleatorio.
        #[test]
        fn prop_order_by_display_parse_roundtrip(
            table in ident_strategy(),
            column in ident_strategy(),
            desc in any::<bool>(),
            limit in prop::option::of(0u64..1000),
        ) {
            let select = Select {
                projection: Projection::All,
                aggregates: vec![],
                group_by: vec![],
                from: table,
                filter: None,
                knn: None,
                traverse: None,
                order_by: Some(OrderBy { column, desc }),
                limit,
            };
            let text = select.to_string();
            let reparsed = parse(&text).expect("reparse");
            prop_assert_eq!(reparsed, select);
        }
    }
}
