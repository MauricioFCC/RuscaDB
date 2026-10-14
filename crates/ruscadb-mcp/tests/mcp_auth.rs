//! Tests de aceptación de la autenticación de sesión MCP (SPEC-0088).
//!
//! Fija el handshake `session/auth`, la ausencia de fugas de token en los
//! errores, la invalidación por rotación del keystore y la persistencia
//! del keystore en disco (KMS-lite).

// El oráculo puede fallar ruidosamente: el `allow-expect` del workspace no
// alcanza a los targets de integración del crate.
#![allow(clippy::expect_used)]

use std::path::PathBuf;

use ruscadb::{Database, DbConfig};
use ruscadb_mcp::McpServer;
use serde_json::Value;

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Código JSON-RPC de sesión no autenticada o invalidada.
const ERR_UNAUTHORIZED: i64 = -32001;

/// Borra los ficheros de una base temporal al finalizar el test.
struct TempDb {
    /// Ruta del fichero de datos `.db` de la base.
    path: PathBuf,
}

impl Drop for TempDb {
    /// Limpia `.db` y `.wal` al salir.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("wal"));
    }
}

/// Abre una base temporal de pruebas con pool amplio.
///
/// Args:
///     tag: Sufijo que aísla el fichero por test.
///
/// Returns:
///     La guarda temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (TempDb, Database) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("reloj del sistema")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ruscadb_mcp_auth_{tag}_{}_{nanos}.db",
        std::process::id()
    ));
    let database = Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
    (TempDb { path }, database)
}

/// Compone un request `session/auth` con el token dado.
///
/// Args:
///     token: Token de sesión a presentar.
///
/// Returns:
///     El mensaje JSON-RPC 2.0 como texto.
fn auth_message(token: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":0,"method":"session/auth","params":{{"token":"{token}"}}}}"#)
}

/// Envía un mensaje y devuelve la respuesta cruda (texto JSON).
///
/// Args:
///     server: Servidor MCP destino.
///     message: Mensaje JSON-RPC 2.0 de entrada.
///
/// Returns:
///     La respuesta serializada como texto.
fn call_raw(server: &mut McpServer, message: &str) -> String {
    server.handle_message(message)
}

/// Envía un mensaje y devuelve la respuesta parseada como JSON.
///
/// Args:
///     server: Servidor MCP destino.
///     message: Mensaje JSON-RPC 2.0 de entrada.
///
/// Returns:
///     La respuesta parseada como `serde_json::Value`.
fn call(server: &mut McpServer, message: &str) -> Value {
    let raw = call_raw(server, message);
    serde_json::from_str(&raw).expect("la respuesta debe ser JSON válido")
}

/// AC-0088-01 — sin sesión, los métodos de datos devuelven -32001.
#[test]
// @spec AC-0088-01
fn test_ac_0088_01_unauthenticated_session_rejected_by_default() {
    let (_guard, database) = open_test_db("ac0088_01");
    let mut server = McpServer::new(database).expect("servidor MCP");

    for method in ["tools/list", "resources/list"] {
        let message = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}"}}"#);
        let response = call(&mut server, &message);
        assert_eq!(
            response["error"]["code"].as_i64(),
            Some(ERR_UNAUTHORIZED),
            "{method} sin sesión debe devolver {ERR_UNAUTHORIZED}"
        );
    }
    let call_dml = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT 1"}}}"#;
    let response = call(&mut server, call_dml);
    assert_eq!(
        response["error"]["code"].as_i64(),
        Some(ERR_UNAUTHORIZED),
        "tools/call sin sesión debe devolver {ERR_UNAUTHORIZED}"
    );

    let token = server.issue_session_token().expect("token de sesión");
    let authed = call(&mut server, &auth_message(&token));
    assert_eq!(
        authed["result"]["authenticated"].as_bool(),
        Some(true),
        "session/auth con token emitido autentica"
    );

    let listed = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
    );
    assert!(
        listed["result"]["tools"].is_array(),
        "tras autenticar, tools/list responde con la lista"
    );
}

/// AC-0088-02 — tokens corruptos se rechazan sin filtrar secretos.
#[test]
// @spec AC-0088-02
fn test_ac_0088_02_invalid_token_rejected_without_leak() {
    let (_guard, database) = open_test_db("ac0088_02");
    let mut server = McpServer::new(database).expect("servidor MCP");
    let token = server.issue_session_token().expect("token de sesión");

    let mut corrupted = token.clone();
    let last = corrupted.pop().expect("token no vacío");
    // hex::decode es case-insensitive: cambiar 'a'→'A' NO cambia el byte
    // (flaky 1/16 observado en CI); se fuerza un nibble distinto ('b'/'c').
    let replacement = if last.eq_ignore_ascii_case(&'b') {
        'c'
    } else {
        'b'
    };
    corrupted.push(replacement);
    let raw = call_raw(&mut server, &auth_message(&corrupted));
    let response: Value = serde_json::from_str(&raw).expect("respuesta JSON");
    assert_eq!(
        response["error"]["code"].as_i64(),
        Some(ERR_UNAUTHORIZED),
        "token con tag AEAD inválido se rechaza"
    );
    assert!(
        !raw.contains(&token),
        "la respuesta jamás puede contener el token válido: {raw}"
    );
    assert!(
        !raw.contains(&corrupted),
        "la respuesta jamás puede ecoar el token presentado: {raw}"
    );

    let forged = "v1.k99.deadbeefdeadbeefdeadbeefdeadbeefdeadbeef.cafebabe";
    let raw_forged = call_raw(&mut server, &auth_message(forged));
    let response: Value = serde_json::from_str(&raw_forged).expect("respuesta JSON");
    assert_eq!(
        response["error"]["code"].as_i64(),
        Some(ERR_UNAUTHORIZED),
        "kid desconocido se rechaza"
    );
    assert!(
        !raw_forged.contains("deadbeef"),
        "la respuesta no puede filtrar material del token: {raw_forged}"
    );

    let listed = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#,
    );
    assert_eq!(
        listed["error"]["code"].as_i64(),
        Some(ERR_UNAUTHORIZED),
        "un auth fallido no deja la sesión abierta"
    );
}

/// AC-0088-03 — rotar el keystore invalida la sesión vigente.
#[test]
// @spec AC-0088-03
fn test_ac_0088_03_rotation_invalidates_existing_sessions() {
    let (_guard, database) = open_test_db("ac0088_03");
    let mut server = McpServer::new(database).expect("servidor MCP");
    let old_token = server.issue_session_token().expect("token antiguo");
    let authed = call(&mut server, &auth_message(&old_token));
    assert_eq!(authed["result"]["authenticated"].as_bool(), Some(true));
    let listed = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
    );
    assert!(listed["result"]["tools"].is_array(), "la sesión funciona");

    let new_token = server.rotate_keystore().expect("rotación");

    let listed_after = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    );
    assert_eq!(
        listed_after["error"]["code"].as_i64(),
        Some(ERR_UNAUTHORIZED),
        "la rotación invalida la sesión vigente"
    );

    let reauth = call(&mut server, &auth_message(&old_token));
    assert_eq!(
        reauth["error"]["code"].as_i64(),
        Some(ERR_UNAUTHORIZED),
        "el token de la key retirada ya no autentica"
    );

    let reauth_new = call(&mut server, &auth_message(&new_token));
    assert_eq!(
        reauth_new["result"]["authenticated"].as_bool(),
        Some(true),
        "el token de la key nueva autentica"
    );
    let listed_new = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
    );
    assert!(
        listed_new["result"]["tools"].is_array(),
        "la sesión re-autenticada vuelve a funcionar"
    );
}

/// AC-0088-04 — el keystore persiste tokens y rotaciones en disco.
#[test]
// @spec AC-0088-04
fn test_ac_0088_04_keystore_persists_across_reloads() {
    let dir = tempfile::tempdir().expect("tempdir del keystore");
    let keystore_path = dir.path().join("session.keystore");

    let (_guard_a, db_a) = open_test_db("ac0088_04a");
    let mut server =
        McpServer::with_keystore(db_a, &keystore_path, false).expect("servidor con keystore");
    let token = server.issue_session_token().expect("token emitido");
    let authed = call(&mut server, &auth_message(&token));
    assert_eq!(authed["result"]["authenticated"].as_bool(), Some(true));
    drop(server);

    // "Proceso nuevo": otra base, mismo keystore en disco.
    let (_guard_b, db_b) = open_test_db("ac0088_04b");
    let mut reopened =
        McpServer::with_keystore(db_b, &keystore_path, false).expect("recarga del keystore");
    let authed_again = call(&mut reopened, &auth_message(&token));
    assert_eq!(
        authed_again["result"]["authenticated"].as_bool(),
        Some(true),
        "el token emitido por el proceso anterior sigue validando"
    );

    let text = std::fs::read_to_string(&keystore_path).expect("lectura del keystore");
    assert!(text.contains("\"version\": 1"), "formato versionado");
    assert!(text.contains("\"kid\""), "registro de kids presente");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&keystore_path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "permisos 0600 en unix");
    }

    // La rotación también persiste: un tercer proceso hereda la retirada.
    let _ = reopened.rotate_keystore().expect("rotación persistida");
    drop(reopened);
    let (_guard_c, db_c) = open_test_db("ac0088_04c");
    let mut third = McpServer::with_keystore(db_c, &keystore_path, false).expect("recarga 2");
    let stale = call(&mut third, &auth_message(&token));
    assert_eq!(
        stale["error"]["code"].as_i64(),
        Some(ERR_UNAUTHORIZED),
        "la rotación persistida invalida el token en un proceso posterior"
    );
}
