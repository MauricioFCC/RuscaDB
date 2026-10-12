//! Tests de aceptación del batch JSON-RPC del servidor MCP (SPEC-0071, AC-04).
//!
//! RED **por aserción**: hoy `handle_message` con un array devuelve un único
//! error `-32602` (el array no tiene `method`), en vez de una respuesta por
//! elemento como exige JSON-RPC 2.0.

#![allow(clippy::expect_used)]

use std::path::PathBuf;

use ruscadb::{Database, DbConfig};
use ruscadb_mcp::McpServer;
use serde_json::Value;

/// Capacidad del buffer pool en los tests.
const POOL_CAPACITY: usize = 128;

/// Borra los ficheros de la base temporal al finalizar el test.
struct TempDb {
    /// Ruta del fichero `.db`.
    path: PathBuf,
}

impl Drop for TempDb {
    /// Limpia `.db` y `.wal` al salir.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("wal"));
    }
}

/// Abre una base temporal única.
///
/// Returns:
///     La guarda de limpieza y la base abierta.
fn open_test_db() -> (TempDb, Database) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("reloj")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ruscadb_mcp_batch_{}_{nanos}.db",
        std::process::id()
    ));
    let database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
    (TempDb { path }, database)
}

/// AC-0071-04 — un batch JSON-RPC devuelve una respuesta por elemento.
#[test]
// @spec AC-0071-04
fn test_ac_0071_04_mcp_batch_returns_array() {
    let (_guard, database) = open_test_db();
    let mut server = McpServer::new(database);

    let batch = r#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"},{"jsonrpc":"2.0","id":2,"method":"foo/bar"}]"#;
    let raw = server.handle_message(batch);
    let response: Value = serde_json::from_str(&raw).expect("la respuesta debe ser JSON");

    let items = response
        .as_array()
        .expect("un batch debe responder con un array");
    assert_eq!(items.len(), 2, "una respuesta por elemento del batch");
    assert_eq!(
        items[0]["id"].as_i64(),
        Some(1),
        "el primer elemento conserva su id"
    );
    assert!(
        items[0]["result"]["tools"].is_array(),
        "el primer elemento debe traer la lista de tools"
    );
    assert_eq!(
        items[1]["error"]["code"].as_i64(),
        Some(-32601),
        "el segundo elemento debe responder -32601"
    );
}
