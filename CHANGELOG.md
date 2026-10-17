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
- **SPEC-0078** (Ola 1): estabilidad de API — `#[non_exhaustive]` en
  `RuscaError`, `RUSCADB_ABI_VERSION` + `ruscadb_version()` en la C-ABI y
  workflow nuevo `semver.yml` con `cargo-semver-checks` contra baseline git
  (sin crates.io, el workspace es `publish = false`).
- **SPEC-0079** (Ola 1): política de cobertura multi-scope —
  `coverage_gate.py` acepta varios `--scope` con umbral propio e imprime % por
  crate; CI gatea core ≥ 80 %, txn ≥ 90 % y wal ≥ 90 % (medido 96.5/96.6 %);
  política documentada en `docs/verification.md`.
- **SPEC-0084** (Ola 1): docs de operación — runbook `docs/operacion.md`
  (lock, migración, backup, claves, troubleshooting) y `SECURITY.md` alineado
  con el código real (se retiran afirmaciones falsas: BLAKE3, cargo-vet,
  cargo-geiger, AES-256-GCM, secrecy; se conservan las verificadas: SBOM,
  cargo-deny, cargo-audit, gitleaks, miri, ASan).
- **SPEC-0080** (Ola 2): observabilidad — spans `tracing` en `commit`,
  `checkpoint`, `page_fault` y `lock_wait` (no-op ~1.2 ns sin subscriber) y
  feature `metrics` (default OFF) que registra latencia de commit e hit-rate
  del buffer pool; tests con collector local en ambas configuraciones.
- **SPEC-0085** (Ola 2): reclamación de storage — free-list persistida en el
  manifiesto (serde con default, compatible hacia atrás) que `allocate()`
  consume antes de crecer el fichero; `reap()` da de alta las páginas
  purgadas en el mismo commit atómico que retira los locator (crash-safe: no
  puede reasignarse una página viva); GC de derivados `remove_nodes` (CSR) y
  `drop_document` (FTS) cableado desde reap sin cambiar resultados de filas
  vivas.
- **SPEC-0086** (Ola 2): catálogo sin techo de 64 KiB — formato v2 con
  superblock de 24 B y cadena de páginas de overflow pagadas con el bump
  `next_page` compartido con el heap (patrón SPEC-0071, sin colisiones por
  construcción); v1 (≤16 páginas) carga sin migración; techo residual u32
  con error actionable.
- **SPEC-0081** (Ola 2): group commit — `FsyncPolicy{Full,Normal}` en el WAL
  con writer thread *greedy-pipelined* (arXiv:2606.18187): `Full` (default)
  fsync por commit idéntico al histórico; `Normal` coalesce los fsyncs
  pendientes en grupos y ack **solo tras** el fsync de su grupo (nunca
  confirma no durable); drenaje+unión en `Drop` sin deadlock; contador de
  fsyncs expuesto. Crash-harness con proceso hijo real + `kill` en mitad de
  un grupo (prefijo acknowledged completo, cola truncada).
- **SPEC-0082** (Ola 3): gates estadísticos SLO — `slo_gate.py` (solo stdlib,
  semilla fija) sustituye ratios de umbral fijo por Mann-Whitney direccional
  (p < 0.002, Bonferroni ≈ 25 métricas), bootstrap pareado 10k para deltas de
  recall y calibración A/A (noise floor de runners CV ≈ 2.7 %); contrato
  0/1/2 ruidoso; workflow `slo.yml` en PR compara criterion de la base del
  PR vs HEAD **en el mismo runner** (test z de Welch `--criterion-pair`);
  fixtures versionados + tests `test_ac_0082_*`.
- **SPEC-0083** (Ola 4): embeddings estáticos — feature `static-embeddings`
  (default OFF) con `model2vec-rs` 0.3 (`local-only` + `fancy-regex`: sin
  red, sin C) que carga safetensors locales (`potion-base-8M`); `StaticEmbedder`
  implementa `Embedder` (dim fija, L2 por contrato, determinista) con error
  WHAT+WHY+WHERE si falta el modelo. **Enmienda ADR-016**: pesos estáticos
  SÍ, runtimes (Candle/ONNX) NO.   Tests sin red con fixture safetensors
  mínimo generado por el propio test.
- **SPEC-0087** (Ola 4): examples ejecutables — tres ejemplos sobre la
  fachada (`quickstart`, `vector_search`, `graph_traverse`) con `main`
  sin `unwrap` (WHAT+WHY+WHERE + exit 1) y lógica en `run()`; el test de
  humo los **ejecuta como binario real** (exit code + salidas
  deterministas validadas) y la nueva guía `docs/api_guide.md` documenta
  cada ejemplo con su comando `cargo run --example ...`.
- **SPEC-0088** (Ola 4): hardening P2 — servidor MCP **sin sesión por
  defecto**: todo método de datos devuelve `-32001` hasta el handshake
  `session/auth`, con tokens AEAD `v1.<kid>.<nonce>.<ct>` sellados por
  el keystore KMS-lite nuevo (`ruscadb-crypto::keystore`: rotación =
  revocación inmediata, persistencia atómica con permisos 0600 en unix,
  re-export `Keystore` desde la fachada); 4 AC de integración
  (`mcp_auth.rs`) y `SECURITY.md` con el modelo de amenaza.
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
- **Determinismo del KNN con filtro (también cazado por el proptest del CI)**:
  `search_layer` de HNSW ordenaba solo por distancia y `post_filter_indexed`
  hacía `take(k)` sobre ese orden, así que los empates salían en orden
  arbitrario del `BinaryHeap` (`GOT=[0,3]` vs esperado `[0,1]`). Ahora hay
  desempate por id de inserción, alineado con el resto de rutas (FVS/iFVS);
  regresión determinista `knn_postfilter_ties_are_deterministic` + seed
  proptest versionada.

- **SPEC-0089** (Ola 5): automatización de releases y validación de
  CHANGELOG/commits — `scripts/check_commits.py` (Conventional Commits sobre
  rangos/ficheros) y `scripts/check_changelog.py` (`[Unreleased]` + sección
  versionada del tag, con `--notes-out`); job `commits` (solo PR) en `ci.yml`
  y `release.yml` que publica el GitHub Release al empujar un tag `v*` (notas
  del CHANGELOG + SBOM CycloneDX; `contents: write` solo al publicar).
- **SPEC-0090** (Ola 5): job `msrv` en `ci.yml` que compila con el piso
  `rust-version` (1.89) y `cargo check --workspace --all-features --locked`;
  `check_ci_config.py` compara el job contra `rust-version` (fuente única).
- **SPEC-0091** (Ola 6): escritura MCP estructurada — `Database::update(table,
  id, scalars)` actualiza escalares de una fila viva por `RecordId` (parcial,
  validando el esquema y reutilizando la ruta de índices del `UPDATE ... WHERE`
  vía `persist_row_update`); tools MCP `update`/`delete` **solo** con
  `McpServer::read_write`, que delegan en la fachada (sin componer SQL), aplican
  el RBAC de la `Database` (-32000 con rol `Read`) y validan `RecordId`
  (-32602). `tools/list` las publica solo en modo escritura.
- **SPEC-0092** (Ola 6): reporte explícito de cobertura de **ramas** (BRDA) —
  `scripts/branch_coverage_report.py` reutiliza el parser de
  `coverage_gate.py` y el `lcov.info` del gate, cita la rama por scope y por
  crate (peor a mejor), aplica `--min-branch` y vuelca `--json-out`; el job
  `coverage` de `ci.yml` sube el artefacto `branch-coverage-report` y
  `check_ci_config.py` lo valida.
- **SPEC-0093** (Ola 6): persistencia del caché semántico — `Database::close`
  escribe `<data>.cache.json` (versión 1, escritura atómica `tmp`+`rename`) y
  `Database::open` lo restaura *best-effort* (fichero ausente/corrupto/versión
  desconocida ⇒ caché vacío, nunca falla la apertura); una mutación retira el
  sidecar y una base cifrada no persiste ni carga el caché en claro
  (`persist_semantic_cache`/`restore_semantic_cache`).
- **SPEC-0094** (Ola 6): gate RED-first — `cargo xtask red <spec>` clasifica la
  salida de `cargo test` (`Red`/`AlreadyPassing`/`Missing`) y exige que los
  tests citados por los ACs de un spec `planned`/`draft` estén en rojo
  (acotado a los crates del spec); job `red-first` (solo PR) que aplica
  `scripts/red_first_gate.py --diff pr.diff` a los specs en curso del diff y
  `check_ci_config.py` lo valida.
- **SPEC-0095** (Ola 6): coherencia de `docs/MVP.md` — alineado con las Olas
  0–6 (lock cross-proceso, `FsyncPolicy`/group commit, `Database::update`,
  catálogo sin techo con overflow, GC de derivados con free-list, caché
  semántico persistente y keystore con rotación) y guard
  `mvp_doc_coherence.rs` que exige las capacidades clave y prohíbe reintroducir
  límites superados (64 KiB, sin rotación, sin GC de derivados).
- **SPEC-0096** (Ola 7): paginación RQL — `LIMIT n OFFSET m` (y `OFFSET n` sin
  `LIMIT`, semántica Postgres); el `OFFSET` se aplica tras `ORDER BY` y antes
  del `LIMIT` (incluido el implícito anti-DoS) en la ruta de escaneo, la de
  agregación (`GROUP BY`) y el `JOIN`; `Display` canónico y errores ruidosos.
- **SPEC-0097** (Ola 7): `SELECT DISTINCT` — deduplica las filas proyectadas
  preservando el orden de primera aparición y se aplica **antes** de
  `OFFSET`/`LIMIT` en las rutas de escaneo, agregación y `JOIN`; `Display`
  canónico (`SELECT DISTINCT ...`) y roundtrip.
- **SPEC-0099** (Ola 7): `LEFT [OUTER] JOIN` (y `INNER JOIN` canónico) — la fila
  exterior sin contraparte se emite una vez con las columnas interiores a `NULL`
  en las rutas indexada y no indexada; `JoinKind` en el AST con `Display`
  canónico (`JOIN`/`LEFT JOIN`).
- **SPEC-0100** (Ola 7): provider LLM del harness — `scripts/llm_provider.py`
  agnóstico con `local` por defecto (**llama.cpp `llama-server`**,
  OpenAI-compatible, sin API key), `openai` para producción (`OPENAI_API_KEY`
  obligatoria) y `ollama` legacy; selección por `RUSCADB_LLM_PROVIDER` con
  overrides CLI; `scripts/llm_judge.py` construye el prompt T2 y
  `--show-config`/`--show-prompt` no tocan la red.
- **SPEC-0101** (Ola 7): compactación de índices derivados — `reap` reconstruye
  HNSW/CSR/FTS desde las filas vivas (`Database::compact_derived_indexes` con
  `rebuild_indexes` *live-aware* en dos pasadas); cierra el límite «sin
  compactación de índices derivados» sin regresión de SPEC-0085.
- **SPEC-0102** (Ola 7): `RETURNING` en DML — `INSERT`/`UPDATE`/`DELETE ...
  RETURNING *|<cols>` devuelven las filas afectadas (valores nuevos en UPDATE,
  valores borrados en DELETE) en lugar de `{"affected": N}`; sin RETURNING el
  contrato no cambia; `RETURNING` vacío es `ParseError`.
- **SPEC-0103** (Ola 7): `RIGHT`/`FULL`/`CROSS JOIN` — `RIGHT [OUTER]` conserva
  las filas interiores sin contraparte (exteriores a `NULL`), `FULL [OUTER]`
  las de ambos lados y `CROSS` el producto cartesiano; `JoinClause.left/right`
  pasan a `Option` (CROSS sin `ON`) con `Display` canónico.
- **SPEC-0104** (Ola 7): varios índices secundarios por tabla — `TableDef`
  guarda una lista de índices (uno por columna), `create_index` admite varios y
  rechaza duplicar la misma columna, el mantenimiento DML/JOIN/planner itera
  todos ellos y el catálogo persiste en las versiones v3/v4 con lectura
  retrocompatible de las bases v1/v2 de un solo índice (marcador `u8`).
- **SPEC-0105** (Ola 7): `rollback` en modo cifrado — al ser el WAL la única
  fuente de verdad (SPEC-0013), el rollback aborta la transacción, vacía el
  buffer pool y **reaplica los commit records con la clave de apertura**
  (`replay`, el mismo camino que la reapertura): el estado vuelve byte a byte
  al último commit **sin tocar `.data` ni `.wal`**; las páginas creadas sin
  commit no están en el WAL y dejan de existir, y los índices en memoria
  (primario + HNSW/CSR/FTS) se reconstruyen desde el heap. Retira el límite
  «rollback no soportado en cifrado» de `docs/MVP.md` (su AC de SPEC-0027
  queda superseded) y lo añade al guard de coherencia del MVP.
- **SPEC-0098** (Ola 7): robustez del gate de mutación — `mutation_diff_gate.py`
  puntúa los dos esquemas de `outcomes.json` (dict legacy y cadena de
  cargo-mutants v25+) y mantiene el fallo ruidoso ante estados desconocidos;
  corregido tras detectar que la última versión de cargo-mutants dejaba el gate
  en **PASS silencioso** (0 mutantes). La corrida adversarial de SPEC-0096 pasó
  a puntuar **MS_diff = 100 %** (7/7).

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
- Quinta edición (ciclo SPEC-0087/0088 + análisis por flujos de trabajo):
  revisión con **3 especialistas en paralelo** (guardian/devops/architect)
  de todos los flujos del repo → hallazgos P0/P1 priorizados. **Aplicados**:
  `check_ci_config.py` y `cargo xtask contract` cableados al job de
  arquitectura de CI (D1/D2), `specs/_template.md` con placeholders seguros
  para trace (D7), mapa de crates de AGENTS.md con los 6 crates faltantes +
  `xtask contract` en el gate local (D8), MSRV unificado a **1.89** en
  README/CONTRIBUTING y en `ruscadb-mcp`, el único crate que seguía en 1.85
  (D9). **Cerradas las Olas 0–4: 13/13 specs, 386 AC trazados.** Pendientes
  como líneas futuras: automatizar releases/tags, branch-coverage en el
  gate, job MSRV en CI, code review multi-agente cableado y gate RED-first
  (roadmap §Estado «Líneas futuras»).
- Sexta edición (Ola 5, flujos P0): **automatización de releases + job MSRV
  implementados** (SPEC-0089/SPEC-0090), cerrando dos de los P0 de proceso del
  análisis 2026-10-13. Quedan como líneas futuras: branch-coverage explícito en
  el gate (el gate ya mide BRDA con `--min-branch`, pero sin job dedicado),
  code review multi-agente cableado (FF-11), gate RED-first con harness y la
  retro T2-LLM-judge programada.

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
