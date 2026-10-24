# RuscaDB — Investigación de frontera (estado del arte 2026)

> Documento de investigación. Fecha: 2026-10-24.  
> Fuentes: web (arXiv, crates.io, docs.rs, blogs de LanceDB, SurrealDB, DuckDB, redb, SQLRite, Stoolap, MoteDB, LLKV, Picovolt, FFS, powdb, Turso/libSQL).

## 1. Panorama competitivo 2026

| Proyecto | Lenguaje | Modelo | Concurrencia | Almacenamiento | Diferenciador |
|---|---|---|---|---|---|
| **SurrealDB** | Rust | Multi-modelo (doc, graph, TS, relacional, geo, KV) | Single-node / cluster | RocksDB / SurrealKV | SurrealQL unificado, ONNX inference, WASM, sync protocol |
| **LanceDB** | Rust | Multimodal vector + tabular | Embedded | Lance columnar (Arrow) | Blob V2, IVF-HNSW, full-text, feature engineering |
| **DuckDB** | C++ | Analítico SQL | In-process | Columnar (morsel-driven) | SQL completo, parquet, UDFs |
| **redb** | Rust | KV | MVCC (readers + writer sin bloqueo) | Copy-on-write B+trees | Zero-copy, crash-safe, savepoints |
| **SQLRite** | Rust | SQL + vector + FTS | BEGIN CONCURRENT (multi-writer) | B-tree + WAL | Snapshot isolation, row-level conflict detection |
| **Stoolap** | Rust | SQL OLTP+OLAP | MVCC | Hot buffer + cold columnar (LZ4) | Adaptive compaction, zone maps, bloom filters |
| **MoteDB** | Rust | Multimodal AI-native | ACID | Columnar + WAL | 2.4M rows/s insert, Snappy, spatial index |
| **LLKV** | Rust | SQL | MVCC | Arrow columnar sobre KV | Zero-copy Arrow, predicate pushdown |
| **Picovolt** | Rust | SQL embedded | MVCC history | VLE (dir ↔ single-file .pvdb) | Time-travel, CAS dedup, columnar compression |
| **FFS** | Rust | Graph (Cypher) | MVCC/WAL | 8 KiB pages, checkpoint a S3 | Attach read-only, checkpoint pins |
| **Turso/libSQL** | Rust | SQLite-compatible | Embedded replica sync | SQLite + replication | HTTP sync, local replica |
| **powdb** | Rust | KV/SQL | Read-write open refuses while readers exist | Single-file | Read-only serving, backup-to-restore |
| **SQLRite** | Rust | SQL + vector | BEGIN CONCURRENT | B-tree + WAL | Row-level conflict detection |

## 2. Técnicas de frontera aplicables

### 2.1 Concurrencia (MVCC)

| Técnica | Fuente | Aplicabilidad a RuscaDB |
|---|---|---|
| **Copy-on-write B+trees** | redb, LMDB | Alta. RuscaDB usa heap paginado; migrar a CoW B+trees permitiría snapshot readers sin bloqueo. Esfuerzo: alto (refactor de storage). |
| **BEGIN CONCURRENT** (multi-writer) | SQLRite, rust_sqlite | Media. Requiere version index en memoria y row-level conflict detection. Esfuerzo: alto. |
| **Epoch-based reclamation (RCU)** | Linux kernel, left-right lock | Media. Permite readers sin coordinación con el writer. Esfuerzo: medio. |
| **Snapshot isolation con timestamp** | txn_db, skipdb | Alta. Ya tenemos MVCC con tx_id; falta propagar snapshot al lector cross-proceso. |
| **Read-only open sin lock** | powdb, RuscaDB (SPEC-0109) | ✅ Ya implementado. Mejora: capturar snapshot consistente (hecho con last_committed_tx). |

### 2.2 Almacenamiento

| Técnica | Fuente | Aplicabilidad |
|---|---|---|
| **Columnar (Arrow/Lance)** | LLKV, LanceDB, DuckDB | Alta para analytics y vectores grandes. Esfuerzo: alto (nuevo engine). |
| **Lance format v2.2** | LanceDB | Alta para multimodal. Blob V2, 50%+ compresión, lazy reads. Esfuerzo: medio (integrar lance crate). |
| **Page compression (zstd/LZ4)** | RocksDB, PostgreSQL, Stoolap | Alta. RuscaDB tiene RLE stub. Esfuerzo: bajo (añadir zstd/lz4 crate). |
| **Single-file deployment (VLE)** | Picovolt | Media. Un solo fichero .db en lugar de .data + .wal + .manifest. Esfuerzo: medio. |
| **Checkpoint a object storage (S3)** | FFS | Media. Backup/restore a S3-compatible. Esfuerzo: medio. |
| **Time-travel / versioning** | Picovolt, CrepeDB | Media. MVCC history permite leer estado anterior. Esfuerzo: medio. |
| **Copy-on-write branching** | Zeta-Lite | Baja. Fork/merge/rebase de bases. Esfuerzo: alto. |

### 2.3 Búsqueda vectorial

| Técnica | Fuente | Aplicabilidad |
|---|---|---|
| **Filtered ANN (RACORN-1, ACORN-1)** | arXiv 2607.00768 | Alta. RuscaDB tiene iFVS (pre/in/post). Mejora: adaptive fallback para baja selectividad. |
| **SIEVE (collection of indexes)** | VLDB 2025 | Media. Múltiples índices por filtro. RuscaDB tiene un índice por columna. |
| **IVF-PQ / IVF-HNSW** | LanceDB, ParqDB | Alta para datasets grandes. RuscaDB usa HNSW puro. Esfuerzo: medio. |
| **Quantization (LVQ8)** | ParqDB | Media. Reduce memoria 4-8x. Esfuerzo: medio. |
| **Hybrid search (vector + FTS + scalar)** | LanceDB, SurrealDB | ✅ Ya implementado (hybrid.rs, RRF, MaxSim). |

### 2.4 Búsqueda de texto (FTS)

| Técnica | Fuente | Aplicabilidad |
|---|---|---|
| **BM25** | RuscaDB (SPEC-0009) | ✅ Ya implementado. |
| **Stemming (Snowball)** | Elasticsearch | Media. RuscaDB tiene stemmer básico (Porter-like). Mejora: Snowball multi-idioma. |
| **Fuzzy search (Levenshtein)** | RuscaDB (SPEC-0117) | ✅ Ya implementado. |
| **Phrase matching** | RuscaDB (SPEC-0117) | ✅ Ya implementado. |
| **Full-text + vector hybrid** | LanceDB | ✅ Ya implementado (hybrid.rs). |

### 2.5 Time-series

| Técnica | Fuente | Aplicabilidad |
|---|---|---|
| **Downsampling** | RuscaDB (SPEC-0118) | ✅ Ya implementado (avg/max/min). |
| **Retention policies** | InfluxDB, TimescaleDB | Media. RuscaDB no tiene retention automática. Esfuerzo: bajo. |
| **Continuous aggregates** | TimescaleDB | Media. Materialized views pre-calculadas. Esfuerzo: medio. |
| **Compression (Gorilla, Delta)** | InfluxDB | Media. Compresión específica para TS. Esfuerzo: medio. |

### 2.6 Observabilidad

| Técnica | Fuente | Aplicabilidad |
|---|---|---|
| **Prometheus export** | RuscaDB (SPEC-0119) | ✅ Stub implementado. Necesita exportador real. |
| **OpenTelemetry tracing** | RuscaDB (SPEC-0080) | ✅ Spans implementados. Necesita OTLP exporter. |
| **Metrics (histograms, gauges)** | Prometheus | Media. RuscaDB tiene contadores. Mejora: histograms, gauges, summaries. |

### 2.7 Seguridad

| Técnica | Fuente | Aplicabilidad |
|---|---|---|
| **SQL injection prevention** | OWASP | ✅ RQL usa parser tipado, no concatenación. |
| **Path traversal** | OWASP | ✅ Blob store usa hash-based paths. |
| **RBAC** | RuscaDB (SPEC-0066) | ✅ Roles Admin/Write/Read. |
| **Cifrado en reposo** | RuscaDB (SPEC-0013) | ✅ XChaCha20-Poly1305 + Argon2id. |
| **KMS/HSM** | AWS KMS, HashiCorp Vault | Pendiente. Esfuerzo: medio. |
| **Audit log** | RuscaDB (SPEC-0073) | ✅ Audit persistido con CRC32C. |

## 3. Gaps identificados (priorizados por ROI)

### P0 — Alto impacto, esfuerzo medio
1. **zstd/LZ4 compression para WAL** — reduce I/O 20-40%. Reemplazar RLE stub con zstd (optional dep).
2. **MVCC snapshot readers sin bloqueo** — ya tiene open_readonly; mejorar con snapshot consistente (last_committed_tx ya implementado).
3. **Retention policies para time-series** — downsampling automático + TTL.
4. **Fuzzy scoring en FTS** — BM25 + Levenshtein para ranking fuzzy.

### P1 — Alto impacto, esfuerzo alto
5. **Arrow columnar engine real** — integrar arrow crate para zero-copy reads.
6. **IVF-PQ para vectores grandes** — reduce memoria 4-8x para datasets >1M.
7. **BEGIN CONCURRENT (multi-writer)** — row-level conflict detection.
8. **OpenTelemetry OTLP exporter** — exportar traces/metrics a Jaeger/Prometheus.

### P2 — Medio impacto, esfuerzo alto
9. **PostgreSQL wire protocol real** — integrar pgwire crate.
10. **Single-file deployment** — VLE (directory ↔ single-file).
11. **Checkpoint a S3** — backup/restore a object storage.
12. **Time-travel** — leer estado anterior por tx_id.
13. **KMS/HSM externo** — custodia de claves.
14. **Copy-on-write B+trees** — refactor de storage (mayor esfuerzo).

### P3 — Bajo impacto / fuera de alcance
15. **Geospatial proyecciones** — WGS84, tiles.
16. **Sync protocol / replicación** — distribuido.
17. **Copy-on-write branching** — fork/merge/rebase.
18. **Continuous aggregates** — materialized views.

## 4. Plan de implementación (próxima ola)

### Ola 10 — Compresión + Retention + Fuzzy scoring (P0)
- SPEC-0121: zstd compression para WAL (optional dep `zstd`)
- SPEC-0122: retention policies para time-series (TTL + downsampling automático)
- SPEC-0123: fuzzy scoring en FTS (BM25 + Levenshtein ranking)

### Ola 11 — Columnar + IVF-PQ + Observabilidad real (P1)
- SPEC-0124: Arrow columnar engine (optional dep `arrow`)
- SPEC-0125: IVF-PQ para vectores grandes
- SPEC-0126: OpenTelemetry OTLP exporter

### Ola 12 — Wire protocol + Backup + Time-travel (P2)
- SPEC-0127: PostgreSQL wire protocol real (pgwire crate)
- SPEC-0128: Checkpoint a S3 (backup/restore)
- SPEC-0129: Time-travel (read as of tx_id)

## 5. Referencias

- redb design doc: https://github.com/cberner/redb/blob/master/docs/design.md
- Lance format: https://lance.org/
- LanceDB blog: https://blog.lancedb.com/
- SQLRite: https://www.sqlritedb.com/
- Stoolap: https://github.com/stoolap/stoolap
- MoteDB: https://crates.io/crates/motedb
- LLKV: https://docs.rs/llkv
- Picovolt: https://docs.rs/picovolt
- FFS: https://www.ffsdb.com/roadmap
- Turso/libSQL: https://turso.tech/
- RACORN-1: https://arxiv.org/abs/2607.00768
- SIEVE: https://dl.acm.org/doi/10.14778/3749646.3749725
- Filtered ANN survey: https://arxiv.org/html/2602.11443
- Zeta-Lite: https://arxiv.org/html/2609.01818
- pgwire: https://docs.rs/pgwire
- zstd Rust: https://crates.io/crates/zstd
- Arrow: https://crates.io/crates/arrow
- ort (ONNX): https://crates.io/crates/ort
- zstd pure-Rust: https://github.com/structured-world/structured-zstd
