---
id: SPEC-0010
feature: ffi_bindings
status: accepted
owner: bindings-team
appetite_days: 8
boundaries:
  crates: [ruscadb-ffi, ruscadb, ruscadb-core]
  out_of_scope: [pyo3, napi, network]
fr:
  - { id: FR-0010-01, desc: "C-ABI estable: russcadb_open/read/write/commit/close/last_error" }
  - { id: FR-0010-02, desc: "handle table con generacion (anti use-after-free)" }
  - { id: FR-0010-03, desc: "catch_unwind en la frontera: ningun panic cruza el ABI" }
  - { id: FR-0010-04, desc: "validacion de (ptr,len) y de punteros nulos" }
nf:
  - { id: NF-0010-01, desc: "0 UB: miri/asan limpio en la suite FFI" }
  - { id: NF-0010-02, desc: "handle invalido devuelve error, nunca desreferencia" }
acceptance_criteria:
  - id: AC-0010-01
    given: un puntero a configuracion valido
    when: se abre y se cierra la base
    then: el handle es valido y se libera sin fugas
    test: test_ac_0010_01_open_and_close
  - id: AC-0010-02
    given: una base abierta por el ABI
    when: se escribe, se hace commit y se reabre
    then: la pagina se lee con su contenido
    test: test_ac_0010_02_write_commit_reopen
  - id: AC-0010-03
    given: un handle invalido o ya liberado
    when: se usa
    then: devuelve error (sin desreferenciar, sin panic)
    test: test_ac_0010_03_invalid_handle_is_error
  - id: AC-0010-04
    given: un puntero nulo o longitud inconsistente
    when: se pasa al ABI
    then: devuelve error (validacion de entrada)
    test: test_ac_0010_04_null_pointer_is_error
exit_criteria:
  - cargo test -p ruscadb-ffi -- test_ac_0010
  - cargo mutants -p ruscadb-ffi mutation score >= 70%
rollback:
  - revertir ruscadb-ffi al stub
sandbox:
  - cargo test -p ruscadb-ffi
---

# SPEC-0010 — Bindings C-ABI

## Contexto

Contrato FFI estable de RuscaDB (`docs/RuscaDB-roadmap.md` §6.2, ADR-008/011).
El C-ABI es la fuente única para todos los drivers; los wrappers Python/Node
se apoyan en él. `ruscadb-ffi` es el único crate autorizado a usar `unsafe`
(presupuesto en `unsafe-allowlist.toml`).

## Trazabilidad

Tests `test_ac_0010_<nn>_*`; verificado por `cargo xtask trace`.
