//! Integración end-to-end de los operadores documentales `->` y `@>` (SPEC-0044).
//!
//! Cubre AC-0044-01..04: extracción simple/anidada, contención, campo ausente
//! (la fila no cumple) y JSON inválido (error accionable).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Record, RecordId, RecordMeta, RuscaError,
    ScalarMap, ScalarValue,
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
    let database = Database::open(DbConfig::new(&path, 64)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `t(a INT)`.
fn create_doc_table(database: &mut Database) {
    database
        .create_table(
            "t",
            vec![ColumnDef {
                name: "a".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create_table");
}

/// Inserta una fila con escalar `a` y un documento JSON dado por `doc`.
///
/// Args:
///     database: Base abierta.
///     a: Valor del escalar `a`.
///     doc: Documento JSON como texto.
fn insert_doc(database: &mut Database, a: i64, doc: &str) {
    let scalars = ScalarMap::from([("a".to_string(), ScalarValue::Int(a))]);
    let record = Record {
        id: RecordId::new(),
        scalars,
        doc: Some(doc.parse().expect("documento JSON válido")),
        edges: EdgeSet::default(),
        vector: None,
        blob: None,
        meta: RecordMeta::default(),
    };
    database.insert_record("t", record).expect("insert_record");
}

/// Devuelve el entero de la columna `a` de una fila resultado.
///
/// Args:
///     row: Fila resultado.
///
/// Returns:
///     El valor entero de `a`.
///
/// Raises:
///     Panic si `a` no es `Int` (fallo de test).
fn int_of(row: &ruscadb::Row) -> i64 {
    match row.get("a") {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en 'a', se obtuvo {other:?}"),
    }
}

/// AC-0044-01 — `doc -> 'a' = 1` devuelve solo la fila con `a = 1`.
#[test] // @spec AC-0044-01
fn test_ac_0044_01_doc_extract_equals() {
    let (_dir, mut database) = open_test_db("ac0044_01");
    create_doc_table(&mut database);
    insert_doc(&mut database, 1, r#"{"a":1}"#);
    insert_doc(&mut database, 2, r#"{"a":2}"#);

    let rows = database
        .execute("SELECT * FROM t WHERE doc -> 'a' = 1")
        .expect("select");
    assert_eq!(rows.len(), 1, "solo la fila con doc {{a:1}}");
    assert_eq!(int_of(&rows[0]), 1);

    // El operador admite otros comparadores.
    let greater = database
        .execute("SELECT * FROM t WHERE doc -> 'a' > 1")
        .expect("select");
    assert_eq!(greater.len(), 1);
    assert_eq!(int_of(&greater[0]), 2);
}

/// AC-0044-02 — extracción por ruta anidada `doc -> 'nested.n' > 3`.
#[test] // @spec AC-0044-02
fn test_ac_0044_02_doc_extract_nested() {
    let (_dir, mut database) = open_test_db("ac0044_02");
    create_doc_table(&mut database);
    insert_doc(&mut database, 1, r#"{"nested":{"n":5}}"#);
    insert_doc(&mut database, 2, r#"{"nested":{"n":2}}"#);

    let rows = database
        .execute("SELECT * FROM t WHERE doc -> 'nested.n' > 3")
        .expect("select");
    assert_eq!(rows.len(), 1);
    assert_eq!(int_of(&rows[0]), 1);

    // Extracción profunda de tres niveles.
    insert_doc(&mut database, 3, r#"{"x":{"y":{"z":42}}}"#);
    let deep = database
        .execute("SELECT * FROM t WHERE doc -> 'x.y.z' = 42")
        .expect("select");
    assert_eq!(deep.len(), 1);
    assert_eq!(int_of(&deep[0]), 3);
}

/// AC-0044-03 — `doc @> '{...}'` devuelve la fila que contiene el subdocumento.
#[test] // @spec AC-0044-03
fn test_ac_0044_03_doc_contains() {
    let (_dir, mut database) = open_test_db("ac0044_03");
    create_doc_table(&mut database);
    insert_doc(&mut database, 1, r#"{"tags":["gato"],"nested":{"n":1}}"#);
    insert_doc(&mut database, 2, r#"{"tags":["perro"]}"#);

    let rows = database
        .execute(r#"SELECT * FROM t WHERE doc @> '{"tags":["gato"]}'"#)
        .expect("select");
    assert_eq!(rows.len(), 1);
    assert_eq!(int_of(&rows[0]), 1);

    // Contención anidada de objetos.
    let nested = database
        .execute(r#"SELECT * FROM t WHERE doc @> '{"nested":{"n":1}}'"#)
        .expect("select");
    assert_eq!(nested.len(), 1);
    assert_eq!(int_of(&nested[0]), 1);
}

/// AC-0044-04 — documento o campo ausente ⇒ la fila no cumple (sin error/panic).
#[test] // @spec AC-0044-04
fn test_ac_0044_04_missing_field_excludes_row() {
    let (_dir, mut database) = open_test_db("ac0044_04");
    create_doc_table(&mut database);
    insert_doc(&mut database, 1, r#"{"a":1}"#);
    insert_doc(&mut database, 2, r#"{"other":9}"#);
    // Registro sin documento.
    database
        .insert(
            "t",
            ScalarMap::from([("a".to_string(), ScalarValue::Int(3))]),
        )
        .expect("insert sin doc");

    let rows = database
        .execute("SELECT * FROM t WHERE doc -> 'a' = 1")
        .expect("select");
    assert_eq!(rows.len(), 1, "solo la fila con el campo presente");
    assert_eq!(int_of(&rows[0]), 1);

    // `@>` sobre un registro sin documento tampoco cumple.
    let contains = database
        .execute(r#"SELECT * FROM t WHERE doc @> '{"a":1}'"#)
        .expect("select");
    assert_eq!(contains.len(), 1);
    assert_eq!(int_of(&contains[0]), 1);
}

/// NF-0044-02 — JSON inválido en `@>` devuelve un error accionable.
#[test]
fn test_ac_0044_invalid_json_is_actionable() {
    let (_dir, mut database) = open_test_db("ac0044_json");
    create_doc_table(&mut database);
    insert_doc(&mut database, 1, r#"{"a":1}"#);

    let error = database
        .execute(r#"SELECT * FROM t WHERE doc @> '{no-es-json}'"#)
        .expect_err("JSON inválido");
    match error {
        RuscaError::ParseError { message, .. } => {
            assert!(message.contains("@>"), "mensaje: {message}");
        }
        other => panic!("se esperaba ParseError, se obtuvo {other:?}"),
    }
}

proptest! {
    /// PBT (dual verification): para cualquier entero `v`, `doc -> 'a' = v`
    /// recupera exactamente las filas cuyo documento tiene `a = v`.
    #[test]
    fn prop_doc_extract_matches_expected_rows(values in prop::collection::vec(-1000i64..1000, 1..20)) {
        let (_dir, mut database) = open_test_db("prop_doc");
        create_doc_table(&mut database);
        for (index, value) in values.iter().enumerate() {
            insert_doc(&mut database, index as i64, &format!("{{\"a\":{value}}}"));
        }
        for probe in [-1000i64, -1, 0, 1, 500, 999] {
            let rows = database
                .execute(&format!("SELECT * FROM t WHERE doc -> 'a' = {probe}"))
                .expect("select");
            let expected = values.iter().filter(|value| **value == probe).count();
            prop_assert_eq!(rows.len(), expected, "probe = {}", probe);
        }
    }
}
