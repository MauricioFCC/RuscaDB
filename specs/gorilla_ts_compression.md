---
id: SPEC-0178
feature: gorilla_ts_compression
status: implemented
owner: ts-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-ts]
  files: [crates/ruscadb-ts/src/lib.rs]
  out_of_scope: [streaming, multi-series]
fr:
  - { id: FR-0178-01, desc: "Gorilla: delta-of-delta para timestamps" }
  - { id: FR-0178-02, desc: "XOR para valores float con leading/trailing zeros" }
  - { id: FR-0178-03, desc: "integracion con TS engine (SPEC-0029)" }
nf:
  - { id: NF-0178-01, desc: "cero dependencias nuevas" }
  - { id: NF-0178-02, desc: "compresion >= 10x en series temporales regulares" }
acceptance_criteria:
  - id: AC-0178-01
    given: una serie temporal con timestamps regulares
    when: se comprime con Gorilla
    then: el ratio de compresion es >= 10x
    test: test_ac_0178_01_ratio
  - id: AC-0178-02
    given: datos comprimidos
    when: se descomprimen
    then: se recuperan los valores exactos
    test: test_ac_0178_02_lossless
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-ts --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0178 — Gorilla compression para time-series

## Contexto

Time-series (SPEC-0029) almacena puntos sin comprimir. Gorilla (Facebook)
ofrece compresion lossless >= 10x en series regulares.

## Superficie

- `ts/lib.rs`: `GorillaEncoder { encode_timestamp, encode_value }`,
  `GorillaDecoder`.

## Estado del arte

- **Gorilla** (Pelkonen et al., VLDB 2015): "Gorilla: A Fast, Scalable,
  In-Memory Time Series Database".
- **RuscaDB actual**: TS engine (SPEC-0029) sin compresion.

## Diseño propuesto

1. **Timestamps**: delta-of-delta con bit-packing variable.
2. **Values**: XOR con valor anterior; contar leading/trailing zeros;
   almacenar solo los bits significativos.
3. **Block**: 2h o 1024 puntos por bloque.

## Riesgos

- Series irregulares: compresion menor; fallback a raw.

## ADR requerido

`docs/adr/ADR-0178-gorilla.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\gorilla_ts_compression.md