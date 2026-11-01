---
id: SPEC-0183
feature: sql_three_valued_logic
status: implemented
maturity: real
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-query, ruscadb-wasm]
  files:
    - crates/ruscadb-query/src/lexer.rs
    - crates/ruscadb-query/src/ast.rs
    - crates/ruscadb-query/src/parser.rs
    - crates/ruscadb-query/src/limits.rs
    - crates/ruscadb/src/executor/eval.rs
    - crates/ruscadb/src/executor/rows.rs
  out_of_scope: [subqueries, OR sobre indices bitmap, NOT EXISTS]
fr:
  - { id: FR-0183-01, desc: "Logica SQL de tres valores y DISTINCT lineal" }
nf:
  - { id: NF-0183-01, desc: "Sin dependencias nuevas; std only" }
acceptance_criteria:
  - id: AC-0183-01
    given: una tabla con enteros y NULL
    when: se filtra con OR, NOT y precedencia SQL
    then: solo las filas con resultado TRUE pasan
    test: test_ac_0183_01_or_not_precedence
  - id: AC-0183-02
    given: valores NULL en columnas
    when: se evalua = NULL, IS NULL e IS NOT NULL
    then: se aplica Kleene (UNKNOWN no pasa WHERE)
    test: test_ac_0183_02_null_semantics
  - id: AC-0183-03
    given: una proyeccion con filas repetidas
    when: se ejecuta SELECT DISTINCT
    then: deduplica por clave canonica en O(n log n)
    test: test_ac_0183_03_distinct_linear
exit_criteria:
  - cargo test -p ruscadb --all-features --test spec_0183_0184
  - cargo test -p ruscadb-query --lib
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir lexer/ast/parser/eval/rows
sandbox:
  - cargo test -p ruscadb --all-features --test spec_0183_0184
---

# SPEC-0183 — Lógica SQL de tres valores y DISTINCT lineal

## Contexto

RQL no tenía `OR`/`NOT`/`IS NULL`: todo `WHERE` era una conjunción de
comparaciones, y `NULL` se trataba como `false` (ni siquiera se distinguía
`UNKNOWN`). Además `SELECT DISTINCT` deduplicaba con `Vec::contains`, O(n²).

## Semántica (Kleene / SQL:2023)

Sea `φ: Row → {T, F, U}`. Las tablas de verdad son:

| `x AND y` | T | F | U |
|---|---|---|---|
| **T** | T | F | U |
| **F** | F | F | F |
| **U** | U | F | U |

| `x OR y` | T | F | U |
|---|---|---|---|
| **T** | T | T | T |
| **F** | T | F | U |
| **U** | T | U | U |

`NOT U = U`. `WHERE φ` conserva la fila si y solo si `φ(row) = T`
(`F` y `U` se descartan). Las comparaciones con `NULL` producen `U`; `IS NULL`
nunca produce `U`.

**Complejidad**: evaluar una fila cuesta `O(|φ|)` (AST) y el filtro `O(n·|φ|)`,
sin productos cartesianos. `DISTINCT` usa una clave canónica `κ(r)` y un
`BTreeSet`: `O(n·k·log n)` frente al `O(n²·k)` anterior (`k` = ancho de fila).

## Referencias

- ISO/IEC 9075-2:2023 (SQL/Foundation), semántica de `NULL` y `WHERE`.
- Codd, E. F. (1979), "Extending the database relational model to capture more
  meaning", ACM TODS — origen del `NULL` relacional.
- Kleene, S. C. (1952), *Introduction to Metamathematics* — lógica de tres
  valores.
- PostgreSQL 18, "Three-Valued Logic" y `IS NULL` (documentación oficial).
