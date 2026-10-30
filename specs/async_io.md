---
id: SPEC-0151
feature: async_io
status: implemented
maturity: stub
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/async_io.rs]
  out_of_scope: [full async rewrite, tokio runtime integration]
fr:
  - { id: FR-0151-01, desc: "feature flag `async-io` (off por defecto) habilita I/O asincrono" }
  - { id: FR-0151-02, desc: "AsyncReader::read_async lee una pagina de forma asincrona" }
nf:
  - { id: NF-0151-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0151-01
    given: la feature `async-io` activada
    when: se lee una pagina de forma asincrona
    then: el resultado es correcto
    test: test_ac_0151_01_async_read
exit_criteria:
  - cargo test -p ruscadb --all-features --test async_io
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0151 â€” I/O asÃ­ncrono (tokio integration)

## Contexto

Frontier 2026 (Turso, tokio): I/O asÃ­ncrono para alta concurrencia.
RuscaDB usa I/O sÃ­ncrono (std::fs). Esta spec aÃ±ade I/O asÃ­ncrono bÃ¡sico
detrÃ¡s de un feature flag.

## Superficie

- Feature `async-io` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/async_io.rs` â€” `AsyncReader` con `read_async`.
- IntegraciÃ³n futura: tokio runtime, async Database API.

## Estado del arte

- **Turso/libSQL**: async I/O con tokio.
- **tokio**: runtime multi-thread para I/O asÃ­ncrono.
- **RuscaDB actual**: I/O sÃ­ncrono (std::fs).

## DiseÃ±o propuesto

1. **AsyncReader**: lee pÃ¡ginas de forma asÃ­ncrona.
2. **AsyncWriter**: escribe pÃ¡ginas de forma asÃ­ncrona.
3. **Runtime**: integraciÃ³n con tokio (futuro).

## Notas

- ImplementaciÃ³n inicial: simulaciÃ³n sÃ­ncrona (sin tokio).
- IntegraciÃ³n real con tokio queda como trabajo futuro.
---
