# Changelog

Todos los cambios notables de RuscaDB se documentan en este archivo.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el
proyecto se adhiere a [Semantic Versioning](https://semver.org/lang/es/).

## [Unreleased]

### Added

- **SPEC-0076** (Ola 0): lock cross-proceso — `LockGuard` RAII sobre sidecar
  `<data>.lock` con `std::fs::File::try_lock` (MSRV del workspace → 1.89);
  segunda apertura (mismo u otro proceso) devuelve `RuscaError::DatabaseInUse`
  con la ruta; drop libera el lock. Test multi-proceso real.
- **SPEC-0077** (Ola 0): migración forward-only de esquema — `Manifest::load`
  diferencia `SchemaVersionTooNew` (fichero válido, binario viejo) de
  `CorruptManifest` y `MigrationFailed`; runner atómico `tmp+rename` con
  registro `[(from,to,apply)]` y `migrate_plan()` dry-run. AC-0019-02
  actualizado.
- **Plan de producción Olas 0–4 (SPEC-0076..0088)**: roadmap actualizado tras
  la auditoría *production readiness* (2026-10-12, veredicto «NO apto para
  producción») y el research de frontera correspondiente (§3): lock
  cross-proceso, migración forward-only, API stability + semver CI, coverage
  por crate, observabilidad, group commit, gates SLO estadísticos, reclusión
  de storage, catálogo ampliado, docs de operación, examples, hardening P2 y
  embeddings estáticos (`model2vec-rs`).

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
- **SPEC-0001..0062**: 62 specs SDD (`specs/*.md`), cada AC trazado a un test
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
- **SPEC-0059**: benches criterion de regresión (point-read indexado, ingest
  blob 1 MiB, group-commit WAL c=64, latencia HNSW k=10) con baseline
  versionada (`benches/baselines/initial.md`), job `benches` alert-only en
  nightly y guard que exige su presencia.
- **SPEC-0061**: gate T2 determinista — jobs `mutation-diff` (MS ≥ 70 % sobre
  el diff, solo PR) y `coverage` (core ≥ 80/70) en `ci.yml` + `cargo xtask
  contract` (marcadores `@spec` y estados de specs bidireccionales).
- **SPEC-0060**: revalidación de ADR-001 con un spike `TableProvider` de
  DataFusion detrás de la feature `datafusion` (OFF por defecto, árbol
  34→894 crates): paridad de scan y differential `WHERE`/`ORDER BY`/proyección
  probados, y veredicto ADR-013 publicado en la tabla §4.6 del roadmap.
- **SPEC-0062**: hardening operativo — `Database::integrity_check()` (CRC32C
  del WAL con `ruscadb_wal::verify` que distingue corrupción de cola rasgada,
  coherencia del heap y cobertura del índice vectorial) + shadow reads
  (`set_shadow_reads`/`shadow_report`): cada KNN se re-verifica contra el
  oráculo exacto `ruscadb_vector::distance` sin alterar el resultado servido.
- **SPEC-0063**: búsqueda híbrida término+vector — `Database::hybrid_search`
  con fusión **RRF** (inversión de rango ponderada, K=60, N=k×4) y re-scoring
  late-interaction por overlap de términos (proxy de MaxSim, TRF
  arXiv:2508.01405), degradación graceful a BM25/KNN puros (`Bm25Only`/
  `KnnOnly`) y API aditiva sin tocar `execute`.
- **SPEC-0064**: gates de ingesta + registry persistible —
  `IngestionPolicy { max_record_bytes, require_registry }` (por defecto abierta)
  con rechazo `ResourceLimit`/`InvalidConfig`, y `ModelRegistry` persistido en
  `<data>.models.json` (sobrevive al reopen).
- **SPEC-0065**: caché semántico de KNN (hit por coseno ≥ 0.8 sobre el
  embedding de consulta; OFF por defecto) + `CostReport` (queries, filas y
  nanos acumulados, reseteable).
- **SPEC-0066**: RBAC por rol de instancia (`Read`/`Write`/`Admin`, default
  Admin) con `RuscaError::PermissionDenied` accionable + audit en memoria
  (OFF por defecto, `audit_trail`/`clear_audit`).
- **SPEC-0067**: pipeline de blobs — cola FIFO (`blob_pipeline_enqueue`/
  `drain`/`status`) con análisis síncrono (tamaño, media type inferido,
  `chunk_count` a 4 KiB) sin mutar el blob.
- **SPEC-0068**: crate **`ruscadb-mcp`** (22.º crate del workspace) — servidor
  MCP por **stdio** (JSON-RPC 2.0) con tools `query`/`tables`/
  `integrity_check`; sin dependencias de red (solo `serde_json`).
- **SPEC-0069**: coherencia del caché semántico — invalidación de entradas en
  toda mutación (insert/insert_many/update/delete), **firma de query** (tabla,
  `k`, `WHERE`, `LIMIT` con el vector enmascarado) y coseno que rechaza
  dimensiones distintas.
- **SPEC-0070**: endurecimiento del camino de escritura — `insert_many`
  (camino del `INSERT` SQL) con RBAC + gates + allowlist, `Database::delete`
  con `require_write`, servidor MCP de **solo lectura** (guard DML) y audit con
  **reloj monótono** estrictamente creciente + notificaciones JSON-RPC sin
  respuesta.
- **SPEC-0071**: robustez miscelánea — re-scoring híbrido sobre todo el top-k
  (scores en una sola escala), registry atómico sin estado parcial (y ruta
  correcta `<data>.models.json`, sin doble extensión), contador de jobs
  saturante y **batch JSON-RPC** en el servidor MCP.
- **SPEC-0072**: reranker real por **late-interaction** (ColBERT-style) con
  **hash embeddings** deterministas (Svenstrup 2017): `MaxSim(q,d) = Σᵢ maxⱼ
  cos(qᵢ,dⱼ)` sustituye el proxy de overlap, sin runtime externo (coherente
  con ADR-016).
- **SPEC-0073**: audit **persistente** append-only en `<data>.audit.jsonl`
  (patrón WAL: CRC32C por línea, tolerancia a corrupción, rotación por tamaño),
  cargado al abrir.
- **SPEC-0074**: **overflow pages del heap** — filas mayores que la página
  (4 KiB) se guardan como cadena de páginas de desbordamiento con stub en el
  slot base (round-trip, update/delete y replay WAL verificados).
- **SPEC-0075**: MCP con **escritura opt-in** (`McpServer::read_write`, RBAC
  propagado) y **resources** (`ruscadb://table/<name>` con `resources/list` y
  `resources/read`).

### Fixed

- **FTS (regresión cazada por el proptest del CI)**: `tokenize` no era
  idempotente ante el lowercase Unicode (`"İ"` indexaba como `"i̇"` y la
  consulta se partía en `"i"`), de modo que un documento podía no recuperarse
  por sus propios términos. Ahora el lowercase se aplica **antes** de segmentar
  y los diacríticos combinantes (Mn) se descartan: `tokenize(token) == token`.

### Retrospectiva

- Roadmap §2.6: ritual de **retrospectiva de frontera** formalizado — se
  ejecuta cada 3 specs/refactors (cadencia mínima) con gatillos adelantados
  (técnica nueva, cambio de ADR, dolor técnico) y salida acotada (veredictos
  sobre §3, specs/ADRs/SLOs encaminados, registro aquí).
- Primera edición (ciclo SPEC-0060/0061/0062 + SSOT): auditoría de frontera con
  3 especialistas → 25+ hallazgos priorizados (P1: filas de trazabilidad §7.5
  falsas, desviaciones SSOT Lance/redb/Candle sin ADR, T3 crash-recovery sin
  job, `verification.md`/README desactualizados).
- **P0 ejecutado del lote**: roadmap §7.5 con tests reales (+nota cov de
  referencia); ADR-014/015/016 (PagedFile/B+tree/embeddings propios) con
  ADR-001/002/003/005 actualizados; §0/§1.4/§3/§4.2/§4.3/§4.4 alineados con la
  realidad del workspace; comentario `ci.yml` corregido (crash-tests viven en
  T1); `verification.md` (contract, benches real, tabla jobs, 62 specs/279
  AC/555 tests); README (555 tests, feature `datafusion` opcional); CHANGELOG
  (SPEC-0061, 62 specs, typo). SPEC-0063 `hybrid_retrieval` aprobada e
  **implementada** en este ciclo (RRF + re-scoring overlap, API aditiva).
- Segunda edición (ciclo SPEC-0063..0068): **las 5 líneas futuras restantes
  implementadas** — gates de ingesta + registry persistible, caché semántico +
  coste por query, audit + RBAC, pipeline de blobs y servidor MCP stdio (crate
  nuevo `ruscadb-mcp`). Veredictos: RRF/MaxSim en producción (reranker ONNX
  real queda opcional); auditoría y caché en memoria (persistencia futura);
  MCP por stdio preserva la identidad «sin red».
- Tercera edición (ciclo SPEC-0069..0071): auditoría retrospectiva de los
  módulos nuevos con **5 hallazgos P1 reales** — caché sin invalidación, clave
  de caché incompleta (tabla/k/WHERE), `insert_many` evadiendo RBAC/gates,
  `delete` sin RBAC y MCP ejecutando escrituras — más 10 P2/P3 (doble extensión
  del registry, coseno truncando dimensiones, notificaciones JSON-RPC,
  scores híbridos mezclados). Los tres P1/P2 corregidos con TDD (RED por
  aserción) en SPEC-0069..0071; quedan como líneas futuras: reranker ONNX real,
  persistencia de caché/audit, overflow pages del heap y tools MCP de
  escritura.
- Cuarta edición (ciclo SPEC-0072..0075): **las 4 líneas futuras restantes
  implementadas** con técnicas de frontera — late-interaction con hash
  embeddings (reranker real sin runtime externo), audit persistente con CRC,
  overflow pages del heap (y el bug de asignador que el test descubrió en
  `heap_update`), y MCP con escritura opt-in + resources. Cierra el roadmap
  §Estado: no quedan líneas futuras con spec pendiente.

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
