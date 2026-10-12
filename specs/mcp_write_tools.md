---
id: SPEC-0075
feature: mcp_write_tools
status: implemented
owner: binding-team
appetite_days: 4
boundaries:
  crates: [ruscadb-mcp]
  out_of_scope: [autenticacion, transporte HTTP, prompts MCP, sampling]
fr:
  - { id: FR-0075-01, desc: "McpServer::read_write habilita escritura opt-in (query DML + tool insert) respetando el RBAC de la Database" }
  - { id: FR-0075-02, desc: "resources MCP: resources/list expone las tablas como ruscadb://table/<name> y resources/read devuelve sus filas JSON" }
nf:
  - { id: NF-0075-01, desc: "cero regresion: McpServer::new sigue siendo solo-lectura; sin dependencias de red" }
  - { id: NF-0075-02, desc: "errores JSON-RPC accionables (-32602/-32000); sin panics" }
acceptance_criteria:
  - id: AC-0075-01
    given: un McpServer::read_write sobre una base con tabla
    when: se envia tools/call query con "INSERT INTO ..."
    then: result con filas afectadas y la fila existe (SELECT posterior la devuelve)
    test: test_ac_0075_01_read_write_allows_dml
  - id: AC-0075-02
    given: un McpServer::new (solo lectura) sobre la misma base
    when: se envia el mismo INSERT
    then: error accionable de solo lectura (sin regresion de SPEC-0070)
    test: test_ac_0075_02_read_only_still_rejects_dml
  - id: AC-0075-03
    given: una base con dos tablas
    when: se envia resources/list
    then: result.resources lista ambas con uri ruscadb://table/<name>
    test: test_ac_0075_03_resources_list_tables
  - id: AC-0075-04
    given: una tabla sembrada
    when: se envia resources/read con su uri
    then: contents[0].text contiene las filas como JSON
    test: test_ac_0075_04_resources_read_rows
  - id: AC-0075-05
    given: un uri desconocido y (en otro caso) read_write sobre una base con rol Read
    when: se envia resources/read del uri y un INSERT
    then: error -32602 para el uri y PermissionDenied propagado como error -32000 para el INSERT
    test: test_ac_0075_05_unknown_uri_and_rbac_propagates
exit_criteria:
  - cargo test -p ruscadb-mcp -- test_ac_0075
  - cargo test -p ruscadb-mcp  (suite del crate, sin regresion)
  - cargo clippy -p ruscadb-mcp --all-targets -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - eliminar read_write y las tools/resources nuevas (volver a solo lectura)
sandbox:
  - cargo test -p ruscadb-mcp
---

# SPEC-0075 — MCP: escritura opt-in + resources

## Contexto

Línea futura «tools MCP de escritura». SPEC-0070 dejó el servidor en solo
lectura por seguridad. La frontera MCP 2026 incluye **resources** (datos
direccionables por URI) además de tools: se exponen las tablas como
`ruscadb://table/<name>`, y la escritura queda como **opt-in explícito**
(`McpServer::read_write`) delegando el permiso real al RBAC de la `Database`
(SPEC-0066).

## Superficie

- `McpServer::new(db)` → solo lectura (sin cambios).
- `McpServer::read_write(db)` → habilita DML en `query` y la tool `insert`.
- Tools: `query`, `tables`, `integrity_check`, `insert` (solo en read_write).
- Métodos: `resources/list`, `resources/read` (uri `ruscadb://table/<name>`).

## Criterios de aceptación

- **AC-0075-01/02** — escritura opt-in y read-only intacto.
- **AC-0075-03/04** — resources de tablas.
- **AC-0075-05** — errores accionables y RBAC propagado.

## Trazabilidad

Tests `test_ac_0075_<nn>_*`; verificado por `cargo xtask trace`/`contract`.