---
id: SPEC-0113
feature: pgwire_protocol
status: implemented
maturity: stub
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/pgwire.rs]
  out_of_scope: [autenticacion SCRAM, replicacion logica, SSL/TLS]
fr:
  - { id: FR-0113-01, desc: "feature flag `pgwire` (off por defecto) habilita el protocolo PostgreSQL wire" }
  - { id: FR-0113-02, desc: "PgWireServer::serve escucha en un puerto TCP y responde a queries simples" }
nf:
  - { id: NF-0113-01, desc: "cero dependencias nuevas en el build por defecto; pgwire crate opcional" }
acceptance_criteria:
  - id: AC-0113-01
    given: la feature `pgwire` activada
    when: se crea un PgWireServer
    then: la creacion no paniquea
    test: test_ac_0113_01_pgwire_server_create
exit_criteria:
  - cargo test -p ruscadb --all-features --test pgwire
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0113 â€” Protocolo PostgreSQL wire (pgwire)

## Contexto

Frontier 2026 (pglite-oxide, libsql): compatibilidad con el ecosistema PostgreSQL
permite usar drivers estÃ¡ndar (psql, JDBC, SQLx, Prisma). RuscaDB tiene RQL propio.
Esta spec aÃ±ade un servidor pgwire opcional detrÃ¡s de un feature flag.

## Superficie

- Feature `pgwire` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/pgwire.rs` â€” `PgWireServer` con `new` + `serve`.
- TraducciÃ³n RQLâ†’SQL bÃ¡sico (SELECT/INSERT/UPDATE/DELETE).

## Notas

- ImplementaciÃ³n inicial: stub sin dependencia de `pgwire` crate.
- La integraciÃ³n real con `pgwire` queda como trabajo futuro.
---
