---
id: SPEC-0138
feature: sync_replication
status: implemented
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-wal]
  files: [crates/ruscadb/src/sync.rs]
  out_of_scope: [clustering, sharding, distributed transactions]
fr:
  - { id: FR-0138-01, desc: "feature flag `sync` (off por defecto) habilita replicacion" }
  - { id: FR-0138-02, desc: "SyncProtocol::checkpoint genera un checkpoint para replicacion" }
nf:
  - { id: NF-0138-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0138-01
    given: la feature `sync` activada
    when: se genera un checkpoint
    then: el checkpoint contiene el LSN y el epoch
    test: test_ac_0138_01_sync_checkpoint
exit_criteria:
  - cargo test -p ruscadb --all-features --test sync
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0138 — Sync protocol / replicación

## Contexto

Frontier 2026 (Turso/libSQL, FFS): replicación embedded (local replica de una
base remota). RuscaDB es single-node. Esta spec añade un protocolo de sync
básico detrás de un feature flag.

## Superficie

- Feature `sync` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/sync.rs` — `SyncProtocol` con `checkpoint`.
- Integración futura: HTTP sync, embedded replica, CDC.

## Estado del arte

- **Turso/libSQL**: embedded replica sync (HTTP), local copy of remote DB.
- **FFS**: checkpoint a S3-compatible object store, attach read-only.
- **RuscaDB actual**: single-node, backup local (SPEC-0116), backup S3 (SPEC-0128).

## Diseño propuesto

1. **Checkpoint**: genera un snapshot consistente (LSN + epoch + manifest).
2. **Sync**: envía el checkpoint a un peer (HTTP).
3. **Replica**: recibe el checkpoint y lo aplica localmente.

## Notas

- Implementación inicial: checkpoint local (sin red).
- Sync real queda como trabajo futuro.
---
