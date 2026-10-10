---
id: SPEC-0059
feature: bench_regression
status: accepted
owner: perf-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-vector, ruscadb-wal]
  out_of_scope: [optimizar (solo medir), CI con hardware dedicado, MSRV bench]
fr:
  - { id: FR-0059-01, desc: "dev-dep criterion en los crates medidos (workspace)" }
  - { id: FR-0059-02, desc: "benches: point-read, write c=64 group-commit, KNN latencia, ingest blob" }
  - { id: FR-0059-03, desc: "baseline committed para comparacion entre ramas" }
  - { id: FR-0059-04, desc: "gate nightly P95 <= +5% (alert-only)" }
nf:
  - { id: NF-0059-01, desc: "benches < 60 min en nightly, < 5 min en smoke local" }
  - { id: NF-0059-02, desc: "cero impacto en builds normales (solo benches/ + dev-deps)" }
acceptance_criteria:
  - id: AC-0059-01
    given: los benches definidos
    when: se ejecutan en modo smoke
    then: compilan, corren y reportan sin crash
    test: test_ac_0059_01_benches_compile_and_run
exit_criteria:
  - cargo bench --workspace -- --test (smoke) verde
  - baseline inicial guardado en benches/baselines/
rollback:
  - revertir benches + dev-deps (sin efecto en lib/tests)
sandbox:
  - cargo bench -p ruscadb --bench facade_smoke -- --test
---

# SPEC-0059 — Benchmarks criterion + gate P95 (T3)

## Contexto

T3 exige `criterion` + P95 ≤ +5% pero no hay ni `benches/` ni la dependencia.
Solo medir (optimizar queda fuera): point-read p50/p95, write QPS c=64,
latencia KNN, ingest blob ≥500 MB/s como meta. Baseline versionado para
`critcmp` entre ramas.

## Criterios de aceptación

- **AC-0059-01** — benches compilan y corren en smoke.

## Trazabilidad

Tests `test_ac_0059_<nn>_*`; verificado por `cargo xtask trace`.
