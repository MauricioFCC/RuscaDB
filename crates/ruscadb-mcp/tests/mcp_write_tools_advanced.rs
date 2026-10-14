//! Tests de aceptación de las tools MCP de escritura avanzada (SPEC-0091).
//!
//! Cubre AC-0091-02..04: roundtrip `update`/`delete` en modo `read_write`,
//! rechazo accionable en solo lectura + validación de `RecordId`, y
//! propagación del RBAC de la `Database` (rol `Read` sin mutación).

#![allow(clippy::expect_used)]

use std::path::PathBuf;

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Role, ScalarMap, ScalarValue};
use ruscadb_mcp::McpServer;
use serde_json::Value;

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Código JSON-RPC: error de ejecución del servidor.
const ERR_SERVER: i64 = -32000;

/// Código JSON-RPC: parámetros inválidos.
const ERR_INVALID_PARAMS: i64 = -32602;

/// Borra los ficheros de una base temporal al finalizar el test.
struct TempDb {
    /// Ruta del fichero de datos `.db` de la base.
    path: PathBuf,
}

impl TempDb {
    /// Registra la ruta base a limpiar.
    ///
    /// Args:
    ///     path: Ruta del fichero de datos de la base temporal.
    ///
    /// Returns:
    ///     Guarda de limpieza lista para mantener viva durante el test.
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("wal"));
    }
}

/// Compone una ruta de base única en el directorio temporal del sistema.
///
/// Args:
///     tag: Sufijo que aísla el fichero por test.
///
/// Returns:
///     La ruta `.db` única (proceso + instante).
fn unique_db_path(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("reloj del sistema")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ruscadb_mcp_adv_write_{tag}_{}_{nanos}.db",
        std::process::id()
    ))
}

/// Abre una base temporal de pruebas con pool amplio.
///
/// Args:
///     tag: Sufijo del nombre de fichero (aísla cada test).
///
/// Returns:
///     La guarda temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (TempDb, Database) {
    let path = unique_db_path(tag);
    let database = Database::open(DbConfig::new(&path, POOL_CAPACITY))
        .expect("apertura de la base de pruebas");
    (TempDb::new(path), database)
}

/// Crea `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base destino.
fn create_items(database: &mut Database) {
    database
        .create_table(
            "items",
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Construye escalares `(id, name)`.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     name: Valor textual de la columna `name`.
///
/// Returns:
///     El mapa de escalares listo para un registro.
fn row_scalars(id: i64, name: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ])
}

/// Envía un mensaje JSON-RPC y parsea la respuesta como JSON.
///
/// Args:
///     server: Servidor MCP destino.
///     message: Texto JSON-RPC 2.0 de entrada.
///
/// Returns:
///     La respuesta parseada como `serde_json::Value`.
fn call(server: &mut McpServer, message: &str) -> Value {
    let raw = server.handle_message(message);
    serde_json::from_str(&raw).expect("la respuesta debe ser JSON válido")
}

/// Autentica la sesión del servidor con un token recién emitido (SPEC-0088).
///
/// Args:
///     server: Servidor MCP recién construido (sin sesión).
fn authenticate(server: &mut McpServer) {
    let token = server.issue_session_token().expect("token de sesión");
    let message = format!(
        r#"{{"jsonrpc":"2.0","id":0,"method":"session/auth","params":{{"token":"{token}"}}}}"#
    );
    let response = call(server, &message);
    assert_eq!(
        response["result"]["authenticated"].as_bool(),
        Some(true),
        "session/auth debe autenticar con el token emitido"
    );
}

/// Cuenta las filas visibles de `items` vía la tool `query`.
///
/// Args:
///     server: Servidor MCP autenticado.
///
/// Returns:
///     El número de filas devueltas por `SELECT * FROM items`.
fn count_rows(server: &mut McpServer) -> usize {
    let select = r#"{"jsonrpc":"2.0","id":90,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT * FROM items"}}}"#;
    let response = call(server, select);
    response["result"]["rows"]
        .as_array()
        .expect("result.rows debe ser un array")
        .len()
}

/// AC-0091-02 — roundtrip MCP `update` → SELECT → `delete` → SELECT.
#[test] // @spec AC-0091-02
fn test_ac_0091_02_mcp_update_and_delete_roundtrip() {
    let (_guard, mut database) = open_test_db("ac0091_02");
    create_items(&mut database);
    let id = database
        .insert("items", row_scalars(1, "uno"))
        .expect("insert de la fila sembrada");
    let mut server = McpServer::read_write(database).expect("servidor MCP con escritura");
    authenticate(&mut server);

    let update = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"update","arguments":{{"table":"items","id":"{id}","values":{{"name":"dos"}}}}}}}}"#
    );
    let updated = call(&mut server, &update);
    assert_eq!(
        updated["result"]["updated"].as_bool(),
        Some(true),
        "update debe confirmar la fila actualizada: {updated}"
    );

    let select = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT * FROM items"}}}"#;
    let after = call(&mut server, select);
    assert_eq!(
        after["result"]["rows"][0]["name"].as_str(),
        Some("dos"),
        "el SELECT debe ver el valor nuevo"
    );

    let delete = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"delete","arguments":{{"table":"items","id":"{id}"}}}}}}"#
    );
    let deleted = call(&mut server, &delete);
    assert_eq!(
        deleted["result"]["deleted"].as_bool(),
        Some(true),
        "delete debe confirmar la fila borrada: {deleted}"
    );
    assert_eq!(
        count_rows(&mut server),
        0,
        "el SELECT posterior al delete no debe ver filas"
    );
}

/// AC-0091-03 — solo lectura rechaza (-32000) e id inválido responde -32602.
#[test] // @spec AC-0091-03
fn test_ac_0091_03_read_only_rejects_and_validates_id() {
    // (a) Solo lectura: tools/list no publica update/delete y la llamada falla.
    let (_guard, mut database) = open_test_db("ac0091_03_ro");
    create_items(&mut database);
    let id = database
        .insert("items", row_scalars(1, "uno"))
        .expect("insert de la fila sembrada");
    let mut server = McpServer::new(database).expect("servidor MCP");
    authenticate(&mut server);

    let listed = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":10,"method":"tools/list"}"#,
    );
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("result.tools debe ser un array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(
        !names.contains(&"update") && !names.contains(&"delete"),
        "solo lectura no debe publicar las tools de escritura: {names:?}"
    );

    let update = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{{"name":"update","arguments":{{"table":"items","id":"{id}","values":{{"name":"dos"}}}}}}}}"#
    );
    let denied = call(&mut server, &update);
    assert_eq!(
        denied["error"]["code"].as_i64(),
        Some(ERR_SERVER),
        "una tool de escritura en solo lectura debe responder -32000: {denied}"
    );
    assert_eq!(
        count_rows(&mut server),
        1,
        "el rechazo de solo lectura no debe mutar la fila"
    );

    // (b) RecordId inválido en read_write → -32602.
    let (_guard_rw, mut rw_db) = open_test_db("ac0091_03_bad_id");
    create_items(&mut rw_db);
    let mut rw_server = McpServer::read_write(rw_db).expect("servidor MCP con escritura");
    authenticate(&mut rw_server);
    let bad = r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"update","arguments":{"table":"items","id":"no-es-un-id","values":{"name":"dos"}}}}"#;
    let response = call(&mut rw_server, bad);
    assert_eq!(
        response["error"]["code"].as_i64(),
        Some(ERR_INVALID_PARAMS),
        "un RecordId inválido debe responder -32602: {response}"
    );
}

/// AC-0091-04 — el RBAC de la fachada deniega update/delete con rol `Read`.
#[test] // @spec AC-0091-04
fn test_ac_0091_04_rbac_denies_structured_writes() {
    let (_guard, mut database) = open_test_db("ac0091_04");
    create_items(&mut database);
    let id = database
        .insert("items", row_scalars(1, "original"))
        .expect("insert de la fila sembrada");
    database.set_role(Role::Read).expect("set_role(Read)");
    let mut server = McpServer::read_write(database).expect("servidor MCP con escritura");
    authenticate(&mut server);

    let update = format!(
        r#"{{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{{"name":"update","arguments":{{"table":"items","id":"{id}","values":{{"name":"mutado"}}}}}}}}"#
    );
    let denied_update = call(&mut server, &update);
    assert_eq!(
        denied_update["error"]["code"].as_i64(),
        Some(ERR_SERVER),
        "el RBAC debe denegar el update con -32000: {denied_update}"
    );

    let delete = format!(
        r#"{{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{{"name":"delete","arguments":{{"table":"items","id":"{id}"}}}}}}"#
    );
    let denied_delete = call(&mut server, &delete);
    assert_eq!(
        denied_delete["error"]["code"].as_i64(),
        Some(ERR_SERVER),
        "el RBAC debe denegar el delete con -32000: {denied_delete}"
    );

    let select = r#"{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT * FROM items"}}}"#;
    let after = call(&mut server, select);
    let rows = after["result"]["rows"]
        .as_array()
        .expect("result.rows debe ser un array");
    assert_eq!(rows.len(), 1, "la fila denegada no debe borrarse");
    assert_eq!(
        rows[0]["name"].as_str(),
        Some("original"),
        "la fila denegada no debe mutarse"
    );
}
