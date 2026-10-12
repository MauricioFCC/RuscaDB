---
id: SPEC-0062
feature: operational_hardening
status: implemented
owner: core-team
appetite_days: 5
boundaries:
  crates: [ruscadb]
  out_of_scope: [RBAC, MCP server, pipeline de blobs, rerank, cifrado (SPEC-0013), reparacion automatica de corrupcion]
fr:
  - { id: FR-0062-01, desc: "integrity_check() verifica WAL (CRC32C por frame), heap (slots coherentes) y cobertura del indice vectorial, devolviendo un reporte sin panic" }
  - { id: FR-0062-02, desc: "shadow reads: con el modo activo cada KNN re-ejecuta la ruta exacta (distancia real sobre los vectores) y compara ids contra la ruta indexada" }
  - { id: FR-0062-03, desc: "las divergencias se acumulan en un reporte consultable (query, ids esperados, ids obtenidos) y NO alteran el resultado servido" }
  - { id: FR-0062-04, desc: "el modo shadow esta OFF por defecto: cero verificaciones y cero coste cuando no se habilita" }
nf:
  - { id: NF-0062-01, desc: "cero regresion: los resultados de las queries son identicos con shadow ON/OFF" }
  - { id: NF-0062-02, desc: "errores accionables WHAT+WHY+WHERE; la deteccion de corrupcion no propaga panic" }
  - { id: NF-0062-03, desc: "docstrings ES en toda API nueva; sin magic numbers; sin unsafe" }
acceptance_criteria:
  - id: AC-0062-01
    given: una base sana con filas vectoriales
    when: se ejecuta integrity_check
    then: reporte con wal_ok/heap_ok/vector_ok en true y sin detalles de error
    test: test_ac_0062_01_integrity_ok_on_healthy_db
  - id: AC-0062-02
    given: una base abierta cuyo WAL en disco tiene un byte de un frame alterado
    when: se ejecuta integrity_check
    then: wal_ok=false con detalle accionable (offset/frame), sin panic
    test: test_ac_0062_02_integrity_detects_corrupt_wal
  - id: AC-0062-03
    given: shadow reads habilitado sobre un indice consistente
    when: se ejecuta una query KNN
    then: verifica al menos una consulta, cero divergencias y resultado identico al modo OFF
    test: test_ac_0062_03_shadow_parity_without_divergence
  - id: AC-0062-04
    given: shadow reads habilitado y un indice vectorial que omite una fila visible
    when: se ejecuta la misma query KNN
    then: la divergencia queda registrada (ids esperados vs obtenidos) y el resultado servido no cambia
    test: test_ac_0062_04_shadow_records_divergence
  - id: AC-0062-05
    given: una base recien abierta
    when: se ejecutan KNN sin habilitar shadow reads
    then: el reporte muestra cero verificaciones (coste nulo por defecto)
    test: test_ac_0062_05_shadow_off_by_default
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0062
  - cargo test -p ruscadb-vector -p ruscadb  (suite previa verde)
  - mutation score acotado a los ficheros tocados >= 70%
rollback:
  - eliminar el modulo de hardening y sus campos en Database; el executor KNN
    queda exactamente como estaba (el camino shadow es un bloque adicional)
sandbox:
  - cargo test -p ruscadb --test operational_hardening
---

# SPEC-0062 — Hardening operativo + shadow reads

## Contexto

**PRIMERA LINEA FUTURA del roadmap ("hardening operativo + shadow reads").**
Una base embebida no tiene un DBA que vigile: necesita autoverificacion
(`integrity_check`) y verificacion continua de sus indices en produccion
(shadow reads), el patron de "shadow traffic" adaptado a un motor embebido:
la lectura se sirve por el camino rapido (indice) y se re-ejecuta por el
camino exacto (brute-force con `ruscadb_vector::distance`), comparando ids sin
alterar el resultado. La divergencia no se oculta ni se lanza: se registra.

## Superficie de API

- `Database::integrity_check() -> Result<IntegrityReport, RuscaError>`
  (`wal_ok`, `heap_ok`, `vector_ok`, `details`).
- `Database::set_shadow_reads(bool)` / `shadow_reads_enabled()`.
- `Database::shadow_report() -> ShadowReport` (`checks`, `divergences`).
- `ShadowDivergence { query, expected, got }` con `RecordId`s.

## Criterios de aceptación

- **AC-0062-01/02** — integridad sana y deteccion de WAL corrupto (CRC32C).
- **AC-0062-03/04/05** — paridad shadow, registro de divergencia y OFF por
  defecto.

## Trazabilidad

Tests `test_ac_0062_<nn>_*`; verificado por `cargo xtask trace` y
`cargo xtask contract`.
