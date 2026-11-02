# RuscaDB — Informe de madurez por funcionalidad (2026-11-02)

> Medición basada en evidencia: specs/ACs, suite completa, benchmarks,
> bindings, mutación y revisión de hot paths. Rúbrica y fuentes en
> `docs/research/frontier-2026-11-02-ruscadb-maturity.md`.
>
> `M(área) = min(ejes)` para no sobredeclarar. L0 ausente, L1 API/stub,
> L2 funcional, L3 verificado, L4 producción.

## 1. Resumen ejecutivo

| Métrica | Valor |
|---|---|
| Specs | **188**, todas `implemented` |
| ACs | **618** (46 specs con 1 AC; 68 con ≤2) |
| Madurez declarada | **58 `real`**, 130 sin `maturity` |
| Suite completa | **209 suites / 1212 tests / 0 fallos** (`-j 2`) |
| Benchmarks | **3** (`facade_smoke`, `knn_latency`, `wal_commit`) |
| Bindings | py **0 tests**, node **0**, wasm **14** |
| Mutación medida | `gc` **85.3 %**, `segmented_wal` **85.7 %** |
| `unsafe` | solo `ruscadb-ffi` (presupuesto controlado) |
| Dependencias default | pure-Rust; PQC/ONNX/pgwire/S3 opcionales |

**Madurez global**: L2/L3 por área; **ningún área L4** todavía (faltan
benchmarks macro, conformance multi-binding y auditoría externa).

## 2. Madurez por área

| Área | L | Evidencia | Gap principal | Spec |
|---|---|---|---|---|
| Core record/catalog | **L3** | 4 archivos, 7 tests; catálogo v1–v4 con overflow | `drop_column`/`rename_column` ausentes | — |
| Storage/page/buffer | **L3** | 4 archivos, 27 tests; LRU-K, free-list, checksums | `O(N²)` al sembrar free-list | SPEC-0194 |
| WAL/MVCC | **L3** | 7 archivos, 148 marcadores; group commit, segmented, cifrado, crash harness, loom, SSI | `aborted` sin reclaim; sin Jepsen | SPEC-0195 |
| Query/RQL | **L3** | 6 archivos, 105 marcadores; SELECT/DML/joins/CTE/ventanas/subconsultas/set-ops | `MERGE`, plan cache, correlacionadas/laterales | — |
| Índices B-tree/HNSW/IVF/FVS/FTS | **L3** | 13 archivos, 187 marcadores; recall, ACORN, RaBitQ, BM25 | sin ANN-Benchmarks (recall/QPS) | SPEC-0198 |
| Grafo | **L2** | CSR, algoritmos, `RELATE`, BFS; 50 marcadores | sin LDBC Graphalytics ni edge delete | SPEC-0198 |
| Multimodal/AI | **L2** | CAS blobs, embeddings estáticos, MLP ONNX-like | sin runtime ONNX real/GPU | — |
| Crypto | **L2** | AEAD L3; ML-KEM verificado + KAT ACVP | auditoría externa PQC pendiente | — |
| Bindings | **L1** | py/node wrappers 16/17 líneas, 0 tests; wasm 14 tests | conformance fase 2 | SPEC-0196 |
| Ops/observabilidad | **L2** | `doctor`, métricas texto, backup/restore, S3 local, OTLP local | sin scrape HTTP | SPEC-0197 |
| CI/calidad | **L3** | fmt/clippy/doc, trace/contract/audit, fuzz, loom, MSRV, SBOM, mutación | cobertura no medible local | — |
| Docs/specs | **L2** | 188 specs, 618 ACs; 130 sin `maturity` | profundidad de ACs y metadata | — |

## 3. Gaps priorizados

| # | Gap | Evidencia | Impacto | Spec |
|---|---|---|---|---|
| G1 | `O(n²)` por `Vec::contains` en `gc`, free-list y columnar | `gc.rs:181`, `paged_file.rs:87`, `columnar.rs:589` | Latencia en reclaim/carga/validación | **SPEC-0194** |
| G2 | `TxnManager::aborted` crece sin límite | `mvcc.rs` (SPEC-0193) | Memoria en procesos largos | **SPEC-0195** |
| G3 | py/node sin conformance; solo Rust ejecuta el fixture | `crates/ruscadb-py`, `ruscadb-node` | Madurez de bindings L1 | **SPEC-0196** |
| G4 | Métricas sin endpoint HTTP de scrape | `observability.rs` | Operabilidad L2 | **SPEC-0197** |
| G5 | Sin benchmarks macro (ANN/LDBC/SQL) | `benches/` (3 micro) | Regresiones no detectadas | **SPEC-0198** |

## 4. Verificación de algoritmos (hot paths)

| Algoritmo | Complejidad actual | Evidencia | Mejora | Coste |
|---|---|---|---|---|
| `gc` reclaim de páginas | `O(P²)` | `retain` + `Vec::contains` | `BTreeSet` ⇒ `O(P log P)` | Bajo |
| `seed_free_pages` | `O(N²)` | `free.iter().any` por página | `BTreeSet` ⇒ `O(N log N)` | Bajo |
| Columnar duplicados | `O(C²)` | `columns.iter().any` | `HashSet` ⇒ `O(C)` | Bajo |
| HNSW `connect` | `O(M·k)` acotado | `list.contains` con M=16 | `SmallVec`/bitset opcional | Medio |
| Join no-equi | `O(n·m)` | nested loop | hash join solo para equi | Medio |
| MVCC aborted | `O(A)` memoria | `BTreeSet` sin purge | horizonte de snapshot | Bajo |

## 5. Lo que no se pudo medir

- **Cobertura local**: `cargo-llvm-cov` existe pero falta
  `llvm-tools-preview` y no hay `rustup` en la imagen. El CI sí ejecuta
  `cargo llvm-cov` con gates core≥80, txn/wal≥90.
- **ANN-Benchmarks/LDBC/TPC-H**: requieren datasets/harness externos; se
  especifican en SPEC-0198, no se descargan aquí.
- **Jepsen/auditoría externa**: requieren infraestructura/proveedor.

## 6. Criterio de cierre de este informe

- Specs de mejora creadas con ACs medibles y tests RED (`#[ignore]`).
- Gates `trace`/`contract`/`audit` siguen verdes.
- Los gaps no implementados quedan declarados en este informe y en el backlog.
