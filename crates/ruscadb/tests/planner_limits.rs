//! Límites de recursos del planner (SPEC-0054, §6.4/R9).
//!
//! Cubre AC-0054-01..05: profundidad AST, budget de complejidad, LIMIT
//! implícito, timeout cooperativo (vía executor, test unitario) y aviso de
//! full-scan en EXPLAIN.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, RuscaError, ScalarMap, ScalarValue};

/// Budget de complejidad documentado (debe coincidir con el planner).
const COMPLEXITY_BUDGET: u64 = 100_000;
/// Límite implícito documentado.
const IMPLICIT_LIMIT: usize = 1000;

/// Abre una base temporal de pruebas.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
    (dir, database)
}

/// Crea `t(a INT, b TEXT)` vacía.
fn create_ab_table(database: &mut Database) {
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

/// Cadena `a = 1 AND a = 2 AND ...` con `count` comparaciones.
fn and_chain(count: usize) -> String {
    (1..=count)
        .map(|value| format!("a = {value}"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// AC-0054-01 — anidamiento > 64 niveles se rechaza en el parseo.
///
/// Given: un WHERE con 100 comparaciones (árbol And de profundidad ~100).
/// When: se ejecuta.
/// Then: `ParseError` accionable que menciona el límite 64.
#[test]
// @spec AC-0054-01
fn test_ac_0054_01_deep_nesting_rejected() {
    let (_dir, mut database) = open_test_db("ac0054_01");
    create_ab_table(&mut database);

    let error = database
        .execute(&format!("SELECT * FROM t WHERE {}", and_chain(100)))
        .expect_err("anidamiento profundo debe fallar");
    assert!(
        matches!(error, RuscaError::ParseError { .. }),
        "se esperaba ParseError, se obtuvo {error:?}"
    );
    assert!(
        error.to_string().contains("64"),
        "el mensaje debe mencionar el límite 64: {error}"
    );
}

/// AC-0054-02 — cadena que supera el budget se rechaza pre-ejecución.
///
/// Given: un KNN con vector de 12000 dims (120000 unidades > budget).
/// When: se ejecuta.
/// Then: `ResourceLimit` con recurso `query_complexity` (sin tocar disco).
#[test]
// @spec AC-0054-02
fn test_ac_0054_02_long_chain_over_budget_rejected() {
    let (_dir, mut database) = open_test_db("ac0054_02");
    create_ab_table(&mut database);

    let query = "[0.1, ".to_string() + &vec!["0.2"; 11_999].join(", ") + "]";
    assert!(query.len() > COMPLEXITY_BUDGET as usize / 10);
    let error = database
        .execute(&format!("SELECT * FROM t KNN embedding <|2|> {query}"))
        .expect_err("sobre budget debe fallar");
    assert!(
        matches!(error, RuscaError::ResourceLimit { ref resource, .. } if resource == "query_complexity"),
        "se esperaba ResourceLimit(query_complexity), se obtuvo {error:?}"
    );
}

/// AC-0054-03 — SELECT sin LIMIT devuelve como máximo 1000 filas.
///
/// Given: tabla con 1500 filas.
/// When: se ejecuta sin LIMIT.
/// Then: devuelve exactamente 1000 filas.
#[test]
// @spec AC-0054-03
fn test_ac_0054_03_implicit_limit_injected() {
    let (_dir, mut database) = open_test_db("ac0054_03");
    create_ab_table(&mut database);
    seed_rows(&mut database, 1500);

    let rows = database.execute("SELECT * FROM t").expect("select");
    assert_eq!(
        rows.len(),
        IMPLICIT_LIMIT,
        "límite implícito: {len}",
        len = rows.len()
    );

    let limited = database
        .execute("SELECT * FROM t LIMIT 10")
        .expect("select con limit");
    assert_eq!(limited.len(), 10, "el LIMIT explícito manda");
}

/// AC-0054-05 — EXPLAIN avisa del full-scan sobre el umbral.
///
/// Given: tabla con 1500 filas (sobre el umbral de aviso).
/// When: se pide EXPLAIN del SELECT.
/// Then: el plan incluye `FullScan(t)` y un aviso de full-scan.
#[test]
// @spec AC-0054-05
fn test_ac_0054_05_explain_flags_full_scan() {
    let (_dir, mut database) = open_test_db("ac0054_05");
    create_ab_table(&mut database);
    seed_rows(&mut database, 1500);

    let plan = database
        .explain("EXPLAIN SELECT * FROM t")
        .expect("explain");
    assert!(
        plan.iter().any(|line| line.contains("FullScan(t)")),
        "el plan debe mostrar FullScan(t): {plan:?}"
    );
    assert!(
        plan.iter().any(|line| line.contains("full-scan")),
        "el plan debe avisar del full-scan: {plan:?}"
    );

    let small_plan = {
        database.create_index("t", "a").expect("create_index");
        database
            .explain("EXPLAIN SELECT * FROM t WHERE a = 1")
            .expect("explain con filtro")
    };
    assert!(
        small_plan.iter().any(|line| line.contains("IndexScan")),
        "con índice el plan usa IndexScan: {small_plan:?}"
    );
    assert!(
        !small_plan.iter().any(|line| line.contains("full-scan")),
        "con índice no hay aviso: {small_plan:?}"
    );
}

/// Inserta `count` filas `(a=i, b="r{i}")` en `t`.
fn seed_rows(database: &mut Database, count: i64) {
    for index in 0..count {
        let scalars = ScalarMap::from([
            ("a".to_string(), ScalarValue::Int(index)),
            ("b".to_string(), ScalarValue::Text(format!("r{index}"))),
        ]);
        database.insert("t", scalars).expect("insert");
    }
}
