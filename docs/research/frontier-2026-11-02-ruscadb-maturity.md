# Frontier research — Madurez de RuscaDB y gaps de ingeniería (2026-11-02)

> Tarea explícita: verificar algoritmos/funcionalidades, medir madurez por
> funcionalidad y estructurar specs de mejora para los gaps identificados.
> Paper trail con URLs leídas; lo no verificado se marca `[UNVERIFIED]`.

## 1. Estado del arte (fuentes leídas)

| Fuente | Aporte | URL |
|---|---|---|
| SQLite — *How SQLite Is Tested* | 4 harnesses independientes, 100% branch coverage, SLT contra PostgreSQL/MySQL/SQL Server/Oracle, OOM/IO/crash/fuzz, 590× test code vs core. | https://www.sqlite.org/testing.html |
| ANN-Benchmarks | Benchmark estándar de ANN: recall@k vs queries/s, datasets públicos (SIFT, GIST, glove). | https://ann-benchmarks.com/ |
| HNSW (Malkov & Yashunin, 2016/2018) | Grafo jerárquico con escala logarítmica; base de la implementación vectorial. | https://arxiv.org/abs/1603.09320 |
| RaBitQ (Gao & Long, SIGMOD 2024) | Cuantización de 1 bit/dim con cota teórica de error; base de `rabitq.rs`. | https://arxiv.org/abs/2405.12497 |
| LDBC Graphalytics | Benchmark industrial de 6 algoritmos de grafos con datasets y salidas de referencia. | https://ldbcouncil.org/benchmarks/graphalytics/ |
| PostgreSQL — *Routine Vacuuming* | MVCC deja versiones muertas; hace falta reclaim y protección contra wraparound. | https://www.postgresql.org/docs/current/routine-vacuuming.html |
| Prometheus — *Exposition formats* | Texto/HTTP `text/plain; version=0.0.4`, `# HELP`/`# TYPE`, muestras por línea. | https://prometheus.io/docs/instrumenting/exposition_formats/ |

## 2. Hipótesis de medición de madurez

Criterios: **A** exactitud de la medida, **C** coste, **R** reproducibilidad,
**S** sensibilidad a gaps reales.

| # | Hipótesis | A | C | R | S | Veredicto |
|---|---|---|---|---|---|---|
| H1 | Madurez = nº de tests | Baja (1212 tests, pero py/node 0) | Bajo | Alta | Baja | Descartada. |
| H2 | Madurez = `status` de spec + ACs | Media (188/188 implemented, pero 46 specs con 1 AC y 130 sin `maturity`) | Bajo | Alta | Media | Insuficiente sola. |
| H3 | Rúbrica multi-eje L0–L4 (semántica, verificación, rendimiento, operación, seguridad) con evidencia | Alta | Medio | Alta | Alta | **Elegida**. |
| H4 | Certificación externa (ISO 25010 / Jepsen) | Alta | Alto | Media | Alta | Futuro para L4. |

## 3. Rúbrica L0–L4

```text
L0 Ausente     : no hay código ni spec.
L1 API/stub    : compila, sin semántica real.
L2 Funcional   : semántica + tests unitarios.
L3 Verificado  : integración + PBT/mutación + recovery/errores + docs.
L4 Producción  : benchmarks/conformance multi-binding + auditoría/operación.
M(área) = min(ejes)   # evita sobredeclarar por un eje fuerte
```

## 4. Evidencia medida (2026-11-02)

- **188 specs**, **618 ACs**, 188 `implemented`; 58 `real`, 130 sin `maturity`.
- **46 specs con 1 AC**, **22 con 2 ACs** (68 ≤ 2): profundidad de aceptación
  desigual.
- Suite completa: **209 suites, 1212 tests, 0 fallos** (`-j 2`).
- Benchmarks: **3** (`facade_smoke`, `knn_latency`, `wal_commit`).
- Bindings: `ruscadb-py` 16 líneas/0 tests, `ruscadb-node` 17/0,
  `ruscadb-wasm` 1090/14 tests; conformance fase 2 no ejecutada.
- Cobertura: `cargo-llvm-cov` 0.9.1 presente pero falta
  `llvm-tools-preview`; no se pudo medir localmente (CI sí lo hace).
- Algoritmos: 3 hot paths con `Vec::contains` dentro de bucles (ver §5).
- MVCC: `aborted: BTreeSet<TxId>` introduce una fuga de memoria potencial.

## 5. Gap analysis (con evidencia de código)

### G1 — `O(n²)` por pertenencia en `Vec` (hot paths)

- `crates/ruscadb/src/gc.rs`: `definition.pages.retain(|raw| !pages.contains(raw))`
  → `O(P²)` por tabla.
- `crates/ruscadb-storage/src/paged_file.rs`: `seed_free_pages` usa
  `self.free.iter().any(...)` por página → `O(N²)` al cargar la free-list.
- `crates/ruscadb/src/columnar.rs`: validación de columnas duplicadas con
  `columns.iter().any(...)` → `O(C²)` con C hasta 65 536.

Mejora: `BTreeSet<u64>`/`HashSet<&str>` ⇒ `O(N log N)`/`O(N)` y memoria `O(N)`.
Formulas: `T(N) = c·N²` → `T(N) = c·N log N`; gate N/2N/4N.

### G2 — `aborted` de MVCC sin reclaim

`TxnManager::aborted: BTreeSet<TxId>` (SPEC-0193) crece con cada rollback y no
se purge. En un proceso largo, `O(A)` memoria. PostgreSQL resuelve esto con
`VACUUM` y un horizonte de snapshots. Mejora: purgar `tx < min_snapshot` tras
cada snapshot/commit; memoria `O(A_activas)`.

### G3 — Bindings sin conformance

`ruscadb-py` y `ruscadb-node` son wrappers de 16/17 líneas y 0 tests; el
fixture `conformance/basic.json` solo corre en Rust. Gap de madurez L1/L2.

### G4 — Observabilidad sin transporte

`MetricsRegistry::export()` produce texto Prometheus, pero no hay endpoint
HTTP de scrape; `otel_real` es local. Gap L2.

### G5 — Benchmarks insuficientes

Solo 3 microbenchmarks; no hay ANN-Benchmarks (recall/QPS), LDBC Graphalytics
ni un workload SQL tipo TPC-H. Sin gates de regresión macro.

## 6. Decisión

Crear specs de mejora para G1–G5 (SPEC-0194..0198) con ACs medibles, tests RED
(`#[ignore]`) y gates N/2N/4N. Prioridad:

1. **G1** (impacto algorítmico directo, medible).
2. **G2** (corrección de memoria a largo plazo).
3. **G3** (madurez de bindings).
4. **G4** (operabilidad).
5. **G5** (benchmarks; coste alto, se especifica en fases).

## 7. Lo no aplicado y por qué

- Cobertura local: falta `llvm-tools-preview` (no hay `rustup` en la imagen);
  el CI ya ejecuta `cargo llvm-cov` con gates core≥80, txn/wal≥90.
- ANN-Benchmarks/LDBC/TPC-H: datasets y harness externos; se especifican pero
  no se descargan en esta sesión.
- Auditoría externa de ML-KEM: requiere proveedor; sigue como riesgo residual.
