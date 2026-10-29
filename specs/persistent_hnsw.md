---
id: SPEC-0173
feature: persistent_hnsw
status: implemented
maturity: stub
owner: vector-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-vector]
  files: [crates/ruscadb-vector/src/hnsw.rs, crates/ruscadb/src/database.rs]
  out_of_scope: [memory-mapped HNSW, distributed]
fr:
  - { id: FR-0173-01, desc: "serializacion de HNSW a disco (bincode-like)" }
  - { id: FR-0173-02, desc: "carga lazy con mmap opcional" }
  - { id: FR-0173-03, desc: "versionado del formato (header + checksum)" }
  - { id: FR-0173-04, desc: "incremental update + persistencia" }
nf:
  - { id: NF-0173-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0173-01
    given: un HNSW con N vectores
    when: se guarda y se recarga
    then: los vectores y la estructura se preservan
    test: test_ac_0173_01_roundtrip
  - id: AC-0173-02
    given: un fichero HNSW corrupto
    when: se carga
    then: se devuelve error (no panic)
    test: test_ac_0173_02_corrupt
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-vector --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0173 — HNSW persistente (serializacion + versionado)

## Contexto

HNSW (SPEC-0006) vive solo en memoria; al cerrar la DB se pierde. Produccion
requiere persistencia.

## Superficie

- `hnsw.rs`: `save(&self, w: &mut dyn Write)`, `load(r: &mut dyn Read)`.
- `database.rs`: integracion con catalogo de indices (guardar/cargar al open).

## Estado del arte

- **Qdrant**: HNSW persistido en segmentos + mmap.
- **LanceDB**: indices Lance (columnar).
- **RuscaDB actual**: HNSW en memoria (SPEC-0006).

## Diseño propuesto

1. **Formato**: header (magic, version, dim, params) + nodos (id, level, vecinos).
2. **Checksum**: CRC32C por seccion (reusa SPEC-0167).
3. **Versionado**: bump de version al cambiar layout; loader rechaza versiones
   desconocidas.
4. **Mmap opcional**: para datasets grandes (futuro).

## Riesgos

- Compatibilidad: migracion de versiones antiguas.

## ADR requerido

`docs/adr/ADR-0173-persistent-hnsw.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\persistent_hnsw.md