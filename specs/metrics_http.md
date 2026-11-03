---
id: SPEC-0197
feature: metrics_http
status: implemented
maturity: real
owner: platform-team
appetite_days: 2
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/observability.rs
    - crates/ruscadb/Cargo.toml
    - .github/workflows/ci.yml
  out_of_scope: [TLS, autenticación, pushgateway, OTLP HTTP real]
fr:
  - { id: FR-0197-01, desc: "feature opcional metrics-http con TcpListener std" }
  - { id: FR-0197-02, desc: "GET /metrics responde 200 con texto Prometheus 0.0.4" }
  - { id: FR-0197-03, desc: "otras rutas responden 404; el servidor se detiene por shutdown" }
nf:
  - { id: NF-0197-01, desc: "cero dependencias nuevas; std::net" }
  - { id: NF-0197-02, desc: "O(metricas) por scrape; sin bloquear el motor" }
acceptance_criteria:
  - id: AC-0197-01
    given: un MetricsRegistry con counters
    when: se hace GET /metrics al servidor
    then: responde 200, Content-Type text/plain; version=0.0.4 y el cuerpo es export()
    test: test_ac_0197_01_metrics_http_ok
  - id: AC-0197-02
    given: una ruta distinta de /metrics
    when: se hace GET /health
    then: responde 404 sin exponer métricas
    test: test_ac_0197_02_metrics_404
  - id: AC-0197-03
    given: el servidor en 127.0.0.1:0
    when: se solicita shutdown
    then: el accept loop termina y el puerto se libera
    test: test_ac_0197_03_metrics_shutdown
  - id: AC-0197-04
    given: una petición con método distinto de GET/HEAD
    when: se envía POST /metrics
    then: responde 405 con Allow y sin cuerpo de métricas
    test: test_ac_0197_04_metrics_method_not_allowed
  - id: AC-0197-05
    given: un registry compartido enlazado con bind_shared
    when: se incrementan contadores después del bind y se hace scrape
    then: el scrape refleja los valores actuales (registry vivo)
    test: test_ac_0197_05_metrics_live_registry
exit_criteria:
  - cargo test -p ruscadb --features metrics-http
  - cargo clippy -p ruscadb --all-targets --features metrics-http -- -D warnings
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - desactivar la feature metrics-http; el export de texto sigue disponible
sandbox:
  - cargo test -p ruscadb --features metrics-http observability::tests
---

# SPEC-0197 — Endpoint HTTP de scrape Prometheus

## Contexto

`MetricsRegistry::export()` ya produce el formato de texto Prometheus
(`# HELP`/`# TYPE`/muestras), pero no hay transporte: un operador no puede
hacer `GET /metrics`. El gap register lo declara “adapter pendiente”. La
solución debe ser opcional para no meter red en el core embebido.

## Diseño

1. Feature `metrics-http` (OFF por defecto).
2. `MetricsServer::bind(addr, registry)` con `std::net::TcpListener`.
3. `serve_until(shutdown: Arc<AtomicBool>)`: acepta conexiones, lee la primera
   línea, responde:
   - `GET /metrics` → `200 OK`, `Content-Type: text/plain; version=0.0.4`,
     cuerpo `registry.export()`.
   - otra ruta → `404 Not Found`.
4. Timeout de lectura (p. ej. 2 s) para no bloquear el accept loop.
5. Sin TLS/auth; fuera de alcance.

Invariante: el endpoint es de solo lectura y no muta el registry.

## Complejidad

```text
scrape: O(M) para M métricas; memoria O(M) por respuesta
accept: O(1) por conexión
```

## TDD adversarial

- Test 200 + content-type + cuerpo exacto.
- Test 404 sin cuerpo de métricas.
- Test shutdown con puerto `127.0.0.1:0`.
- Test de dos scrapes concurrentes (sin corrupción).

## Riesgos

- No exponer en `0.0.0.0` por defecto; bind explícito.
- Sin auth; se documenta que es un adapter local/operador.

## Estado de implementación (2026-11-03)

- Feature `metrics-http` (OFF por defecto) y `MetricsServer` con `std::net`.
- `GET /metrics` → 200 `text/plain; version=0.0.4` con `registry.export()`;
  otras rutas → 404; métodos distintos de GET/HEAD → 405 con `Allow: GET, HEAD`;
  shutdown con `AtomicBool`.
- `bind_shared` acepta un `Arc<RwLock<MetricsRegistry>>` vivo: los scrapes
  reflejan incrementos posteriores al bind.
- Concurrencia acotada a `MAX_IN_FLIGHT = 8` handlers; el exceso recibe
  `503 Service Unavailable` sin crear hilos (anti-slowloris).
- Tests activos: AC-0197-01..05 en `observability.rs`.
