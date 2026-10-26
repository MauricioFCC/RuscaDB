---
id: SPEC-0179
feature: crdt_sync_real
status: implemented
owner: storage-team
appetite_days: 7
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb/src/sync.rs]
  out_of_scope: [distributed, network transport]
fr:
  - { id: FR-0179-01, desc: "CRDT merge (LWW-register, OR-Set) para documentos" }
  - { id: FR-0179-02, desc: "sync protocol con vector clocks" }
  - { id: FR-0179-03, desc: "conflict resolution determinista" }
  - { id: FR-0179-04, desc: "export/import de cambios (delta sync)" }
nf:
  - { id: NF-0179-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0179-01
    given: dos replicas con cambios concurrentes
    when: se mergean
    then: el resultado es determinista y convergente
    test: test_ac_0179_01_merge
  - id: AC-0179-02
    given: un delta sync
    when: se aplica a una replica
    then: el estado converge al global
    test: test_ac_0179_02_delta
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0179 — CRDT sync real (local-first)

## Contexto

`SyncProtocol` (SPEC-0138) solo guarda LSN/epoch. Local-first requiere
merge de cambios concurrentes sin coordinador.

## Superficie

- `sync.rs`: `CrdtRegister`, `OrSet`, `VectorClock`, `merge`, `delta`.

## Estado del arte

- **Shapiro et al. 2011**: "Conflict-free Replicated Data Types".
- **ElectricSQL / PowerSync**: sync engines sobre SQLite.
- **RuscaDB actual**: stub (SPEC-0138).

## Diseño propuesto

1. **LWW-register**: last-writer-wins por timestamp logico.
2. **OR-Set**: add/remove con tags unicos; merge = union.
3. **Vector clock**: detectar concurrencia.
4. **Delta sync**: exportar solo cambios desde el ultimo sync.

## Riesgos

- Tombstones: acumulacion; compactacion periodica.

## ADR requerido

`docs/adr/ADR-0179-crdt-sync.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\crdt_sync_real.md