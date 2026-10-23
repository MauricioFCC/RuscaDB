---
id: SPEC-0109
feature: mvcc_concurrent_readers
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/database.rs, crates/ruscadb/tests/readonly_open.rs]
  out_of_scope: [writers concurrentes, sync protocol, columnar engine]
fr:
  - { id: FR-0109-01, desc: "Database::open_readonly abre la base sin lock exclusivo (SPEC-0076)" }
  - { id: FR-0109-02, desc: "las operaciones de escritura devuelven RuscaError::ReadOnly en modo solo lectura" }
nf:
  - { id: NF-0109-01, desc: "cero dependencias nuevas; reusa el manifiesto y el replay existentes" }
acceptance_criteria:
  - id: AC-0109-01
    given: una base abierta en modo write
    when: se abre en modo readonly en el mismo proceso
    then: la apertura tiene exito sin DatabaseInUse
    test: test_ac_0109_01_readonly_open_succeeds
  - id: AC-0109-02
    given: una base abierta en modo readonly
    when: se intenta insertar
    then: falla con RuscaError::ReadOnly
    test: test_ac_0109_02_readonly_rejects_writes
exit_criteria:
  - cargo test -p ruscadb --all-features --test readonly_open
  - cargo test --workspace --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0109 — MVCC concurrent readers (read-only open)

## Contexto

Frontier 2026 (redb, lmdb, SurrealDB): los lectores no bloquean al escritor ni
entre sí. RuscaDB tenía lock exclusivo (SPEC-0076) que impedía lectores
concurrentes. Esta spec añade `Database::open_readonly`, que abre la base sin
lock exclusivo y rechaza escrituras, permitiendo varios procesos lectores
concurrentes mientras un único proceso tiene la base en modo write.

## Superficie

- `crates/ruscadb/src/database.rs` — `open_readonly` (sin lock), campo
  `read_only`, `is_read_only`, `ensure_writable`.
- `crates/ruscadb-core/src/error.rs` — variante `RuscaError::ReadOnly`.
- Escrituras (`insert`, `update`, `delete`, `create_table`, `execute` DML)
  rechazan en modo solo lectura.

## Notas

- El lector ve el estado persistido (último commit fsync'd). No hay
  shared lock ni snapshot MVCC cross-proceso en esta iteración; la
  consistencia se basa en el WAL + manifest persistidos.
- Futuro: copy-on-write B+trees (redb-style) para snapshot readers sin
  bloqueo (SPEC-0110+).
---
