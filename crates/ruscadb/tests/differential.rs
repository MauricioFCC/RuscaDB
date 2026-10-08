//! Differential testing del executor (SPEC-0045).
//!
//! Compara el resultado de `Database::execute` contra un **oráculo en memoria**
//! que evalúa la semántica SQL de `SELECT`/`WHERE`/`ORDER BY`/`LIMIT`/proyección
//! sobre filas conocidas. Un generador determinista (semilla fija) produce
//! datasets y queries; la suite verifica 0 divergencias en 200 casos (dual
//! verification, CPD).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use std::cmp::Ordering;

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Record, RecordId, RecordMeta, Row,
    ScalarMap, ScalarValue,
};

/// Semilla fija del generador (reproducibilidad total de la suite).
const SEED: u64 = 0x5EED_1234_ABCD_0001;

/// Número de casos del lote diferencial (AC-0045-05).
const BATCH_CASES: usize = 200;

/// Generador pseudoaleatorio determinista (xorshift64*).
struct Rng(u64);

impl Rng {
    /// Crea el generador con una semilla fija.
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// Devuelve el siguiente `u64` de la secuencia.
    fn next_u64(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        self.0 = state;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Devuelve un valor en `0..upper` (`upper > 0`).
    fn next_below(&mut self, upper: u64) -> u64 {
        self.next_u64() % upper
    }

    /// Devuelve un booleano equilibrado.
    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// Operador de comparación del oráculo.
#[derive(Clone, Copy)]
enum Op {
    /// `=`.
    Eq,
    /// `!=`.
    NotEq,
    /// `<`.
    Lt,
    /// `<=`.
    LtEq,
    /// `>`.
    Gt,
    /// `>=`.
    GtEq,
}

impl Op {
    /// Símbolo textual del operador.
    fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::LtEq => "<=",
            Self::Gt => ">",
            Self::GtEq => ">=",
        }
    }
}

/// Predicado `a OP valor` del oráculo.
#[derive(Clone, Copy)]
struct Predicate {
    /// Operador de comparación.
    op: Op,
    /// Valor de referencia.
    value: i64,
}

impl Predicate {
    /// Evalúa el predicado sobre una fila con semántica SQL (`NULL` excluye).
    fn matches(&self, row: &RowData) -> bool {
        let Some(value) = row.a else {
            return false;
        };
        match self.op {
            Op::Eq => value == self.value,
            Op::NotEq => value != self.value,
            Op::Lt => value < self.value,
            Op::LtEq => value <= self.value,
            Op::Gt => value > self.value,
            Op::GtEq => value >= self.value,
        }
    }

    /// Texto SQL del predicado.
    fn sql(&self) -> String {
        format!("a {} {}", self.op.as_str(), self.value)
    }
}

/// Fila del dataset (escalar `a` anulable y texto `b`).
struct RowData {
    /// Valor de `a` (`None` = `NULL`).
    a: Option<i64>,
    /// Valor de `b`.
    b: String,
}

/// Caso diferencial: dataset + consulta.
struct Case {
    /// Filas del dataset.
    rows: Vec<RowData>,
    /// Predicados del `WHERE` (conjunción; vacío = sin filtro).
    filter: Vec<Predicate>,
    /// `true` ⇒ `SELECT a`; `false` ⇒ `SELECT *`.
    only_a: bool,
    /// `true` ⇒ `ORDER BY a`.
    order: bool,
    /// `true` ⇒ orden descendente.
    desc: bool,
    /// `LIMIT` opcional.
    limit: Option<u64>,
}

impl Case {
    /// Construye el texto RQL del caso sobre `table`.
    fn sql(&self, table: &str) -> String {
        let projection = if self.only_a { "a" } else { "*" };
        let filter = if self.filter.is_empty() {
            String::new()
        } else {
            let predicates: Vec<String> = self.filter.iter().map(Predicate::sql).collect();
            format!(" WHERE {}", predicates.join(" AND "))
        };
        let order = if self.order {
            format!(" ORDER BY a{}", if self.desc { " DESC" } else { "" })
        } else {
            String::new()
        };
        let limit = self
            .limit
            .map_or(String::new(), |value| format!(" LIMIT {value}"));
        format!("SELECT {projection} FROM {table}{filter}{order}{limit}")
    }
}

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura de la base");
    (dir, database)
}

/// Crea `t(a INT, b TEXT)` e inserta `rows` en un solo commit.
fn build_table(database: &mut Database, table: &str, rows: &[RowData]) {
    database
        .create_table(
            table,
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
    let records: Vec<Record> = rows
        .iter()
        .map(|row| Record {
            id: RecordId::new(),
            scalars: ScalarMap::from([
                (
                    "a".to_string(),
                    row.a.map_or(ScalarValue::Null, ScalarValue::Int),
                ),
                ("b".to_string(), ScalarValue::Text(row.b.clone())),
            ]),
            doc: None,
            edges: EdgeSet::default(),
            vector: None,
            blob: None,
            meta: RecordMeta::default(),
        })
        .collect();
    database
        .insert_many(table, records)
        .expect("insert_many del dataset");
}

/// Oráculo en memoria: evalúa el caso sobre sus filas.
fn oracle(case: &Case) -> Vec<Row> {
    let mut selected: Vec<&RowData> = case
        .rows
        .iter()
        .filter(|row| case.filter.iter().all(|predicate| predicate.matches(row)))
        .collect();
    if case.order {
        selected.sort_by(|left, right| compare_a(left.a, right.a, case.desc));
    }
    if let Some(limit) = case.limit {
        selected.truncate(limit as usize);
    }
    selected
        .iter()
        .map(|row| project(row, case.only_a))
        .collect()
}

/// Compara dos valores de `a` con `NULL` al final en `ASC` (y al principio en `DESC`).
fn compare_a(left: Option<i64>, right: Option<i64>, descending: bool) -> Ordering {
    let ordering = match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(first), Some(second)) => first.cmp(&second),
    };
    if descending {
        ordering.reverse()
    } else {
        ordering
    }
}

/// Proyecta una fila del oráculo como [`Row`] (`a` y, salvo `only_a`, `b`).
fn project(row: &RowData, only_a: bool) -> Row {
    let mut out = Row::new();
    out.insert(
        "a".to_string(),
        row.a.map_or(ScalarValue::Null, ScalarValue::Int),
    );
    if !only_a {
        out.insert("b".to_string(), ScalarValue::Text(row.b.clone()));
    }
    out
}

/// Ejecuta el caso en la base y lo compara contra el oráculo.
fn assert_case(database: &mut Database, table: &str, case: &Case, label: &str) {
    let sql = case.sql(table);
    let got = database.execute(&sql).expect("execute");
    let expected = oracle(case);
    assert_eq!(got, expected, "divergencia ({label}) en la consulta: {sql}");
}

/// Genera una fila aleatoria con `a` anulable y `b` de pocas variantes.
fn random_row(rng: &mut Rng) -> RowData {
    let a = if rng.next_below(4) == 0 {
        None
    } else {
        Some(rng.next_below(7) as i64 - 3)
    };
    RowData {
        a,
        b: format!("v{}", rng.next_below(4)),
    }
}

/// Genera un predicado aleatorio sobre `a`.
fn random_predicate(rng: &mut Rng) -> Predicate {
    let op = match rng.next_below(6) {
        0 => Op::Eq,
        1 => Op::NotEq,
        2 => Op::Lt,
        3 => Op::LtEq,
        4 => Op::Gt,
        _ => Op::GtEq,
    };
    Predicate {
        op,
        value: rng.next_below(7) as i64 - 3,
    }
}

/// Genera un caso diferencial aleatorio (dataset + consulta).
fn random_case(rng: &mut Rng) -> Case {
    let size = 1 + rng.next_below(10) as usize;
    let rows = (0..size).map(|_| random_row(rng)).collect();
    let filter_len = rng.next_below(3) as usize;
    let filter = (0..filter_len).map(|_| random_predicate(rng)).collect();
    let limit = if rng.next_below(3) == 0 {
        Some(rng.next_below(15))
    } else {
        None
    };
    Case {
        rows,
        filter,
        only_a: rng.next_bool(),
        order: rng.next_bool(),
        desc: rng.next_bool(),
        limit,
    }
}

/// Dataset fijo con `NULL`, duplicados y negativos para los casos dirigidos.
fn mixed_rows() -> Vec<RowData> {
    vec![
        RowData {
            a: Some(3),
            b: "v0".to_string(),
        },
        RowData {
            a: None,
            b: "v1".to_string(),
        },
        RowData {
            a: Some(-1),
            b: "v0".to_string(),
        },
        RowData {
            a: Some(3),
            b: "v2".to_string(),
        },
        RowData {
            a: Some(0),
            b: "v1".to_string(),
        },
        RowData {
            a: None,
            b: "v3".to_string(),
        },
        RowData {
            a: Some(2),
            b: "v2".to_string(),
        },
    ]
}

/// AC-0045-01 — scan/filter: el resultado coincide con el oráculo.
#[test] // @spec AC-0045-01
fn test_ac_0045_01_differential_scan_filter() {
    let (_dir, mut database) = open_test_db("ac0045_01");
    let rows = mixed_rows();
    build_table(&mut database, "t", &rows);

    let filters = [
        vec![],
        vec![Predicate {
            op: Op::Eq,
            value: 3,
        }],
        vec![Predicate {
            op: Op::NotEq,
            value: 3,
        }],
        vec![Predicate {
            op: Op::Lt,
            value: 1,
        }],
        vec![Predicate {
            op: Op::GtEq,
            value: 0,
        }],
        vec![
            Predicate {
                op: Op::Gt,
                value: -2,
            },
            Predicate {
                op: Op::Lt,
                value: 3,
            },
        ],
    ];
    for (index, filter) in filters.into_iter().enumerate() {
        let case = Case {
            rows: mixed_rows(),
            filter,
            only_a: false,
            order: false,
            desc: false,
            limit: None,
        };
        assert_case(&mut database, "t", &case, &format!("scan {index}"));
    }
}

/// AC-0045-02 — `ORDER BY a [ASC|DESC]` coincide con el oráculo (con `NULL`).
#[test] // @spec AC-0045-02
fn test_ac_0045_02_differential_order_by() {
    let (_dir, mut database) = open_test_db("ac0045_02");
    build_table(&mut database, "t", &mixed_rows());

    for desc in [false, true] {
        for filter in [
            vec![],
            vec![Predicate {
                op: Op::GtEq,
                value: 0,
            }],
        ] {
            let case = Case {
                rows: mixed_rows(),
                filter,
                only_a: false,
                order: true,
                desc,
                limit: None,
            };
            assert_case(
                &mut database,
                "t",
                &case,
                if desc { "order desc" } else { "order asc" },
            );
        }
    }
}

/// AC-0045-03 — `LIMIT` y proyección coinciden con el oráculo (incluye 0).
#[test] // @spec AC-0045-03
fn test_ac_0045_03_differential_limit_projection() {
    let (_dir, mut database) = open_test_db("ac0045_03");
    build_table(&mut database, "t", &mixed_rows());

    for only_a in [true, false] {
        for limit in [Some(0), Some(1), Some(3), Some(100), None] {
            let case = Case {
                rows: mixed_rows(),
                filter: vec![],
                only_a,
                order: true,
                desc: false,
                limit,
            };
            assert_case(
                &mut database,
                "t",
                &case,
                &format!("limit {limit:?} only_a={only_a}"),
            );
        }
    }
}

/// AC-0045-04 — la semántica de `NULL` coincide en filtros y órdenes.
#[test] // @spec AC-0045-04
fn test_ac_0045_04_differential_nulls() {
    let (_dir, mut database) = open_test_db("ac0045_04");
    build_table(&mut database, "t", &mixed_rows());

    let filters = [
        vec![Predicate {
            op: Op::Eq,
            value: 0,
        }],
        vec![Predicate {
            op: Op::NotEq,
            value: 0,
        }],
        vec![Predicate {
            op: Op::Lt,
            value: 0,
        }],
        vec![Predicate {
            op: Op::GtEq,
            value: 3,
        }],
    ];
    for filter in filters {
        let case = Case {
            rows: mixed_rows(),
            filter,
            only_a: true,
            order: true,
            desc: false,
            limit: None,
        };
        assert_case(&mut database, "t", &case, "null semantics");
    }
}

/// AC-0045-05 — 200 casos aleatorios (semilla fija) sin divergencias.
#[test] // @spec AC-0045-05
fn test_ac_0045_05_differential_batch() {
    let (_dir, mut database) = open_test_db("ac0045_05");
    let mut rng = Rng::new(SEED);
    for index in 0..BATCH_CASES {
        let case = random_case(&mut rng);
        let table = format!("t{index}");
        build_table(&mut database, &table, &case.rows);
        assert_case(&mut database, &table, &case, &format!("batch {index}"));
    }
}
