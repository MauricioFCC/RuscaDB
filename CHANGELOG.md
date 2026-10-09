# Changelog

Todos los cambios notables de RuscaDB se documentan en este archivo.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el
proyecto se adhiere a [Semantic Versioning](https://semver.org/lang/es/).

## [Unreleased]

### Added

- **F0 — Fundación**: workspace Cargo hexagonal, plantilla `specs/` SDD,
  `cargo xtask trace`, gates T1/T3, presupuesto de `unsafe` y `deny.toml`.
- **F1 — Storage/WAL/MVCC**: `ruscadb-core` (dominio), `ruscadb-storage`
  (buffer pool LRU-K + `PagedFile`), `ruscadb-wal` (frames CRC32C, recovery,
  group commit) y `ruscadb-txn` (snapshot isolation + manifiesto versionado).
- **F2 — Query engine (RQL)**: `ruscadb-query` (lexer, parser, IR y `Display`)
  y su ejecución en la fachada: `SELECT`/`WHERE`/`LIMIT`, DML
  (`INSERT`/`UPDATE`/`DELETE`), `ORDER BY`, `GROUP BY` + agregados, operadores
  de documento `->`/`@>` y `MATCH` de texto completo.
- **F3 — Índices y grafos**: `ruscadb-vector` (HNSW), `ruscadb-fvs` (filtrado
  híbrido pre/in/post e iFVS), `ruscadb-graph` (CSR + algoritmos), `ruscadb-fts`
  (índice invertido + BM25) y `ruscadb-btree` (B+tree ordenado).
- **F4 — Multimodal/IA/time-series**: `ruscadb-multimodal` (blob store CAS con
  GC y barrier R7), `ruscadb-ai` (embeddings locales), `ruscadb-ts`
  (time-series: bucketing, ventanas, remuestreo, percentiles, rate) y versionado
  de embeddings.
- **F5 — Bindings**: `ruscadb-ffi` (C-ABI), `ruscadb-py`, `ruscadb-node` y
  `ruscadb-wasm`.
- **F6 — Hardening/Release**: cifrado en reposo XChaCha20-Poly1305 + Argon2id,
  endurecimiento de CI (fuzzing continuo, differential tests, cross-platform),
  SBOM y supply chain.
- **SPEC-0001..0050**: 50 specs SDD (`specs/*.md`), cada AC trazado a un test
  `test_ac_XXXX_NN_*` verificado por `cargo xtask trace`.

### Changed

- RQL ampliado de forma incremental: `SELECT`/`WHERE`/`LIMIT` (SPEC-0005) →
  extensiones `KNN`/`TRAVERSE`/`EXPLAIN` (SPEC-0015) → `ORDER BY` (SPEC-0036) →
  `GROUP BY` + agregados (SPEC-0040) → DML y operadores de documento
  (SPEC-0043/0044).
- La fachada `ruscadb` integra el blob store y su GC con el barrier R7
  (SPEC-0038) y el iFVS en el executor del `KNN` con `WHERE` (SPEC-0048).

### Fixed

- Sin correcciones publicadas todavía.

## [0.1.0] - no publicado

Primera línea base del workspace. **Aún no publicada**: los crates internos
declaran `publish = false` y la API pública puede cambiar sin previo aviso. Las
fases F0–F6 y las specs SPEC-0001..0050 están implementadas y trazadas por
`cargo xtask trace`.
