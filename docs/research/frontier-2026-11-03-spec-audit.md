# Auditoría frontier de SPEC-0194..0198 (2026-11-03)

> Tarea explícita: revisar si las specs se implementaron correctamente, aplicar
> lo faltante y buscar mejoras en la implementación. Ciclo
> `frontier-engineering` (load → research → design → spec diff → TDD → docs).
> Sin datos privados ni rutas personales.

## 1. Load (activos relevantes)

| Activo | Uso |
|---|---|
| `specs/set_membership_hotpaths.md` (SPEC-0194) | sets en hot paths |
| `specs/mvcc_aborted_reclamation.md` (SPEC-0195) | reclaim de `aborted` |
| `specs/bindings_conformance.md` (SPEC-0196) | conformance py/node/wasm |
| `specs/metrics_http.md` (SPEC-0197) | scrape Prometheus |
| `specs/benchmark_suite.md` (SPEC-0198) | benchmarks + gate |
| `crates/ruscadb/src/conformance.rs` | contrato canónico |
| `crates/xtask/src/bench.rs` | gate de regresión |
| `bindings/python/conformance.py`, `bindings/node/conformance.js` | runners |
| `crates/ruscadb/tests/conformance/basic.json` | fixture compartido |
| `docs/RuscaDB-maturity.md`, `docs/RuscaDB-backlog.md` | estado y gaps |

IDP: no se reimplementó nada; se auditó lo existente y se aplicaron solo los
huecos medibles (ver §5).

## 2. Research SOTA — fuentes leídas

| Fuente | URL | Qué aporta |
|---|---|---|
| RFC 8785 (JCS) | https://www.rfc-editor.org/rfc/rfc8785 | canonicalización JSON determinista: orden de propiedades y serialización de primitivas |
| Prometheus exposition formats | https://prometheus.io/docs/instrumenting/exposition_formats/ | `Content-Type: text/plain; version=0.0.4`, line-oriented, última línea con `\n` |
| Bencher thresholds | https://bencher.dev/docs/explanation/thresholds/ | *lower/upper boundary* por throughput/latencia; tests percentage/z-score/t-test |
| ANN-Benchmarks | https://ann-benchmarks.com/ | eje canónico recall vs QPS, k=10, ground truth exacto |
| sqllogictest (SQLite) | https://www.sqlite.org/sqllogictest/doc/trunk/about.wiki | comparación de resultados entre motores — **[UNVERIFIED]**: la página exige verificación anti-robot y no devolvió contenido |

Búsqueda web (`websearch`) sin resultados en esta sesión; las URLs anteriores
se leyeron directamente con `webfetch`.

## 3. Hipótesis comparadas

Criterios: accuracy (fidelidad semántica), latency (coste en CI), scalability,
robustness (fallos ruidosos vs silenciosos), reproducibility.

### 3.1 Canonicalización multi-binding

| # | Hipótesis | Accuracy | Latency | Scalability | Robustness | Reproducibility | Veredicto |
|---|---|---|---|---|---|---|---|
| H1 | Hash del JSON de transporte tal cual | Baja: `2.0` difiere entre Python/Rust/JS; `"Null"` unitario | Baja | O(bytes) | Baja: mismatch espurio | Baja | Rechazada |
| H2 | JSON canónico (claves/filas ordenadas) + números JCS para flotantes integrales | Alta para el fixture | O(f·c log f) | O(filas·columnas) | Alta: mismatch nombra binding/caso | Alta | **Elegida** |
| H3 | JCS completo con librería (`serde_jcs`) | Muy alta (ECMAScript exacto) | Igual | Igual | Alta | Alta | Diferida: dependencia nueva y el fixture no usa exponentes extremos |
| H4 | Comparación fila a fila estilo SLT sin hash | Alta | O(filas) | Alta | Media: el fallo no da hash estable | Media | Rechazada: no hay hash reproducible entre runners |

### 3.2 Gate de regresión de benchmarks

| # | Hipótesis | Accuracy | Latency | Scalability | Robustness | Reproducibility | Veredicto |
|---|---|---|---|---|---|---|---|
| H1 | Umbral sobre wall-clock de criterion en CI compartida | Media | Alta | Alta | Baja: flaky | Baja | Rechazada |
| H2 | JSON de métricas + umbrales relativos QPS/p95 (elegida) | Alta sobre el reporte | Baja | O(benchmarks) | Alta si valida entradas | Alta | **Elegida** |
| H3 | Test estadístico (t-test / z-score) con histórico | Alta | Muy alta (servicio) | Alta | Alta | Alta | Diferida: requiere backend de histórico |
| H4 | Conteo de instrucciones (iai/valgrind) | Muy alta | Media | Media | Alta | Muy alta | Futura: instrumentar hot paths |

### 3.3 Complejidad de pertenencia

`Vec::contains` en bucle → `O(N²)`; `BTreeSet` → `O(N log N)`;
`HashSet` → `O(N)` esperado. La elección por hot path se mantiene.

## 4. Gap analysis

| Gap detectado | Evidencia | Impacto | Acción |
|---|---|---|---|
| Fixture sin Float/Text/Bool/NULL ni caso de error | `basic.json` solo `Int`; el TDD de SPEC-0196 los exigía | El hash no probaba normalización de tipos | AC-0196-05 + fixture tipado |
| `2.0` diverge entre lenguajes | Rust `2.0`, Python `2.0`, JS `2` | Mismatch espurio al añadir floats | `canonical_number` JCS en los tres runners |
| `bench-gate` ignoraba benchmarks eliminados | loop solo sobre `head` | Falso verde por cobertura reducida | AC-0198-05 |
| `bench-gate` aceptaba `qps` negativo/duplicados/vacío | `metrics()` sin validación | Reportes corruptos pasaban | AC-0198-05 |
| AC-0194-04 solo comprobaba equivalencia | sin medir escalado | Claim O(N²)→O(N log N) sin evidencia | Gate N/2N/4N por comparaciones |
| SPEC-0198 NF-02 exigía criterion | no hay benches criterion nuevos | Requisito incumplido | Diff de NF con rationale (wall-clock no es gate) |

## 5. Decisión (Δquality > 0 ∧ Δcost < 0)

- **AC-0196-05**: fixture tipado + normalización JCS + caso de error. Coste:
  ~40 líneas y dos seeds; calidad: cubre 4 tipos y un error.
- **AC-0198-05**: validación de cobertura e integridad del reporte. Coste:
  ~20 líneas; evita falsos verdes.
- **AC-0194-04**: gate determinista por comparaciones instrumentadas. Coste:
  ~40 líneas de test; evidencia medible sin reloj.
- **NF-0198-02**: se sustituye criterion por tests deterministas + JSON. Coste
  menor (no compila benches extra); calidad igual o mayor en reproducibilidad.

## 6. No aplicado (y por qué)

- **JCS completo** (exponentes, `-0`, precisión ECMAScript): requiere
  dependencia y el fixture no lo necesita; documentado como límite.
- **Test estadístico de benchmarks** (t-test/z-score): necesita histórico
  persistente; fuera del alcance embebido.
- **Datasets ANN-Benchmarks/LDBC/TPC-H**: descarga y tamaño incompatibles con
  CI; fixtures deterministas siguen siendo la fuente de verdad.
- **`iai`/conteo de instrucciones**: requiere valgrind en CI; candidato futuro.
- **Endurecimiento del endpoint HTTP** (405, límite de concurrencia,
  registry vivo): **aplicado** en este ciclo (AC-0197-04/05); no estaba en la
  auditoría inicial y se añadió por robustez operativa.
