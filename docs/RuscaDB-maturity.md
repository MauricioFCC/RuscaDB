# RuscaDB — Informe de madurez por funcionalidad (2026-11-03)

> Medición basada en evidencia: specs/ACs, suite completa, benchmarks,
> bindings, mutación y revisión de hot paths. Rúbrica y fuentes en
> `docs/research/frontier-2026-11-02-ruscadb-maturity.md` y auditoría de cierre
> en `docs/research/frontier-2026-11-03-spec-audit.md`.
>
> `M(área) = min(ejes)` para no sobredeclarar. L0 ausente, L1 API/stub,
> L2 funcional, L3 verificado, L4 producción.

## 1. Resumen ejecutivo

| Métrica | Valor |
|---|---|
| Specs | **203**, todas `implemented` |
| ACs | **674** (46 specs con 1 AC; 68 con ≤2) |
| Madurez declarada | **203 `real`**, 0 sin `maturity` |
| Suite completa | **223 suites / 1277 tests / 0 fallos** (`-j 4`) |
| Benchmarks | **3 micro** + tests deterministas de recall, grafos y SQL |
| Bindings | py/node conformance con hash común + CI; wasm **14 tests** |
| Mutación | nightly `cargo mutants --workspace` en 8 shards; gate MS ≥ 85 % (local: `gc` 85.3 %, `segmented_wal` 85.7 %) |
| `unsafe` | solo `ruscadb-ffi` (presupuesto controlado) |
| Dependencias default | pure-Rust; PQC/ONNX/pgwire/S3 opcionales |

**Madurez global**: L2/L3 por área; **ningún área L4** todavía (faltan
auditoría externa PQC/Jepsen y datasets ANN-Benchmarks/LDBC completos).

## 2. Madurez por área

| Área | L | Evidencia | Gap principal | Spec |
|---|---|---|---|---|
| Core record/catalog | **L3** | 4 archivos, 7 tests; catálogo v1–v4 con overflow | `drop_column`/`rename_column` ausentes | — |
| Storage/page/buffer | **L3** | 4 archivos, 27 tests; LRU-K, free-list, checksums, sets en hot paths | compactación de páginas | SPEC-0194 |
| WAL/MVCC | **L3** | 7 archivos, 148 marcadores; group commit, segmented, cifrado, crash harness, loom, SSI | sin Jepsen externo | SPEC-0195 (reclaim) |
| Query/RQL | **L3** | 6 archivos, 105 marcadores; SELECT/DML/joins/CTE/ventanas/subconsultas/set-ops; planner por coste + stats + plan cache + EXPLAIN ANALYZE (SPEC-0201/0203/0204) | correlacionadas/laterales | — |
| Índices B-tree/HNSW/IVF/FVS/FTS | **L3** | 13 archivos, 187 marcadores; recall, ACORN, RaBitQ, BM25; recall@10 determinista | sin ANN-Benchmarks externo | SPEC-0198 |
| Grafo | **L2** | CSR, algoritmos, `RELATE`, BFS; 50 marcadores | sin LDBC Graphalytics ni edge delete | SPEC-0198 |
| Multimodal/AI | **L2** | CAS blobs, embeddings estáticos, MLP ONNX-like | sin runtime ONNX real/GPU | — |
| Crypto | **L2** | AEAD L3; ML-KEM verificado + KAT ACVP | auditoría externa PQC pendiente | — |
| Bindings | **L1** | py/node wrappers finos, 0 tests unitarios; wasm 14 tests; scripts de conformance py/node con hash común + CI | cobertura unitaria de wrappers | SPEC-0196 |
| Ops/observabilidad | **L3** | `doctor`, métricas texto, scrape HTTP opt-in con TLS/auth, backup/restore, S3 local, OTLP local | sin OTLP remoto | SPEC-0197/0202 |
| CI/calidad | **L3** | fmt/clippy/doc, trace/contract/audit, fuzz, loom, MSRV, SBOM, mutación | cobertura no medible local | — |
| Docs/specs | **L3** | 203 specs, 674 ACs; índice Diátaxis + gate de docs + maturity completo | — | — |

## 3. Gaps priorizados

| # | Gap | Evidencia | Estado | Spec |
|---|---|---|---|---|
| G1 | `O(n²)` por `Vec::contains` en `gc`, free-list y columnar | `gc.rs`, `paged_file.rs`, `columnar.rs` | ✅ implementado | SPEC-0194 |
| G2 | `TxnManager::aborted` crece sin límite | `mvcc.rs` (SPEC-0193) | ✅ implementado (reap) | SPEC-0195 |
| G3 | py/node sin conformance; solo Rust ejecuta el fixture | `bindings/python/conformance.py`, `bindings/node/conformance.js`, `ruscadb-wasm/tests/conformance.rs` | ✅ implementado (fixture tipado + hash JCS común + tamper gate en CI; WASM parse) | SPEC-0196 |
| G4 | Métricas sin endpoint HTTP de scrape | `observability.rs` | ✅ implementado (405, registry vivo, concurrencia acotada) | SPEC-0197 |
| G5 | Sin benchmarks macro (ANN/LDBC/SQL) | tests de recall/build/graph/SQL + `xtask bench-gate` | ✅ implementado (gate valida cobertura e integridad) | SPEC-0198 |

## 4. Verificación de algoritmos (hot paths)

| Algoritmo | Complejidad actual | Evidencia | Mejora | Coste |
|---|---|---|---|---|
| `gc` reclaim de páginas | `O(P²)` | `retain` + `Vec::contains` | `BTreeSet` ⇒ `O(P log P)` | Bajo |
| `seed_free_pages` | `O(N²)` | `free.iter().any` por página | `BTreeSet` ⇒ `O(N log N)` | Bajo |
| Columnar duplicados | `O(C²)` | `columns.iter().any` | `HashSet` ⇒ `O(C)` | Bajo |
| HNSW `connect` | `O(M·k)` acotado | `list.contains` con M=16 | `SmallVec`/bitset opcional | Medio |
| Join no-equi | `O(n·m)` | nested loop | solo equi usa índice/hash (SPEC-0200) | Medio |
| MVCC aborted | `O(1)` memoria tras reap | `BTreeSet` + `forget_aborted` | horizonte de snapshot histórico | Bajo |

## 5. Medición local y límites

- **Cobertura local (medida, 2026-11-03)**: `rustup` + toolchain 1.99.0 +
  `llvm-tools-preview`; `cargo llvm-cov --no-clean -p ruscadb -p ruscadb-core
  -p ruscadb-txn -p ruscadb-wal --all-features --lcov` →
  core **84.9 %**, txn **97.0 %**, wal **94.0 %**; gate PASS. BRDA n/a porque
  el export lcov no lleva `--branch-coverage` (igual que CI).
- **ANN-Benchmarks/LDBC/TPC-H**: requieren datasets/harness externos; los
  gates usan fixtures deterministas (SPEC-0198) y no se descargan aquí.
- **Jepsen/auditoría externa**: requieren infraestructura/proveedor.

## 6. Criterio de cierre de este informe

- Specs de mejora con ACs medibles y tests activos (sin scaffolds `#[ignore]`).
- Gates `trace`/`contract`/`audit` verdes: 203 specs, 674 ACs.
- Los gaps no implementados quedan declarados en este informe y en el backlog.
