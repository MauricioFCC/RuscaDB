---
id: SPEC-0091
feature: mcp_advanced_write
status: implemented
owner: binding-team
appetite_days: 2
boundaries:
  crates: [ruscadb, ruscadb-mcp]
  files: [crates/ruscadb/src/row_update.rs, crates/ruscadb/src/dml.rs, crates/ruscadb/src/delete.rs, crates/ruscadb/src/lib.rs, crates/ruscadb-mcp/src/lib.rs]
  out_of_scope: [transporte HTTP, autenticacion (SPEC-0088), borrado fisico/GC, bulk update multi-fila por predicado]
fr:
  - { id: FR-0091-01, desc: "Database::update(table, id, scalars) actualiza escalares de una fila viva por RecordId (parcial), valida el esquema y mantiene los indices" }
  - { id: FR-0091-02, desc: "las tools MCP update={table,id,values} y delete={table,id} delegan en la fachada (sin componer SQL) y respetan el RBAC" }
  - { id: FR-0091-03, desc: "tools/list expone update/delete solo con read_write; en solo lectura responden un rechazo accionable (-32000)" }
nf:
  - { id: NF-0091-01, desc: "el adapter no compone SQL a mano (delega en la fachada); sin dependencias nuevas" }
  - { id: NF-0091-02, desc: "errores JSON-RPC accionables (-32602/-32000) e idempotencia: un RecordId inexistente devuelve false sin error" }
acceptance_criteria:
  - id: AC-0091-01
    given: una fila viva en una tabla `items(id Int, name Text)`
    when: se llama Database::update con solo la columna `name`
    then: devuelve Ok(true), el SELECT posterior ve el nuevo `name` y la columna `id` conserva su valor
    test: test_ac_0091_01_update_facade_persists_partial_change
  - id: AC-0091-02
    given: un McpServer::read_write autenticado con una fila sembrada
    when: se envia tools/call update y luego tools/call delete con el RecordId
    then: update responde updated=true y el SELECT ve el valor nuevo; delete responde deleted=true y el SELECT no ve filas
    test: test_ac_0091_02_mcp_update_and_delete_roundtrip
  - id: AC-0091-03
    given: un McpServer::new (solo lectura) y un McpServer::read_write con un RecordId inválido
    when: se envia tools/list, tools/call update y tools/call update con id inválido
    then: tools/list omite update/delete, el update en solo lectura responde -32000 accionable y el id inválido responde -32602
    test: test_ac_0091_03_read_only_rejects_and_validates_id
  - id: AC-0091-04
    given: un McpServer::read_write sobre una base con rol Read
    when: se envia tools/call update y tools/call delete
    then: ambos responden -32000 y la fila sembrada queda intacta (RBAC de la fachada propagado)
    test: test_ac_0091_04_rbac_denies_structured_writes
exit_criteria:
  - cargo test -p ruscadb --test row_update_api
  - cargo test -p ruscadb-mcp --test mcp_write_tools_advanced
  - cargo test -p ruscadb --lib row_update
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace  (y contract)
rollback:
  - retirar las tools update/delete del catalogo MCP, Database::update, persist_row_update y el modulo row_update.rs
sandbox:
  - cargo test -p ruscadb-mcp --test mcp_write_tools_advanced
---

# SPEC-0091 — Tools MCP de escritura avanzada (update/delete) + API estructurada

## Contexto

Línea futura de producto (roadmap §Estado, «Líneas futuras»): las *tools* MCP
de escritura avanzada (update/delete por tool) quedaron pendientes tras
SPEC-0075, que solo expuso `insert`. La frontera MCP 2026 promueve escritura
**estructurada** (argumentos por columna) en lugar de SQL libre en el adapter:
el modelo de amenazas del transporte exige que el cliente no pueda inyectar
identificadores ni sentencias, y que el RBAC/validación viva en la fachada, no
en el adapter.

Esta spec cierra el hueco con dos piezas:

- **Fachada:** `Database::update(table, id, scalars)` — actualización parcial de
  una fila viva por `RecordId`, reutilizando la ruta de persistencia de índices
  del `UPDATE ... WHERE` (SPEC-0043) y el borrado lógico de `Database::delete`
  (SPEC-0022).
- **Adapter MCP:** tools `update`/`delete` (solo `read_write`) que delegan en la
  fachada; `tools/list` las publica únicamente en modo escritura.

## Superficie

- `crates/ruscadb/src/row_update.rs`: `Database::update` (nuevo módulo).
- `crates/ruscadb/src/dml.rs`: `persist_row_update` (`pub(crate)`) — punto único
  de persistencia de una actualización (índice secundario + derivados).
- `crates/ruscadb/src/delete.rs`: `locate_row` pasa a `pub(crate)`.
- `crates/ruscadb-mcp/src/lib.rs`: tools `update`/`delete`, `parse_record_id`,
  rechazo accionable en solo lectura y catálogo ampliado.

## Criterios de aceptación

- **AC-0091-01** — `Database::update` fusiona columnas parciales y persiste.
- **AC-0091-02** — roundtrip MCP update→SELECT→delete→SELECT.
- **AC-0091-03** — solo lectura rechaza con -32000; id inválido con -32602.
- **AC-0091-04** — RBAC: rol `Read` deniega update/delete sin mutar la fila.

## Trazabilidad

Tests `test_ac_0091_<nn>_*` con marcador `@spec AC-0091-NN`; verificado por
`cargo xtask trace`/`contract`.
