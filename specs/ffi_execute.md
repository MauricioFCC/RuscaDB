---
id: SPEC-0026
feature: ffi_execute
status: accepted
owner: platform-team
appetite_days: 5
boundaries:
  crates: [ruscadb-ffi, ruscadb]
  out_of_scope: [streaming de resultados, callbacks, zero-copy Arrow]
fr:
  - { id: FR-0026-01, desc: "ruscadb_execute(handle, sql, out_buf, buf_len) ejecuta RQL y devuelve JSON" }
  - { id: FR-0026-02, desc: "el resultado JSON es un array de filas (objeto columna->valor)" }
  - { id: FR-0026-03, desc: "errores por el codigo de retorno + russcadb_last_error (sin panics)" }
  - { id: FR-0026-04, desc: "los wrappers Python/Node exponen execute(sql) -> JSON" }
  - { id: FR-0026-05, desc: "buffer insuficiente devuelve el tamano requerido (semantica de truncado)" }
nf:
  - { id: NF-0026-01, desc: "catch_unwind en la frontera; ningun panic cruza el ABI" }
  - { id: NF-0026-02, desc: "validacion de (ptr,len) antes de todo acceso" }
acceptance_criteria:
  - id: AC-0026-01
    given: un handle abierto y una tabla con filas
    when: se llama russcadb_execute con "SELECT * FROM t"
    then: devuelve RC_OK y un JSON con las filas
    test: test_ac_0026_01_execute_returns_json
  - id: AC-0026-02
    given: un handle abierto
    when: se ejecuta una query invalida
    then: devuelve un codigo de error y russcadb_last_error lo describe
    test: test_ac_0026_02_invalid_query_is_error
  - id: AC-0026-03
    given: una query valida y un buffer pequeno
    when: se llama russcadb_execute
    then: no desborda y comunica el tamano requerido
    test: test_ac_0026_03_small_buffer_reports_size
  - id: AC-0026-04
    given: un handle invalido o nulo
    when: se llama russcadb_execute
    then: devuelve RC_INVALID_HANDLE / RC_NULL_POINTER sin panics
    test: test_ac_0026_04_invalid_handle_is_error
  - id: AC-0026-05
    given: la API de wrappers
    when: se inspeccionan python/node
    then: exponen execute(sql) que devuelve JSON
    test: test_ac_0026_05_wrappers_expose_execute
exit_criteria:
  - cargo test -p ruscadb-ffi -- test_ac_0026
  - cargo mutants -p ruscadb-ffi mutation score >= 70%
rollback:
  - revertir russcadb-ffi al commit previo
sandbox:
  - cargo test -p ruscadb-ffi
---

# SPEC-0026 — `ruscadb_execute` en el C-ABI + wrappers

## Contexto

Completa F5: hoy el C-ABI solo expone operaciones de página; los wrappers
Python/Node no pueden consultar. Se añade:

- `ruscadb_execute(handle, sql, out_buf, buf_len) -> c_int`: parsea/ejecuta RQL
  vía la fachada (`Database::execute`) y serializa `Vec<Row>` a JSON con
  `serde_json`; copia el JSON a `out_buf` (sin desbordar) y devuelve `RC_OK`, o
  el tamaño requerido si el buffer es pequeño (documentado), o un código de error.
- `bindings/python/ruscadb.py` y `bindings/node/ruscadb.js`: método `execute(sql)`
  que devuelve el JSON.

## Criterios de aceptación

- **AC-0026-01..04** — ejecución, errores, buffer pequeño, handle inválido.
- **AC-0026-05** — wrappers con `execute`.

## Trazabilidad

Tests `test_ac_0026_<nn>_*`; verificado por `cargo xtask trace`.
