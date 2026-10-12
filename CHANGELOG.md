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
- **SPEC-0051**: `GROUP BY` multi-clave + `HAVING` (`COUNT`/`SUM`/`AVG` con
  `AND`; `HAVING` sin `GROUP BY` es `ParseError`).
- **SPEC-0052**: `INNER JOIN` de dos tablas por igualdad simple (nested-loop
  con lookups al índice primario; no-equi rechazado con error accionable).
- **SPEC-0053**: modelo loom del MVCC (commits concurrentes sin pérdida,
  atomicidad del snapshot).
- **SPEC-0055**: contención MVCC c=64 sin pérdidas, first-committer-wins con
  error `WriteConflict`, `retry_on_conflict` con backoff acotado y métrica
  `dead_versions` para el reaper.
- **SPEC-0056**: harness crash-recovery del WAL (hijo + kill pre/post `sync`,
  tail rasgada truncada, replay idempotente).
- **SPEC-0058**: budget duro de RAM para HNSW (`ResourceLimit` con estimado)
  + índice plano cuantizado (1 B/dim) con recall@10 ≥ 0.95.
- **SPEC-0054**: límites del planner pre-ejecución (profundidad AST ≤ 64,
  budget de complejidad `ResourceLimit`, `LIMIT` implícito 1000, timeout
  cooperativo `QueryTimeout` y `EXPLAIN` con aviso de full-scan R9).
- **SPEC-0057**: time-series cableado en la fachada (`ts_api`): `ts_bucket`
  alineado a `time_bucket` y `ts_window` deslizante con agregados
  `count/sum/min/max/avg`, columna no temporal rechazada con error accionable.
- **SPEC-0059**: benches criterion de regressión (point-read indexado, ingest
  blob 1 MiB, group-commit WAL c=64, latencia HNSW k=10) con baseline
  versionada (`benches/baselines/initial.md`), job `benches` alert-only en
  nightly y guard que exige su presencia.
- **SPEC-0060**: revalidación de ADR-001 con un spike `TableProvider` de
  DataFusion detrás de la feature `datafusion` (OFF por defecto, árbol
  34→894 crates): paridad de scan y differential `WHERE`/`ORDER BY`/proyección
  probados, y veredicto ADR-013 publicado en la tabla §4.6 del roadmap.
- **SPEC-0062**: hardening operativo — `Database::integrity_check()` (CRC32C
  del WAL con `ruscadb_wal::verify` que distingue corrupción de cola rasgada,
  coherencia del heap y cobertura del índice vectorial) + shadow reads
  (`set_shadow_reads`/`shadow_report`): cada KNN se re-verifica contra el
  oráculo exacto `ruscadb_vector::distance` sin alterar el resultado servido.

### Retrospectiva

- Roadmap §2.6: ritual de **retrospectiva de frontera** formalizado — se
  ejecuta cada 3 specs/refactors (cadencia mínima) con gatillos adelantados
  (técnica nueva, cambio de ADR, dolor técnico) y salida acotada (veredictos
  sobre §3, specs/ADRs/SLOs encaminados, registro aquí).
- Primera edición (ciclo SPEC-0060/0061/0062 + SSOT): auditoría de frontera con
  3 especialistas → 25+ hallazgos priorizados (P1: filas de trazabilidad §7.5
  falsas, desviaciones SSOT Lance/redb/Candle sin ADR, T3 crash-recovery sin
  job, `verification.md`/README desactualizados). Pendientes P1 se ejecutan
  como P0 del siguiente lote.

### Changed

- RQL ampliado de forma incremental: `SELECT`/`WHERE`/`LIMIT` (SPEC-0005) →
  extensiones `KNN`/`TRAVERSE`/`EXPLAIN` (SPEC-0015) → `ORDER BY` (SPEC-0036) →
  `GROUP BY` + agregados (SPEC-0040) → DML y operadores de documento
  (SPEC-0043/0044) →   `GROUP BY` multi-clave + `HAVING` (SPEC-0051) →
  `INNER JOIN` por PK (SPEC-0052). La fachada también expone el planner de
  límites (SPEC-0054) y el módulo time-series `ts_api` (SPEC-0057).
- La fachada `ruscadb` integra el blob store y su GC con el barrier R7
  (SPEC-0038) y el iFVS en el executor del `KNN` con `WHERE` (SPEC-0048).

### Fixed

- Sin correcciones publicadas todavía.

## [0.1.0] - no publicado

Primera línea base del workspace. **Aún no publicada**: los crates internos
declaran `publish = false` y la API pública puede cambiar sin previo aviso. Las
fases F0–F6 y las specs SPEC-0001..0050 están implementadas y trazadas por
`cargo xtask trace`.
