---
id: SPEC-0068
feature: mcp_server
status: implemented
owner: binding-team
appetite_days: 5
boundaries:
  crates: [ruscadb-mcp]
  out_of_scope: [transporte HTTP/WebSocket, autenticacion, listado de recursos MCP, tools de escritura]
fr:
  - { id: FR-0068-01, desc: "crate ruscadb-mcp: servidor MCP sobre transporte stdio (JSON-RPC 2.0 por stdin/stdout) sin red" }
  - { id: FR-0068-02, desc: "tools minimos: query(sql), tables(), integrity_check() — el resto queda para iteracion posterior; response tipada" }
nf:
  - { id: NF-0068-01, desc: "sin dependencias de red (ni tokio/hyper): solo serde_json; compila sin features extra" }
  - { id: NF-0068-02, desc: "errores JSON-RPC correctos (-32601 metodo, -32602 params, -32000 ejecucion) con mensajes accionables; handle_message nunca panic" }
acceptance_criteria:
  - id: AC-0068-01
    given: un mensaje JSON-RPC `tools/list`
    when: se pasa a handle_message
    then: respuesta JSON con id, result y la lista de tools (query/tables/integrity_check)
    test: test_ac_0068_01_tools_list_returns_tools
  - id: AC-0068-02
    given: un mensaje `tools/call` con name "query" y arguments { sql: "SELECT ..." } sobre una base sembrada
    when: se pasa a handle_message
    then: resultado JSON con las filas tipadas correctas
    test: test_ac_0068_02_tools_call_query_returns_rows
  - id: AC-0068-03
    given: un metodo desconocido y un call con arguments invalidos
    when: se pasan a handle_message
    then: errores JSON-RPC con code -32601 / -32602 y mensaje accionable, sin panics
    test: test_ac_0068_03_unknown_method_and_bad_params_error
  - id: AC-0068-04
    given: una base con datos y un mensaje `tools/call` name "integrity_check"
    when: se pasa a handle_message
    then: reporte JSON { wal_ok, heap_ok, vector_ok } en result
    test: test_ac_0068_04_integrity_check_tool_reports
  - id: AC-0068-05
    given: el Cargo.toml del crate ruscadb-mcp y el arbol de dependencias
    when: se inspecciona y compila el crate
    then: sin tokio/hyper/async de red; build y tests del crate en verde
    test: test_ac_0068_05_crate_is_stdio_only_and_builds
exit_criteria:
  - cargo test -p ruscadb-mcp -- test_ac_0068
  - cargo test -p ruscadb-mcp  (suite del crate)
  - cargo clippy -p ruscadb-mcp --all-targets -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - eliminar el crate ruscadb-mcp del workspace (Cargo.toml members y carpeta)
sandbox:
  - cargo test -p ruscadb-mcp
---

# SPEC-0068 — Servidor MCP (Model Context Protocol, transporte stdio)

## Contexto

Línea futura del roadmap: **MCP server**. RuscaDB es "sin servidor y sin red"
por identidad; el transporte **stdio** de MCP es local (un proceso padre lanza
el binario y habla JSON-RPC 2.0 por pipes) — encaja sin violar la identidad, y
abre la puerta a usar RuscaDB como herramienta de datos local para agentes de
IA (Claude/Cursor/Codex). Este MVP expone tools de **lectura e inspección**
(query, tables, integrity_check); escritura y recursos quedan para iteración.

## Superficie de API

```rust
// crates/ruscadb-mcp (nuevo miembro del workspace)
pub struct McpServer { db: ruscadb::Database }
impl McpServer {
    pub fn new(db: ruscadb::Database) -> Self;
    /// JSON-RPC 2.0 request -> respuesta JSON (nunca panic).
    pub fn handle_message(&mut self, json: &str) -> String;
}
```

Contrato JSON-RPC: `{"jsonrpc":"2.0","id":N,"method":"tools/list"}` →
`{"jsonrpc":"2.0","id":N,"result":{"tools":[...]}}`;
`tools/call` con `params:{"name","arguments":{...}}`. Errores: `-32601` (método),
`-32602` (params), `-32000` (error de ejecución, detalle en message).

## Criterios de aceptación

- **AC-0068-01/02/04** — tools list/query/integrity_check con respuesta tipada.
- **AC-0068-03** — errores JSON-RPC correctos sin panics.
- **AC-0068-05** — crate autónomo, solo stdio + serde_json, compila y testea.

## Trazabilidad

Tests `test_ac_0068_<nn>_*`; verificado por `cargo xtask trace`/`contract`.