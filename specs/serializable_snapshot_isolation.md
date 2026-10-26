---
id: SPEC-0170
feature: serializable_snapshot_isolation
status: implemented
owner: txn-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb-txn/src/mvcc.rs, crates/ruscadb/src/database.rs]
  out_of_scope: [full SSI in distributed, predicate locks]
fr:
  - { id: FR-0170-01, desc: "nivel de aislamiento Serializable via SSI" }
  - { id: FR-0170-02, desc: "deteccion de ciclos read-write (rw-dependencies)" }
  - { id: FR-0170-03, desc: "abort de transacciones con ciclo (serialization failure)" }
  - { id: FR-0170-04, desc: "configurable: ReadCommitted / Snapshot / Serializable" }
nf:
  - { id: NF-0170-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0170-01
    given: dos transacciones concurrentes con rw-dependency ciclica
    when: ambas intentan commit
    then: una se aborta con SerializationFailure
    test: test_ac_0170_01_cycle
  - id: AC-0170-02
    given: nivel Serializable configurado
    when: se ejecuta una transaccion de solo lectura
    then: no se aborta (read-only optimization)
    test: test_ac_0170_02_readonly
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-txn --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0170 — Serializable Snapshot Isolation (SSI) opcional

## Contexto

RuscaDB ofrece snapshot isolation (no serializable). SSI permite serializabilidad
sin locks, detectando ciclos rw-dependency.

## Superficie

- `mvcc.rs`: `IsolationLevel { ReadCommitted, Snapshot, Serializable }`,
  `SsiTracker` con rw-dependencies.
- `database.rs`: configuracion por transaccion.

## Estado del arte

- **Cahill et al. 2009**: "Serializable Isolation for Snapshot Databases".
- **PostgreSQL**: SSI desde 9.1.
- **RuscaDB actual**: snapshot isolation (SPEC-0019).

## Diseño propuesto

1. **rw-dependency**: transaccion T1 lee un dato escrito por T2 (T1 -> T2) y
   T2 lee un dato escrito por T1 (T2 -> T1) => ciclo => abort.
2. **Read-only optimization**: transacciones de solo lectura no se abortan.
3. **Configurable**: por base de datos o por transaccion.

## Riesgos

- Falsos positivos: SSI puede abortar transacciones sin ciclo real (raro).

## ADR requerido

`docs/adr/ADR-0170-ssi.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\serializable_snapshot_isolation.md