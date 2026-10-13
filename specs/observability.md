---
id: SPEC-0080
feature: observability
status: implemented
owner: platform-team
appetite_days: 3
boundaries:
  crates: [ruscadb, ruscadb-storage, ruscadb-wal, ruscadb-txn]
  files: [Cargo.toml (workspace), crates/ruscadb*/Cargo.toml, crates/ruscadb/src/database.rs, crates/ruscadb/src/lock.rs, crates/ruscadb-wal/src/lib.rs, crates/ruscadb-storage/src/buffer_pool.rs, crates/ruscadb/tests/observability.rs]
  out_of_scope: [tracing-subscriber en producción/exporters OTLP, métricas remotas/push, dashboards, spans por página individual del query plan, logs estructurados de error]
fr:
  - { id: FR-0080-01, desc: "spans de tracing en commit, checkpoint, page_fault y lock_wait, con contexto (tabla/lsn/page_id) donde aplique" }
  - { id: FR-0080-02, desc: "feature `metrics` (default OFF) que registra latencia de commit e hit-rate del buffer pool sin coste cuando está desactivada" }
  - { id: FR-0080-03, desc: "cero dependencias nuevas obligatorias en el camino por defecto: `tracing` sin subscriber es no-op (~1.2 ns); `metrics` solo con feature" }
nf:
  - { id: NF-0080-01, desc: "overhead sin subscriber/feature OFF despreciable: no se hacen allocations en el hot path si nadie escucha" }
  - { id: NF-0080-02, desc: "sin unwrap/expect en producción; docstrings ES-UTF8; errores WHAT+WHY+WHERE" }
  - { id: NF-0080-03, desc: "los tests no dependen de un subscriber global instaurado: usan un collector local con `tracing::subscriber::with_default`" }
acceptance_criteria:
  - id: AC-0080-01
    given: un collector de spans instalado localmente
    when: se ejecutan commit, checkpoint, un page fault (lectura no cacheada) y una espera de lock
    then: los 4 spans aparecen con sus nombres exactos (commit/checkpoint/page_fault/lock_wait)
    test: test_ac_0080_01_core_spans_are_emitted
  - id: AC-0080-02
    given: el workspace compilado con `--all-features` (metrics ON)
    when: se ejecuta un commit
    then: la métrica de latencia de commit queda registrada y legible desde el test
    test: test_ac_0080_02_metrics_on_records_commit_latency
  - id: AC-0080-03
    given: la compilación por defecto (metrics OFF)
    when: se ejecuta la misma operación
    then: corre limpio sin registrar métricas (test solo compilable con el feature apagado)
    test: test_ac_0080_03_metrics_off_is_clean_default
exit_criteria:
  - cargo test -p ruscadb --test observability
  - cargo test --workspace --all-features   (sin regresión)
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar spans, la feature metrics y las dependencias (vuelve a instrumentación cero)
sandbox:
  - cargo test -p ruscadb --test observability
---

# SPEC-0080 — Observabilidad: spans tracing + metrics (Ola 2)

## Contexto

Auditoría *production readiness* 2026-10-12 (riesgo 5): **sin observabilidad**
— no hay forma de ver commit/checkpoint/page_fault/lock_wait en runtime ni
latencias agregadas. Hoy el repo no tiene ni `tracing` ni `metrics`.

Frontera 2026 (§3, research T4): `tracing` añade ~1.2 ns por span **sin
subscriber** (es no-op), así que es seguro siempre-encendido; `metrics`
mide ~1.2 ns desactivado pero aun así va **detrás de feature OFF por
defecto** para garantizar coste cero en el camino por defecto.

## Superficie

- Workspace `Cargo.toml`: `tracing` en `[workspace.dependencies]`;
  `crates/ruscadb/Cargo.toml` (y los que haga falta) con `metrics`
  opcional tras la feature `metrics` (default OFF).
- Spans: **commit** (`crates/ruscadb/src/database.rs`), **checkpoint**
  (`crates/ruscadb-wal/src/lib.rs`), **page_fault**
  (`crates/ruscadb-storage/src/buffer_pool.rs`), **lock_wait**
  (`crates/ruscadb/src/lock.rs`).
- Test: `crates/ruscadb/tests/observability.rs` con collector local
  (`tracing::subscriber::with_default`) y, para métricas, el registry en
  memoria que exponga la propia feature.

## Criterios de aceptación

- **AC-0080-01** — los 4 spans se emiten con sus nombres exactos.
- **AC-0080-02** — metrics ON registra latencia de commit (corre en
  `--all-features`).
- **AC-0080-03** — metrics OFF es el default limpio (test con cfg negado).

## Trazabilidad

Tests `test_ac_0080_<nn>_*` con marcador `@spec AC-0080-NN`; verificado por
`cargo xtask trace`/`contract`.
