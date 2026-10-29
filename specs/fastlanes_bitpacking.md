---
id: SPEC-0176
feature: fastlanes_bitpacking
status: implemented
maturity: real
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-storage]
  files: [crates/ruscadb/src/compression.rs]
  out_of_scope: [SIMD intrinsics manuales]
fr:
  - { id: FR-0176-01, desc: "FastLanes: bit-packing vectorizado para enteros" }
  - { id: FR-0176-02, desc: "unpack con auto-vectorizacion del compilador" }
  - { id: FR-0176-03, desc: "integracion con columnar y WAL" }
nf:
  - { id: NF-0176-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0176-01
    given: un array de u32 con valores < 2^k
    when: se bit-packea
    then: el tamano se reduce a k bits por valor
    test: test_ac_0176_01_pack
  - id: AC-0176-02
    given: datos bit-packed
    when: se unpackean
    then: se recuperan los valores originales
    test: test_ac_0176_02_unpack
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0176 — FastLanes bit-packing

## Contexto

Columnar y WAL almacenan enteros sin comprimir. FastLanes ofrece bit-packing
eficiente con layout que favorece la auto-vectorizacion.

## Superficie

- `compression.rs`: `FastLanes { pack, unpack }`.

## Estado del arte

- **FastLanes** (Afroozeh et al., SIGMOD 2023): "FastLanes: A High-Performance
  Lightweight Data Format".
- **RuscaDB actual**: RLE (SPEC-0115) + zstd (SPEC-0121).

## Diseño propuesto

1. **Pack**: para cada valor, escribir k bits; layout transpuesto para SIMD.
2. **Unpack**: leer k bits por valor.
3. **Auto-vectorizacion**: usar slices y #[inline] para que el compilador
   vectorice.

## Riesgos

- k=0 (todos ceros): manejar como caso especial.

## ADR requerido

`docs/adr/ADR-0176-fastlanes.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\fastlanes_bitpacking.md