---
id: SPEC-0043
feature: dml_statements
status: accepted
owner: query-team
appetite_days: 8
boundaries:
  crates: [ruscadb-query, ruscadb]
  out_of_scope: [UPSERT, MERGE, subconsultas, RETURNING]
fr:
  - { id: FR-0043-01, desc: "parsear INSERT INTO t (cols) VALUES (fila)[, (fila)...]" }
  - { id: FR-0043-02, desc: "parsear UPDATE t SET col = lit[, ...] [WHERE ...]" }
  - { id: FR-0043-03, desc: "parsear DELETE FROM t [WHERE ...]" }
  - { id: FR-0043-04, desc: "ejecutar DML sobre Database y devolver filas afectadas" }
  - { id: FR-0043-05, desc: "Display/roundtrip de las tres formas" }
nf:
  - { id: NF-0043-01, desc: "sin regresion en SELECT; errores accionables (tabla/columna/valor)" }
  - { id: NF-0043-02, desc: "INSERT multi-fila = un solo commit (usa insert_many)" }
acceptance_criteria:
  - id: AC-0043-01
    given: "INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'y')"
    when: se ejecuta
    then: inserta 2 filas y devuelve affected=2
    test: test_ac_0043_01_insert_statement
  - id: AC-0043-02
    given: "UPDATE t SET b = 'z' WHERE a = 1"
    when: se ejecuta
    then: actualiza las filas que cumplen y devuelve affected
    test: test_ac_0043_02_update_statement
  - id: AC-0043-03
    given: "DELETE FROM t WHERE a = 2"
    when: se ejecuta
    then: borra las filas que cumplen y devuelve affected
    test: test_ac_0043_03_delete_statement
  - id: AC-0043-04
    given: una sentencia DML mal formada
    when: se parsea/ejecuta
    then: error accionable (con posicion o columna/tabla)
    test: test_ac_0043_04_dml_errors_are_actionable
  - id: AC-0043-05
    given: las tres formas
    when: se hace Display y se reparsean
    then: el IR es igual (roundtrip)
    test: test_ac_0043_05_display_parse_roundtrip_dml
exit_criteria:
  - cargo test -p ruscadb-query -p ruscadb -- test_ac_0043
  - cargo mutants -p ruscadb-query mutation score >= 70%
rollback:
  - revertir query + fachada
sandbox:
  - cargo test -p ruscadb-query -p ruscadb
---

# SPEC-0043 — DML en RQL (INSERT/UPDATE/DELETE)

## Contexto

Hoy las escrituras son métodos de la fachada (`insert`, `delete`), no lenguaje.
Se añaden las tres sentencias DML al parser/IR (`Statement::Insert/Update/Delete`)
y su ejecución en la fachada: `INSERT` multi-fila usa `insert_many` (un commit);
`UPDATE` reescribe filas que cumplen el `WHERE`; `DELETE` usa el borrado lógico
MVCC (`delete`). `Database::execute` devuelve, para DML, una fila
`{"affected": N}` (documentado).

## Criterios de aceptación

- **AC-0043-01..03** — INSERT/UPDATE/DELETE con conteo.
- **AC-0043-04/05** — errores y roundtrip.

## Trazabilidad

Tests `test_ac_0043_<nn>_*`; verificado por `cargo xtask trace`.
