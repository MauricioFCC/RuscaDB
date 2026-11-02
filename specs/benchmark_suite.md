---
id: SPEC-0198
feature: benchmark_suite
status: proposed
maturity: stub
owner: performance-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-vector, ruscadb-graph, xtask]
  files:
    - crates/ruscadb/benches/facade_smoke.rs
    - crates/ruscadb-vector/benches/knn_latency.rs
    - crates/ruscadb/benches/graph_algorithms.rs
    - crates/ruscadb/benches/sql_workload.rs
    - crates/xtask/src/main.rs
  out_of_scope: [descarga de datasets ANN/LDBC/TPC-H, GPU, comparación con otros motores]
fr:
  - { id: FR-0198-01, desc: "benchmark vectorial recall@10 vs QPS sobre dataset fijo" }
  - { id: FR-0198-02, desc: "benchmark de grafos (BFS, PageRank, componentes, SSSP) contra oraculo" }
  - { id: FR-0198-03, desc: "workload SQL analitico (scan/filter/group/join/order) con resultados canonicos" }
  - { id: FR-0198-04, desc: "gate de regresion relativo (ratio N/2N/4N y QPS/recall)" }
nf:
  - { id: NF-0198-01, desc: "datasets pequenos y deterministas versionados; sin red en CI" }
  - { id: NF-0198-02, desc: "criterion + xtask bench-gate; umbrales declarados" }
acceptance_criteria:
  - id: AC-0198-01
    given: un dataset vectorial determinista (SIFT-like, 2k x 128)
    when: se ejecuta KNN con HNSW
    then: recall@10 >= 0.95 y el QPS se reporta
    test: test_ac_0198_01_vector_benchmark
  - id: AC-0198-02
    given: un grafo determinista con salida de referencia
    when: se ejecutan BFS/PageRank/componentes/SSSP
    then: la salida coincide con el oraculo congelado
    test: test_ac_0198_02_graph_benchmark
  - id: AC-0198-03
    given: un workload SQL con resultados canonicos
    when: se ejecuta el bench
    then: cada consulta coincide y se reporta latencia
    test: test_ac_0198_03_sql_benchmark
  - id: AC-0198-04
    given: dos corridas (base y head)
    when: se aplica el gate de regresion
    then: una caida > X% de QPS o > Y ms de p95 falla con detalle
    test: test_ac_0198_04_regression_gate
exit_criteria:
  - cargo bench -p ruscadb --no-run
  - cargo bench -p ruscadb-vector --no-run
  - cargo test -p ruscadb --test bench_smoke -- --ignored
  - cargo xtask bench-gate --baseline <json> --head <json>
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar los benches/gate; no toca el runtime
sandbox:
  - cargo bench -p ruscadb-vector --bench knn_latency -- --test
---

# SPEC-0198 — Suite de benchmarks macro y gate de regresión

## Contexto

Hoy solo hay 3 microbenchmarks (`facade_smoke`, `knn_latency`, `wal_commit`).
No se mide recall/QPS vectorial, ni algoritmos de grafos, ni un workload SQL.
ANN-Benchmarks y LDBC Graphalytics son los estándares de referencia, pero
descargan datasets; esta spec usa fixtures pequeños y deterministas para CI,
dejando los datasets completos para nightly.

## Diseño

1. **Vector**: dataset sintético determinista (2k vectores, dim 128, seed fija),
   ground truth exacto; medir `recall@10` y QPS con HNSW.
2. **Grafo**: grafo sintético con salida congelada; BFS, PageRank, componentes
   conexas, SSSP (pesos 1).
3. **SQL**: tabla analítica pequeña; `scan`, `filter`, `group by`, `join`,
   `order by`, `limit`; resultados canónicos.
4. **Gate**: `xtask bench-gate` compara JSON de base/head; falla si
   `QPS_head < QPS_base·(1-x)` o `p95_head > p95_base·(1+y)`.
5. Nightly puede sustituir fixtures por ANN-Benchmarks/LDBC/TPC-H.

## Complejidad

```text
HNSW: O(log N) esperado por consulta (Malkov & Yashunin)
BFS:  O(V + E)
PageRank: O(k·(V + E)) para k iteraciones
SQL scan: O(N); group: O(N); join hash: O(N + M)
```

## TDD adversarial

- Ground truth exacto para recall (oráculo congelado).
- Salidas de grafos Comparadas con `BTreeSet`/orden canónico.
- Resultados SQL canónicos.
- Gate con dos JSON sintéticos (mejora/regresión).
- PBT: el dataset y las queries son deterministas con la misma seed.

## Riesgos

- Ruido de CI: el gate usa ratios y p95, no valores absolutos; se ejecuta en
  runner dedicado o nightly.
- Los fixtures deben ser pequeños (<5 MB) para no inflar el repo.

## Estado (2026-11-02)

- **AC-0198-01** activo: `crates/ruscadb-vector/tests/benchmarks.rs` mide
  recall@10 >= 0.95 sobre 2k vectores dim 16 (LCG determinista).
- **AC-0198-02** activo: `crates/ruscadb-graph/tests/benchmarks.rs` valida BFS,
  componentes y PageRank contra oráculo.
- **AC-0198-03** activo: workload SQL canónico (filter/group/join/order/limit)
  en `crates/ruscadb/tests/gap_specs.rs`.
- Pendiente: AC-0198-04 (`xtask bench-gate`) y benches criterion formales.
