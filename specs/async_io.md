---
id: SPEC-0151
feature: async_io
status: proposed
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

# SPEC-0151 — I/O asíncrono (tokio integration)

## Contexto

Frontier 2026 (Turso, tokio): I/O asíncrono para alta concurrencia.
RuscaDB usa I/O síncrono (std::fs). Esta spec añade I/O asíncrono básico
detrás de un feature flag.

## Superficie

- Feature `async-io` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/async_io.rs` — `AsyncReader` con `read_async`.
- Integración futura: tokio runtime, async Database API.

## Estado del arte

- **Turso/libSQL**: async I/O con tokio.
- **tokio**: runtime multi-thread para I/O asíncrono.
- **RuscaDB actual**: I/O síncrono (std::fs).

## Diseño propuesto

1. **AsyncReader**: lee páginas de forma asíncrona.
2. **AsyncWriter**: escribe páginas de forma asíncrona.
3. **Runtime**: integración con tokio (futuro).

## Notas

- Implementación inicial: simulación síncrona (sin tokio).
- Integración real con tokio queda como trabajo futuro.
---
