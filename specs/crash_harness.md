---
id: SPEC-0056
feature: crash_harness
status: implemented
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb-wal, ruscadb]
  out_of_scope: [fault injection en produccion, red/disco simulado, 10k puntos T3]
fr:
  - { id: FR-0056-01, desc: "hook de fault-injection solo-test en fases F1/F2/F3 del commit" }
  - { id: FR-0056-02, desc: "harness con proceso hijo + SIGKILL en cada punto de fault" }
  - { id: FR-0056-03, desc: "reopen con replay idempotente: post-fsync presente exacto, pre-fsync invisible" }
  - { id: FR-0056-04, desc: "tail truncada con CRC invalido se descarta sin corromper el prefijo" }
nf:
  - { id: NF-0056-01, desc: "el hook es 0-cost y ausente en build normal (cfg test)" }
  - { id: NF-0056-02, desc: "harness acotado en tiempo (< 5 min en CI)" }
acceptance_criteria:
  - id: AC-0056-01
    given: un commit con SIGKILL antes del fsync (F2)
    when: se reabre y se hace replay
    then: el commit es invisible y el estado previo intacto
    test: test_ac_0056_01_kill_before_fsync_invisible
  - id: AC-0056-02
    given: un commit con SIGKILL despues del fsync
    when: se reabre y se hace replay
    then: la fila existe exacta con blobs/vectores/metadatos (SI-1)
    test: test_ac_0056_02_kill_after_fsync_durable
  - id: AC-0056-03
    given: un WAL con tail truncada
    when: se invoca recovery
    then: se trunca al ultimo frame CRC-valido, 0 perdidas ack'd
    test: test_ac_0056_03_truncated_tail_discarded
  - id: AC-0056-04
    given: un WAL ya recuperado
    when: se repite el replay
    then: replay(replay(w)) == replay(w) (I3)
    test: test_ac_0056_04_replay_idempotent
exit_criteria:
  - cargo test -p ruscadb-wal -p ruscadb -- test_ac_0056
  - 0 perdidas en la matriz puntos(F1/F2/F3) x fases
  - cargo mutants -p ruscadb-wal acotado MS >= 70%
rollback:
  - revertir wal + fachada (hook cfg(test), sin efecto en release)
sandbox:
  - cargo test -p ruscadb-wal
---

# SPEC-0056 — Harness crash-recovery con fault injection (I1/FF-12)

## Contexto

Canon WAL-en-Rust (arXiv:2507.13062): torn-write → truncar, nunca reparar;
replay idempotente desde `checkpoint_lsn`. El hook vive tras `cfg(test)`:
0 coste en release. Harness: hijo que ejecuta hasta el fault point, padre que
manda SIGKILL, reapertura y verificación de SI-1.

## Criterios de aceptación

- **AC-0056-01/02** — invisibilidad pre-fsync, durabilidad post-fsync.
- **AC-0056-03/04** — truncado de tail + idempotencia (I3).

## Trazabilidad

Tests `test_ac_0056_<nn>_*`; verificado por `cargo xtask trace`.
