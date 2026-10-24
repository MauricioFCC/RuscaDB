---
id: SPEC-0127
feature: pgwire_real
status: implemented
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/pgwire_real.rs]
  out_of_scope: [autenticacion SCRAM, replicacion logica, SSL/TLS, full SQL]
fr:
  - { id: FR-0127-01, desc: "feature flag `pgwire-real` (off por defecto) habilita pgwire real" }
  - { id: FR-0127-02, desc: "PgWireRealServer traduce RQL a respuestas pgwire" }
nf:
  - { id: NF-0127-01, desc: "pgwire crate opcional detras del flag" }
acceptance_criteria:
  - id: AC-0127-01
    given: la feature `pgwire-real` activada
    when: se crea un PgWireRealServer
    then: la creacion no paniquea
    test: test_ac_0127_01_pgwire_real_create
exit_criteria:
  - cargo test -p ruscadb --all-features --test pgwire_real
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0127 — PostgreSQL wire protocol (pgwire real)

## Contexto

Frontier 2026 (pglite-oxide, libsql): compatibilidad con el ecosistema PostgreSQL.
RuscaDB tiene un stub pgwire (SPEC-0113). Esta spec añade integración real
detrás de un feature flag (pgwire crate).

## Superficie

- Feature `pgwire-real` en `ruscadb` (off por defecto).
- Dependencia opcional `pgwire = "0.42"`.
- `crates/ruscadb/src/pgwire_real.rs` — `PgWireRealServer` con `new`.
- Integración futura: autenticación SCRAM, SSL/TLS, full SQL.

## Notas

- Implementación inicial: stub sin dependencia de pgwire crate.
- La integración real con pgwire queda como trabajo futuro.
---
