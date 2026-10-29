---
id: SPEC-0175
feature: alp_compression
status: implemented
maturity: real
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-storage]
  files: [crates/ruscadb/src/compression.rs]
  out_of_scope: [dictionary compression, streaming]
fr:
  - { id: FR-0175-01, desc: "ALP: Adaptive Lossless floating-Point compression" }
  - { id: FR-0175-02, desc: "encoding con exponente + bit-packing" }
  - { id: FR-0175-03, desc: "integracion con columnar (SPEC-0110)" }
nf:
  - { id: NF-0175-01, desc: "cero dependencias nuevas" }
  - { id: NF-0175-02, desc: "compresion >= 3x en datos float con pocos decimales" }
acceptance_criteria:
  - id: AC-0175-01
    given: un array de f64 con 2 decimales
    when: se comprime con ALP
    then: el ratio de compresion es >= 3x
    test: test_ac_0175_01_ratio
  - id: AC-0175-02
    given: datos comprimidos
    when: se descomprimen
    then: se recuperan los valores exactos (lossless)
    test: test_ac_0175_02_lossless
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0175 — ALP: compresion lossless de floats

## Contexto

Columnar (SPEC-0110) almacena floats sin comprimir. ALP ofrece compresion
lossless adaptativa para floats con pocos decimales (caso comun en TS/ML).

## Superficie

- `compression.rs`: `AlpCompressor { encode, decode }`.
- Integracion con `ColumnarTable` (SPEC-0110).

## Estado del arte

- **ALP** (Afroozeh et al., SIGMOD 2024): "Adaptive Lossless floating-Point
  Compression".
- **FastLanes** (2023): bit-packing vectorizado.
- **RuscaDB actual**: RLE (SPEC-0115) + zstd (SPEC-0121).

## Diseño propuesto

1. **Encode**: detectar exponente (numero de decimales) y convertir a entero
   escalado; bit-pack los enteros.
2. **Decode**: desescalar y reconstruir f64 exacto.
3. **Fallback**: si no hay patron decimal, almacenar raw.

## Riesgos

- Floats arbitrarios (NaN, Inf): manejar como excepciones.

## ADR requerido

`docs/adr/ADR-0175-alp.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\alp_compression.md