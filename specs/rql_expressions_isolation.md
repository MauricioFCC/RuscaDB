---
id: SPEC-0182
feature: rql_expressions_isolation
status: implemented
maturity: real
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-query]
  files:
    - crates/ruscadb-query/src/lexer.rs
    - crates/ruscadb-query/src/ast.rs
    - crates/ruscadb-query/src/parser.rs
    - crates/ruscadb/src/executor.rs
    - crates/ruscadb/src/database.rs
    - crates/ruscadb/src/columnar.rs
  out_of_scope: [subqueries escalares, aritmetica general, IS NULL]
fr:
  - { id: FR-0182-01, desc: "CASE/booleanos en RQL y APIs P1 del consumidor" }
nf:
  - { id: NF-0182-01, desc: "Sin dependencias nuevas; std + crates del workspace" }
acceptance_criteria:
  - id: AC-0182-01
    given: una tabla con columnas numericas
    when: se agrega SUM/AVG/MIN/MAX sobre CASE
    then: el resultado coincide con el calculo sobre 3 filas
    test: test_ac_0182_01_case_in_aggregate
  - id: AC-0182-02
    given: una tabla con una columna booleana
    when: se filtra con = TRUE/FALSE, columna desnuda o IS NOT TRUE
    then: devuelve exactamente las filas esperadas
    test: test_ac_0182_02_boolean_literals
  - id: AC-0182-03
    given: una ColumnarTable con columnas INT/FLOAT/TEXT
    when: se escribe con write_rcol1 y se lee con read_rcol1
    then: round-trip exacto y bytes escritos > 0
    test: test_ac_0182_03_rcol1_roundtrip
  - id: AC-0182-04
    given: una base abierta
    when: se configura with_isolation(Serializable)
    then: isolation() lo refleja y SSI queda activo
    test: test_ac_0182_04_isolation_api
  - id: AC-0182-05
    given: una tabla relacional con escalares
    when: se inserta una fila con insert_with_embedding y se ejecuta KNN
    then: el vecino mas cercano es la fila insertada con ese embedding
    test: test_ac_0182_05_insert_with_vector_knn
exit_criteria:
  - cargo test -p ruscadb --all-features --test p0_p1_api
  - cargo test -p ruscadb-query --lib
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir lexer/ast/parser/executor/database/columnar
sandbox:
  - cargo test -p ruscadb --all-features --test p0_p1_api
---

# SPEC-0182 — Expresiones RQL y APIs P1 (peticiones Misellium 2026-10-31)

## Contexto

Peticiones medidas por el consumidor (`Misellium-Edge-AI`,
`docs/research/ruscadb-peticiones-2026-10-31.md`):

- **P0.1** `SUM(CASE ...)`: los agregados solo aceptaban columna.
- **P0.2** `WHERE closed = TRUE`: `TRUE` se interpretaba como columna.
- **P1.1** no había API pública `write_rcol1`/`read_rcol1`.
- **P1.2** no había setter de nivel de aislamiento.

## Diseño

- `Expr::Bool` y `Expr::Case { operand, branches, otherwise }`; el parser
  acepta CASE simple y buscado. `USING`/`IS`/`NOT` se reconocen como
  identificadores opcionales para no reservar palabras.
- `Aggregate.expr: Option<Expr>` permite `FUNC(expr)`; el tipo se infiere de las
  ramas (`Int`+`Float` → `Float`) y `SUM`/`AVG` exigen resultado numérico.
- `WHERE col` sobre `Bool` se evalúa como predicado; `IS [NOT] TRUE|FALSE` es
  azúcar de `= TRUE`/`!= TRUE`.
- `write_rcol1` devuelve los bytes escritos; `read_rcol1` reconstruye la tabla.
- `IsoLevel`: `ReadCommitted` refresca el snapshot por lectura; `Snapshot`
  (defecto) lo fija en `begin`; `Serializable` añade SSI.

## Coste

- CASE: `O(ramas)` por fila; el agregado sigue siendo `O(n)`.
- Escritura RCOL1: `O(n·d)` (codificación) y `O(bytes)` de I/O.
- Snapshot fijado: `O(1)` por transacción (clon del snapshot).
