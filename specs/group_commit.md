---
id: SPEC-0081
feature: group_commit
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb-wal, ruscadb]
  files: [crates/ruscadb-wal/src/lib.rs, crates/ruscadb/src/database.rs, crates/ruscadb/tests/group_commit_crash.rs]
  out_of_scope: [multi-proceso con escritores concurrentes (el lock de SPEC-0076 es exclusivo), group commit del manifiesto aparte, FsyncPolicy para páginas de datos, compresión del WAL]
fr:
  - { id: FR-0081-01, desc: "FsyncPolicy{Full, Normal} configurable al abrir: Full (default) hace fsync del WAL antes de confirmar cada commit, como hoy" }
  - { id: FR-0081-02, desc: "Normal ejecuta group commit: los commits entrantes se agrupan (greedy) en un solo fsync con pipeline (se serializa el grupo N+1 mientras se sincroniza el N); el ack llega SOLO tras el fsync de su grupo" }
  - { id: FR-0081-03, desc: "la cuenta de fsyncs con Normal es estrictamente menor que la de commits con cargas agrupables (coalescing medible)" }
nf:
  - { id: NF-0081-01, desc: "crash-safety: todo commit acked sobrevive a kill -9 en cualquier instante; recovery truncado conserva el prefijo acknowledged completo" }
  - { id: NF-0081-02, desc: "sin dependencias nuevas (std::thread/canals); el writer thread se drena y une en drop/close sin deadlock" }
  - { id: NF-0081-03, desc: "default Full idéntico al comportamiento actual (sin cambio de semántica ni de tests previos)" }
acceptance_criteria:
  - id: AC-0081-01
    given: una base abierta con FsyncPolicy::Full y un proceso hijo que hace N commits confirmados
    when: el hijo muere tras recibir los N acks
    then: la reapertura recupera exactamente los N commits (cada ack es durable)
    test: test_ac_0081_01_full_policy_every_ack_is_durable
  - id: AC-0081-02
    given: una base con FsyncPolicy::Normal y una ráfaga de N commits
    when: se ejecuta la ráfaga y se consulta el contador de fsyncs
    then: fsyncs < N (coalescing) y tras reapertura los N acks están en el WAL
    test: test_ac_0081_02_normal_policy_groups_fsyncs
  - id: AC-0081-03
    given: un proceso hijo con Normal que commitea en bucle y se le hace kill en mitad de un grupo
    when: el padre recupera la base
    then: recovery limpio (prefijo acknowledged completo, tail truncado, sin corrupción de frames)
    test: test_ac_0081_03_kill_during_group_recovers_clean_prefix
exit_criteria:
  - cargo test -p ruscadb --test group_commit_crash
  - cargo test -p ruscadb-wal
  - cargo test --workspace --all-features   (sin regresión)
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar FsyncPolicy y el writer thread (vuelve a fsync síncrono por commit, estado actual)
sandbox:
  - cargo test -p ruscadb --test group_commit_crash
---

# SPEC-0081 — Group commit real (Ola 2, en solitario por tocar el WAL)

## Contexto

Auditoría *readiness* (riesgo del WAL): el módulo declara «group commit
(coalescing fsync)» pero `Database::commit` hace `wal.append` + `wal.sync`
**por commit** (`database.rs:301-302`) — no hay coalescing ni pipelining. Es
decir: falso group commit.

Frontera 2026 (§3, research T5): *greedy-pipelined group commit*
(arXiv:2606.18187) — un writer thread coalescea las peticiones de fsync
pendientes (grupo codicioso) y hace pipeline (serializa el grupo siguiente
mientras sincroniza el actual), con `FsyncPolicy{Full, Normal}` al estilo
SQLite/PostgreSQL (`NORMAL` = coalescing; `FULL` = fsync por commit). El ack
siempre espera el fsync de su grupo: nunca se confirma un commit no durable.

## Superficie

- `crates/ruscadb-wal/src/lib.rs`: `FsyncPolicy`, `Wal` con modo de
  agrupación (contador de fsyncs expuesto para el AC-0081-02), writer
  thread con canal de peticiones `{lsn, ack}` y drenaje en `sync`/`close`/
  drop.
- `crates/ruscadb/src/database.rs`: apertura con política (opción nueva
  aditiva; el default `Full` conserva la firma actual de `open`), `commit`
  espera el ack del grupo en `Normal`.
- Test: `crates/ruscadb/tests/group_commit_crash.rs` — proceso hijo real
  (patrón de `tests/process_lock.rs`) con kill en mitad del grupo.

## Criterios de aceptación

- **AC-0081-01** — Full: cada ack es durable (kill tras N acks → N recuperados).
- **AC-0081-02** — Normal: fsyncs < N y los N acks persisten.
- **AC-0081-03** — kill durante el grupo → recovery con prefijo completo.

## Trazabilidad

Tests `test_ac_0081_<nn>_*` con marcador `@spec AC-0081-NN`; verificado por
`cargo xtask trace`/`contract`.
