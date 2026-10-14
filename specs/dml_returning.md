---
id: SPEC-0102
feature: dml_returning
status: implemented
owner: query-team
appetite_days: 2
boundaries:
  crates: [ruscadb-query, ruscadb]
  files: [crates/ruscadb-query/src/lexer.rs, crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/src/dml.rs, crates/ruscadb/src/executor.rs]
  out_of_scope: [UPSERT/MERGE, RETURNING con expresiones/agregados, RETURNING de columnas inexistentes (error de esquema), ORDER BY sobre RETURNING]
fr:
  - { id: FR-0102-01, desc: "INSERT/UPDATE/DELETE aceptan RETURNING *|<cols> con canonizacion Display y roundtrip" }
  - { id: FR-0102-02, desc: "el executor devuelve las filas afectadas (insertadas/actualizadas/borradas) en lugar de {affected:N} cuando hay RETURNING" }
nf:
  - { id: NF-0102-01, desc: "sin RETURNING el contrato {affected:N} no cambia (cero regresion); con RETURNING las columnas se validan contra el esquema" }
  - { id: NF-0102-02, desc: "DELETE RETURNING sin coincidencias devuelve 0 filas (sin error); RETURNING mal formado es ParseError accionable" }
acceptance_criteria:
  - id: AC-0102-01
    given: una tabla `t(id INT, name TEXT)`
    when: se ejecuta INSERT ... RETURNING id, name (y RETURNING *)
    then: devuelve una fila por fila insertada con las columnas pedidas, sin `affected`
    test: test_ac_0102_01_insert_returning_rows
  - id: AC-0102-02
    given: filas sembradas
    when: se ejecuta UPDATE ... RETURNING y DELETE ... RETURNING
    then: devuelven las filas afectadas (valores nuevos en UPDATE, valores borrados en DELETE)
    test: test_ac_0102_02_update_delete_returning_rows
  - id: AC-0102-03
    given: texto RQL con RETURNING bien y mal formado
    when: se parsea y re-serializa
    then: el valido canoniza (`RETURNING *`/`RETURNING a, b`) y hace roundtrip; sin columnas es ParseError
    test: test_ac_0102_03_returning_parses_and_displays
exit_criteria:
  - cargo test -p ruscadb --test dml_returning
  - cargo test -p ruscadb-query
  - cargo xtask trace / contract verdes
rollback:
  - retirar la palabra clave RETURNING, el campo returning y project_records
sandbox:
  - cargo test -p ruscadb --test dml_returning
---

# SPEC-0102 — `RETURNING` en DML

## Contexto

Límite del MVP: «DML sin `UPSERT`/`MERGE`/`RETURNING`». Se implementa
`RETURNING` (SQL:2016) en `INSERT`/`UPDATE`/`DELETE`: en lugar del contador
`{"affected": N}`, la sentencia devuelve las filas afectadas con las columnas
pedidas (`RETURNING *` o una lista).

## Superficie

- `ruscadb-query`: `Keyword::Returning`, campo `returning` en `Insert`/`Update`/
  `Delete`, parseo tras la sentencia y `Display` canónico.
- `ruscadb`: `execute_insert`/`execute_update`/`execute_delete` devuelven filas
  proyectadas con `project_row` (compartido con `SELECT`).

## Criterios de aceptación

- **AC-0102-01** — `INSERT ... RETURNING` devuelve las filas insertadas.
- **AC-0102-02** — `UPDATE`/`DELETE ... RETURNING` devuelven las afectadas.
- **AC-0102-03** — parseo/canonización y error ruidoso.

## Trazabilidad

Tests `test_ac_0102_<nn>_*` con marcador `@spec AC-0102-NN`; verificado por
`cargo xtask trace`/`contract`.
