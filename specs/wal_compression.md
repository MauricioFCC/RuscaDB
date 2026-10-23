---
id: SPEC-0115
feature: wal_compression
status: proposed
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-wal, ruscadb-core]
  files: [crates/ruscadb/src/compression.rs]
  out_of_scope: [compresion de paginas de datos, zstd real]
fr:
  - { id: FR-0115-01, desc: "feature flag `compression` (off por defecto) habilita compresion de WAL" }
  - { id: FR-0115-02, desc: "Compressor::compress y Compressor::decompress son inversos" }
nf:
  - { id: NF-0115-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0115-01
    given: la feature `compression` activada
    when: se comprime y descomprime un payload
    then: el resultado es identico al original
    test: test_ac_0115_01_compress_roundtrip
exit_criteria:
  - cargo test -p ruscadb --all-features --test compression
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0115 — Compresión de WAL

## Contexto

Frontier 2026: compresión de WAL reduce I/O y almacenamiento. RuscaDB tiene WAL
con CRC32C pero sin compresión. Esta spec añade compresión opcional detrás de
un feature flag.

## Superficie

- Feature `compression` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/compression.rs` — `Compressor` con `compress`/`decompress`.
- Integración futura: WAL frames comprimidos, páginas de datos comprimidas.

## Notas

- Implementación inicial: RLE (run-length encoding) sin dependencias.
- zstd/lz4 se añadirán cuando se requiera compresión real.
---
