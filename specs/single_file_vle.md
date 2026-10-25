---
id: SPEC-0137
feature: single_file_vle
status: implemented
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-storage]
  files: [crates/ruscadb/src/vle.rs]
  out_of_scope: [distributed, clustering]
fr:
  - { id: FR-0137-01, desc: "feature flag `vle` (off por defecto) habilita single-file deployment" }
  - { id: FR-0137-02, desc: "VleDatabase::bake convierte directorio a single-file .pvdb" }
nf:
  - { id: NF-0137-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0137-01
    given: la feature `vle` activada
    when: se crea un VleDatabase
    then: la creacion no paniquea
    test: test_ac_0137_01_vle_create
exit_criteria:
  - cargo test -p ruscadb --all-features --test vle
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0137 — Single-file deployment (VLE)

## Contexto

Frontier 2026 (Picovolt): Virtualization Layer Engine (VLE) convierte un
directorio de base (mutable, append-only) en un single-file memory-mappable
(`.pvdb`, producción). RuscaDB usa múltiples ficheros (.data, .wal, .manifest).

## Superficie

- Feature `vle` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/vle.rs` — `VleDatabase` con `bake` + `open`.
- Integración futura: memory-mapping, zero-copy reads.

## Estado del arte

- **Picovolt** (picovolt): VLE shifts between directory (dev) and single-file
  binary (prod). Memory-mappable, CAS dedup, columnar compression.
- **RuscaDB actual**: múltiples ficheros (.data, .wal, .manifest, .lock).

## Diseño propuesto

1. **bake**: convierte el directorio de base a un single-file .pvdb.
2. **open**: abre un .pvdb en modo read-only (memory-mapped).
3. **VLE**: virtualización de páginas (directory ↔ single-file).

## Notas

- Implementación inicial: stub sin memory-mapping.
- VLE real queda como trabajo futuro.
---
