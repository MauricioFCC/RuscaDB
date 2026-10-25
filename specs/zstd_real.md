---
id: SPEC-0154
feature: zstd_real
status: proposed
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-wal]
  files: [crates/ruscadb/src/zstd_real.rs]
  out_of_scope: [compresion de paginas de datos, zstd real en WAL]
fr:
  - { id: FR-0154-01, desc: "feature flag `zstd-real` (off por defecto) habilita zstd real (dep opcional)" }
  - { id: FR-0154-02, desc: "ZstdReal::compress y ::decompress son inversos" }
nf:
  - { id: NF-0154-01, desc: "zstd crate opcional detras del flag" }
acceptance_criteria:
  - id: AC-0154-01
    given: la feature `zstd-real` activada
    when: se comprime y descomprime un payload
    then: el resultado es identico al original
    test: test_ac_0154_01_zstd_real_roundtrip
exit_criteria:
  - cargo test -p ruscadb --all-features --test zstd_real
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0154 — Compresión zstd real (no RLE)

## Contexto

Frontier 2026 (RocksDB, PostgreSQL): compresión zstd real para WAL.
RuscaDB tiene RLE (SPEC-0115) y zstd-compression (SPEC-0121, dep opcional).
Esta spec unifica la compresión zstd real detrás de un feature flag.

## Superficie

- Feature `zstd-real` en `ruscadb` (off por defecto).
- Dependencia opcional `zstd = "0.14"`.
- `crates/ruscadb/src/zstd_real.rs` — `ZstdReal` con `compress`/`decompress`.
- Integración futura: WAL frames comprimidos.

## Estado del arte

- **RocksDB**: zstd para WAL y SST.
- **PostgreSQL**: zstd para WAL (full page images).
- **Stoolap**: LZ4 para WAL y volumes.
- **RuscaDB actual**: RLE (SPEC-0115), zstd-compression (SPEC-0121).

## Diseño propuesto

1. **ZstdReal**: wrapper sobre zstd crate.
2. **Nivel configurable**: 1-22 (default 3).
3. **Streaming**: soporte para payloads grandes.

## Notas

- Implementación inicial: zstd encode_all/decode_all.
- Streaming y niveles configurables quedan como trabajo futuro.
---
