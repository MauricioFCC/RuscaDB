---
id: SPEC-0121
feature: zstd_compression
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-wal]
  files: [crates/ruscadb/src/compression.rs, crates/ruscadb/Cargo.toml]
  out_of_scope: [compresion de paginas de datos, zstd real en WAL]
fr:
  - { id: FR-0121-01, desc: "feature flag `zstd-compression` (off por defecto) habilita compresion zstd" }
  - { id: FR-0121-02, desc: "ZstdCompressor::compress y ::decompress son inversos" }
  - { id: FR-0121-03, desc: "Compressor (RLE) sigue disponible como fallback sin deps" }
nf:
  - { id: NF-0121-01, desc: "zstd crate opcional detras del flag; build por defecto sin deps nuevas" }
acceptance_criteria:
  - id: AC-0121-01
    given: la feature `zstd-compression` activada
    when: se comprime y descomprime un payload
    then: el resultado es identico al original
    test: test_ac_0121_01_zstd_roundtrip
  - id: AC-0121-02
    given: la feature `zstd-compression` activada
    when: se comprime un payload vacio
    then: el resultado es vacio
    test: test_ac_0121_02_zstd_empty
exit_criteria:
  - cargo test -p ruscadb --all-features --test zstd_compression
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0121 — Compresión zstd para WAL

## Contexto

Frontier 2026 (RocksDB, PostgreSQL, Stoolap): compresión zstd para WAL reduce
I/O 20-40%. RuscaDB tiene RLE stub (SPEC-0115). Esta spec añade zstd real
detrás de un feature flag, sin cambiar el build por defecto.

## Superficie

- Feature `zstd-compression` en `ruscadb` (off por defecto).
- Dependencia opcional `zstd = "0.14"`.
- `crates/ruscadb/src/compression.rs` — `ZstdCompressor` con `compress`/`decompress`.
- Integración futura: WAL frames comprimidos, páginas de datos comprimidas.

## Notas

- Implementación: zstd crate (bindings a libzstd).
- RLE (Compressor) sigue disponible como fallback sin deps.
- Nivel de compresión configurable (default 3).
---
