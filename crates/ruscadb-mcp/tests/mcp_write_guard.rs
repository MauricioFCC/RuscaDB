//! Tests de aceptación del guardado de escritura del servidor MCP (SPEC-0070).
//!
//! Cubre AC-0070-03/05: el `McpServer` fuerza rol `Read` en su `Database`, por
//! lo que `tools/call query` con DML devuelve un error JSON-RPC accionable de
//! permiso y la tabla queda intacta; y las notificaciones JSON-RPC (sin campo
//! `id`) NO reciben respuesta.
//!
//! Estos tests son **RED por aserción** (no por símbolos): `handle_message` ya
//! existe y la suite compila, pero hoy el servidor ejecuta el DML con el rol
//! heredado (Admin) y responde a las notificaciones con `id: null`.

// El oráculo puede fallar ruidosamente: el `allow-expect` del workspace no
// alcanza a los targets de integración del crate.
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};
use ruscadb_mcp::McpServer;
use serde_json::Value;

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Código JSON-RPC de error de ejecución del servidor.
const ERR_SERVER: i64 = -32000;

/// Código JSON-RPC de parámetros inválidos.
const ERR_INVALID_PARAMS: i64 = -32602;

/// Número de filas sembradas antes de enviar el DML por MCP.
const SEEDED_ROWS: usize = 1;

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
        "ruscadb_mcp_write_{tag}_{}_{nanos}.db",
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

/// AC-0070-03 — `query` con DML se rechaza y la tabla queda intacta.
///
/// El `McpServer` debe forzar rol `Read`: aunque la base subyacente sea `Admin`,
/// un `INSERT` por `tools/call query` responde un error de permiso y un `SELECT`
/// posterior sigue viendo solo la fila sembrada.
#[test]
// @spec AC-0070-03
fn test_ac_0070_03_mcp_query_forces_read_only() {
    let (_guard, mut database) = open_test_db("ac0070_03");
    create_items(&mut database, "items");
    database
        .insert("items", row_scalars(1, "original"))
        .expect("insert de la fila sembrada");

    let mut server = McpServer::new(database).expect("servidor MCP");
    authenticate(&mut server);
    let insert = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"query","arguments":{"sql":"INSERT INTO items (id, name) VALUES (9, 'x')"}}}"#;
    let response = call(&mut server, insert);

    let code = response["error"]["code"]
        .as_i64()
        .expect("un DML por MCP debe responder un error JSON-RPC (no result)");
    assert!(
        code == ERR_SERVER || code == ERR_INVALID_PARAMS,
        "el rechazo por permiso debe usar -32000 o -32602, se obtuvo {code}"
    );
    let message = response["error"]["message"]
        .as_str()
        .expect("el error debe traer message")
        .to_lowercase();
    assert!(
        !message.trim().is_empty(),
        "el mensaje de rechazo debe ser accionable (no vacío)"
    );
    assert!(
        message.contains("permiso")
            || message.contains("deneg")
            || message.contains("rol")
            || message.contains("read"),
        "el mensaje debe explicar el permiso de solo lectura: {message}"
    );

    let select = r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT * FROM items"}}}"#;
    let after = call(&mut server, select);
    let rows = after["result"]["rows"]
        .as_array()
        .expect("result.rows debe ser un array");
    assert_eq!(
        rows.len(),
        SEEDED_ROWS,
        "el DML rechazado no debe crear filas: {} filas",
        rows.len()
    );
    assert_eq!(
        rows[0]["name"].as_str(),
        Some("original"),
        "la fila sembrada debe quedar intacta"
    );
}

/// AC-0070-05 — una notificación JSON-RPC (sin `id`) no genera respuesta.
#[test]
// @spec AC-0070-05
fn test_ac_0070_05_notification_gets_no_response() {
    let (_guard, database) = open_test_db("ac0070_05");
    let mut server = McpServer::new(database).expect("servidor MCP");
    authenticate(&mut server);

    let raw = server.handle_message(r#"{"jsonrpc":"2.0","method":"tools/list"}"#);
    assert_eq!(
        raw, "",
        "JSON-RPC 2.0 prohíbe responder a una notificación (request sin id)"
    );
}
