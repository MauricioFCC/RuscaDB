---
id: SPEC-0002
feature: wal_durability
status: accepted
owner: storage-team
appetite_days: 10
boundaries:
  crates: [ruscadb-wal, ruscadb-core]
  out_of_scope: [storage, query, bindings, ai]
fr:
  - { id: FR-0002-01, desc: "Cada COMMIT emite un registro WAL con checksum CRC32C" }
  - { id: FR-0002-02, desc: "fsync (o group commit) obligatorio antes de ack al caller" }
  - { id: FR-0002-03, desc: "El recovery trunca el WAL en el primer frame con CRC invalido" }
  - { id: FR-0002-04, desc: "El replay del WAL desde checkpoint_lsn es idempotente" }
nf:
  - { id: NF-0002-01, desc: "commit P99 < 5 ms en NVMe con group commit" }
  - { id: NF-0002-02, desc: "recovery < 1 s por GB de WAL" }
acceptance_criteria:
  - id: AC-0002-01
    given: una base con un COMMIT confirmado
    when: el proceso es terminado con SIGKILL
    then: al reopen, la fila existe y su checksum valida
    test: test_ac_0002_01_wal_durability_after_sigkill
  - id: AC-0002-02
    given: un WAL con un frame truncado a mitad
    when: se invoca recovery
    then: el frame rasgado se descarta sin corromper el prefijo valido
    test: test_ac_0002_02_wal_recovery_truncated_tail
  - id: AC-0002-03
    given: un WAL valido
    when: se reaplica el replay dos veces
    then: el estado final es identico (idempotencia)
    test: test_ac_0002_03_wal_replay_is_idempotent
exit_criteria:
  - cargo test -p ruscadb-wal -- test_ac_0002
  - crash-injection 0 perdidas de commits ack'd
  - mutation score del diff >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - flag RUSCADB_FF_WAL_V2=0
  - down-migration documentada en specs/migrations/0002_down.md
sandbox:
  - cargo test -p ruscadb-wal
---

# SPEC-0002 — Durabilidad del WAL

## Contexto

El WAL global es el sustrato de durabilidad de RuscaDB: escribe un frame con
CRC32C y hace `fsync` (o group commit) **antes** de publicar el manifiesto.
Ver `docs/RuscaDB-roadmap.md` §5.3 y ADR-006.

Frame: `[ len u32 | lsn u64 | tx_id u64 | kind u8 | payload | crc32c u32 ]`.

## Criterios de aceptacion

- **AC-0002-01** — durabilidad tras crash (Invariante I1 / SI-1).
- **AC-0002-02** — torn write: truncar, nunca reparar (Invariante I10).
- **AC-0002-03** — replay idempotente (Invariante I3).

## Trazabilidad

Cada AC se implementa con un test cuyo nombre empieza por `test_ac_0002_<nn>_`
y se anota en el codigo con `// @spec AC-0002-<nn>`.
