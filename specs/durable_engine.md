---
id: SPEC-0004
feature: durable_engine
status: accepted
owner: core-team
appetite_days: 10
boundaries:
  crates: [ruscadb, ruscadb-storage, ruscadb-wal]
  out_of_scope: [query, vector, graph, bindings, ai]
fr:
  - { id: FR-0004-01, desc: "Database compone PagedFile + BufferPool + Wal (composition root)" }
  - { id: FR-0004-02, desc: "commit es WAL-first: append + fsync antes de publicar páginas" }
  - { id: FR-0004-03, desc: "open ejecuta replay idempotente de los commit records" }
nf:
  - { id: NF-0004-01, desc: "durabilidad: todo commit ack'd sobrevive a reopen" }
  - { id: NF-0004-02, desc: "recovery O(bytes de WAL), sin recompilación ni red" }
acceptance_criteria:
  - id: AC-0004-01
    given: una página escrita y commiteada
    when: se cierra y reabre la base
    then: la página se lee con su contenido (durabilidad)
    test: test_ac_0004_01_commit_survives_reopen
  - id: AC-0004-02
    given: un commit con el archivo de datos perdido
    when: se reabre la base
    then: el replay del WAL restaura las páginas (crash antes de flush)
    test: test_ac_0004_02_recovery_replays_committed_pages
  - id: AC-0004-03
    given: un WAL con cola rasgada tras un commit
    when: se reabre la base
    then: la cola se trunca y el commit válido se recupera
    test: test_ac_0004_03_torn_wal_tail_is_truncated
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0004
  - cargo mutants -p ruscadb mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir crates/ruscadb a la fachada vacía (sin Database)
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0004 — Motor de almacenamiento durable

## Contexto

Integra `PagedFile` + `BufferPool` (SPEC-0003) + `Wal` (SPEC-0002) en un
`Database` (composition root de la fachada). Política **WAL-first**: el commit
fsync'ea el WAL antes de publicar páginas; las páginas sucias no se desalojan
antes de su registro WAL, por lo que el presupuesto debe cubrir el working set
sucio entre commits.

## Criterios de aceptación

- **AC-0004-01** — durabilidad tras reopen.
- **AC-0004-02** — replay restaura páginas perdidas (crash antes de flush).
- **AC-0004-03** — cola rasgada del WAL se trunca sin perder el prefijo válido.

## Trazabilidad

Tests `test_ac_0004_<nn>_*` + property tests; verificado por `cargo xtask trace`.
