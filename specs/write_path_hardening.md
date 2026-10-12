---
id: SPEC-0070
feature: write_path_hardening
status: implemented
owner: core-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-mcp]
  out_of_scope: [usuarios multiples, persistencia del audit, RBAC por tabla]
fr:
  - { id: FR-0070-01, desc: "todo camino de escritura exige RBAC y aplica gates de ingesta: insert_many (camino real del SQL INSERT) y Database::delete incluidos" }
  - { id: FR-0070-02, desc: "la deteccion de DML en execute usa el parser (parse_statement), no heuristicas de prefijo" }
  - { id: FR-0070-03, desc: "el servidor MCP fuerza rol Read en su Database: tools/call query con DML devuelve error accionable" }
  - { id: FR-0070-04, desc: "audit con reloj monótono: nanos estrictamente crecientes; las notificaciones JSON-RPC (sin id) no generan respuesta" }
nf:
  - { id: NF-0070-01, desc: "cero regresion: Admin por defecto sigue pudiendo todo; SPEC-0066 sigue verde" }
  - { id: NF-0070-02, desc: "sin panics; errores accionables; sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0070-01
    given: rol Read y una tabla existente
    when: se llama insert_many y (en otro caso) se inserta un record que supera max_record_bytes con Admin
    then: insert_many deniega con PermissionDenied y el gate de tamano rechaza en el camino de insert_many
    test: test_ac_0070_01_insert_many_respects_rbac_and_gates
  - id: AC-0070-02
    given: rol Read con una fila existente
    when: se llama Database::delete(table, id)
    then: PermissionDenied y la fila sigue existiendo
    test: test_ac_0070_02_delete_requires_write_role
  - id: AC-0070-03
    given: un McpServer sobre una base Admin con una tabla
    when: se envia tools/call query con "INSERT INTO ..." 
    then: error JSON-RPC (-32000) accionable de permiso y la tabla queda intacta
    test: test_ac_0070_03_mcp_query_forces_read_only
  - id: AC-0070-04
    given: audit habilitado
    when: se emiten tres operaciones consecutivas
    then: los nanos son estrictamente crecientes (reloj monótono)
    test: test_ac_0070_04_audit_nanos_strictly_increase
  - id: AC-0070-05
    given: un McpServer
    when: se envia una notificacion JSON-RPC (sin campo id)
    then: la respuesta es cadena vacia (JSON-RPC prohibe responder a notificaciones)
    test: test_ac_0070_05_notification_gets_no_response
exit_criteria:
  - cargo test -p ruscadb --test write_path_hardening -- test_ac_0070
  - cargo test -p ruscadb-mcp  (SPEC-0068 sigue verde)
  - cargo clippy -p ruscadb -p ruscadb-mcp --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - revertir los checks anadidos en insert_many/delete/execute y el forzado Read del MCP
sandbox:
  - cargo test -p ruscadb --test write_path_hardening
---

# SPEC-0070 — Endurecimiento del camino de escritura (retrospectiva 3)

## Contexto

**Hallazgos P1/P2 de la retrospectiva 3**: `insert_many` (el camino real del
`INSERT` SQL, `dml.rs`) evade RBAC (SPEC-0066), gates de ingesta (SPEC-0064) y
allowlist (SPEC-0032); `Database::delete` es público sin RBAC; la detección de
DML es una heurística de prefijo frágil; el servidor MCP ejecuta `query` con el
rol heredado (por defecto Admin) pese a anunciar solo lectura; y el audit usa
reloj de pared (puede retroceder).

## Cambios

- `insert_many`/`insert_many_at`: `require_write("insert")` + gates por record.
- `delete`: `require_write("delete")`.
- `execute`: clasificar DML con `parse_statement` (variante) en vez de prefijos.
- `ruscadb-mcp`: `McpServer::new` fuerza `set_role(Role::Read)`; notificación
  (sin `id`) → respuesta vacía.
- Audit: nanos estrictamente crecientes (base `Instant` + contador).

## Criterios de aceptación

- **AC-0070-01/02** — caminos de escritura con RBAC y gates.
- **AC-0070-03/05** — MCP solo lectura y sin respuestas a notificaciones.
- **AC-0070-04** — audit monótono estricto.

Nota: la clasificación de DML por parser (FR-0070-02) es defensa en
profundidad sin AC propio (la heurística actual ya cubre espacios/minúsculas).

## Trazabilidad

Tests `test_ac_0070_<nn>_*`; verificado por `cargo xtask trace`/`contract`.