//! Tests de aceptación del servidor MCP (SPEC-0075): escritura opt-in + resources.
//!
//! RED por símbolo: `McpServer::read_write` aún no existe en el crate
//! `ruscadb-mcp`, por lo que la compilación de esta suite falla con E0599
//! (`no function or associated item named read_write`). Los tests fijan el
//! contrato de escritura opt-in (FR-0075-01) y de los resources MCP
//! (`resources/list`, `resources/read`, uri `ruscadb://table/<name>`), además
//! del error accionable de solo lectura (sin regresión de SPEC-0070) y la
//! propagación del RBAC de la `Database` (SPEC-0066).

// El oráculo puede fallar ruidosamente: el `allow-expect` del workspace no
// alcanza a los targets de integración del crate.
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

/// Esquema de URI de los resources de tabla expuestos por el servidor MCP.
const TABLE_URI_SCHEME: &str = "ruscadb://table/";

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
        "ruscadb_mcp_write_res_{tag}_{}_{nanos}.db",
        std::process::id()
    ))
}

/// Abre una base temporal de pruebas con pool amplio (rol `Admin` por defecto).
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
///     table: Nombre de la tabla a crear.
fn create_items(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
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

/// Extrae el mensaje textual de error de una respuesta JSON-RPC fallida.
///
/// Args:
///     response: Respuesta JSON-RPC parseada.
///
/// Returns:
///     El mensaje de error en minúsculas (el test decide cómo inspeccionarlo).
fn error_message(response: &Value) -> String {
    response["error"]["message"]
        .as_str()
        .expect("un error JSON-RPC debe traer message textual")
        .to_lowercase()
}

/// AC-0075-01 — `read_write` habilita el DML por `tools/call query`.
///
/// La base es `Admin` por defecto: con `McpServer::read_write` un `INSERT` por
/// `query` no debe responder error, y un `SELECT` posterior debe ver la fila.
#[test]
// @spec AC-0075-01
fn test_ac_0075_01_read_write_allows_dml() {
    let (_guard, mut database) = open_test_db("ac0075_01");
    create_items(&mut database, "items");
    let mut server = McpServer::read_write(database);

    let insert = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"query","arguments":{"sql":"INSERT INTO items (id, name) VALUES (1, 'uno')"}}}"#;
    let response = call(&mut server, insert);

    assert!(
        response.get("error").is_none(),
        "read_write debe permitir el DML: {response}"
    );
    assert!(
        response.get("result").is_some(),
        "un INSERT aceptado debe responder con result, no con error"
    );

    let select = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT * FROM items"}}}"#;
    let after = call(&mut server, select);
    let rows = after["result"]["rows"]
        .as_array()
        .expect("result.rows debe ser un array");
    assert_eq!(
        rows.len(),
        1,
        "el INSERT de read_write debe persistir 1 fila"
    );
    assert_eq!(
        rows[0]["name"].as_str(),
        Some("uno"),
        "la fila insertada debe leerse de vuelta"
    );
}

/// AC-0075-02 — `McpServer::new` sigue rechazando el DML (sin regresión).
#[test]
// @spec AC-0075-02
fn test_ac_0075_02_read_only_still_rejects_dml() {
    let (_guard, mut database) = open_test_db("ac0075_02");
    create_items(&mut database, "items");
    database
        .insert("items", row_scalars(1, "original"))
        .expect("insert de la fila sembrada");
    let mut server = McpServer::new(database);

    let insert = r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"query","arguments":{"sql":"INSERT INTO items (id, name) VALUES (2, 'dos')"}}}"#;
    let response = call(&mut server, insert);

    assert!(
        response.get("result").is_none(),
        "el servidor solo-lectura no debe responder result al DML"
    );
    let code = response["error"]["code"]
        .as_i64()
        .expect("un DML en solo-lectura debe responder un error JSON-RPC");
    assert!(
        code == ERR_SERVER || code == ERR_INVALID_PARAMS,
        "el rechazo de solo lectura debe usar -32000 o -32602, se obtuvo {code}"
    );
    assert!(
        !error_message(&response).trim().is_empty(),
        "el mensaje de rechazo debe ser accionable (no vacío)"
    );

    let select = r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT * FROM items"}}}"#;
    let after = call(&mut server, select);
    let rows = after["result"]["rows"]
        .as_array()
        .expect("result.rows debe ser un array");
    assert_eq!(
        rows.len(),
        1,
        "el DML rechazado no debe crear filas: {} filas",
        rows.len()
    );
    assert_eq!(
        rows[0]["name"].as_str(),
        Some("original"),
        "la fila sembrada debe quedar intacta"
    );
}

/// AC-0075-03 — `resources/list` expone cada tabla como `ruscadb://table/<name>`.
#[test]
// @spec AC-0075-03
fn test_ac_0075_03_resources_list_tables() {
    let (_guard, mut database) = open_test_db("ac0075_03");
    create_items(&mut database, "items");
    create_items(&mut database, "otros");
    let mut server = McpServer::read_write(database);

    let response = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":21,"method":"resources/list"}"#,
    );

    let resources = response["result"]["resources"]
        .as_array()
        .expect("result.resources debe ser un array");
    assert_eq!(resources.len(), 2, "se esperan dos tablas como resources");

    let mut uris: Vec<&str> = resources
        .iter()
        .map(|resource| {
            resource["uri"]
                .as_str()
                .expect("cada resource debe exponer un uri textual")
        })
        .collect();
    uris.sort_unstable();
    let expected = [
        format!("{TABLE_URI_SCHEME}items"),
        format!("{TABLE_URI_SCHEME}otros"),
    ];
    assert_eq!(
        uris,
        expected.iter().map(String::as_str).collect::<Vec<&str>>(),
        "los uri deben seguir el esquema ruscadb://table/<name>"
    );

    let mut names: Vec<&str> = resources
        .iter()
        .map(|resource| {
            resource["name"]
                .as_str()
                .expect("cada resource debe exponer un name textual")
        })
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["items", "otros"],
        "el name del resource debe ser el nombre de la tabla"
    );
}

/// AC-0075-04 — `resources/read` devuelve las filas como JSON en `contents[0].text`.
#[test]
// @spec AC-0075-04
fn test_ac_0075_04_resources_read_rows() {
    let (_guard, mut database) = open_test_db("ac0075_04");
    create_items(&mut database, "items");
    database
        .insert("items", row_scalars(1, "uno"))
        .expect("insert de la fila sembrada");
    let mut server = McpServer::read_write(database);

    let message = r#"{"jsonrpc":"2.0","id":31,"method":"resources/read","params":{"uri":"ruscadb://table/items"}}"#;
    let response = call(&mut server, message);

    let contents = response["result"]["contents"]
        .as_array()
        .expect("result.contents debe ser un array");
    assert_eq!(contents.len(), 1, "se espera un único bloque de contenido");
    let text = contents[0]["text"]
        .as_str()
        .expect("contents[0].text debe ser el JSON textual de las filas");

    let rows: Value = serde_json::from_str(text).expect("contents[0].text debe ser JSON válido");
    let rows = rows
        .as_array()
        .expect("el texto debe ser un array de filas");
    assert_eq!(rows.len(), 1, "se sembró una sola fila");
    assert_eq!(
        rows[0]["id"].as_i64(),
        Some(1),
        "el id de la fila debe viajar como número JSON"
    );
    assert_eq!(
        rows[0]["name"].as_str(),
        Some("uno"),
        "el name de la fila debe viajar como string JSON"
    );
}

/// AC-0075-05 — uri desconocido (-32602) y RBAC de la Database propagado (-32000).
///
/// (a) `resources/read` de una tabla inexistente responde -32602 con mensaje no
/// vacío. (b) `read_write` NO salta el RBAC: con rol `Read` el INSERT se
/// deniega con -32000 y el mensaje menciona el permiso.
#[test]
// @spec AC-0075-05
fn test_ac_0075_05_unknown_uri_and_rbac_propagates() {
    // (a) uri desconocido → -32602 accionable.
    let (_guard_uri, mut uri_db) = open_test_db("ac0075_05_uri");
    create_items(&mut uri_db, "items");
    let mut uri_server = McpServer::read_write(uri_db);

    let unknown = r#"{"jsonrpc":"2.0","id":41,"method":"resources/read","params":{"uri":"ruscadb://table/nope"}}"#;
    let response = call(&mut uri_server, unknown);
    assert_eq!(
        response["error"]["code"].as_i64(),
        Some(ERR_INVALID_PARAMS),
        "un uri de tabla desconocida debe responder -32602"
    );
    assert!(
        !error_message(&response).trim().is_empty(),
        "el error de uri desconocido debe ser accionable (no vacío)"
    );

    // (b) RBAC propagado: read_write sobre rol Read sigue denegando el DML.
    let (_guard_rbac, mut rbac_db) = open_test_db("ac0075_05_rbac");
    create_items(&mut rbac_db, "items");
    rbac_db.set_role(Role::Read).expect("set_role(Read)");
    let mut rbac_server = McpServer::read_write(rbac_db);

    let insert = r#"{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"query","arguments":{"sql":"INSERT INTO items (id, name) VALUES (1, 'uno')"}}}"#;
    let denied = call(&mut rbac_server, insert);
    assert_eq!(
        denied["error"]["code"].as_i64(),
        Some(ERR_SERVER),
        "el RBAC de la Database debe propagarse como -32000"
    );
    let message = error_message(&denied);
    assert!(
        message.contains("permiso") || message.contains("deneg") || message.contains("rol"),
        "el mensaje debe mencionar el permiso denegado por el RBAC: {message}"
    );
}
