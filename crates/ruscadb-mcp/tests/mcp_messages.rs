//! Tests de aceptación del servidor MCP (SPEC-0068), transporte stdio.
//!
//! RED por diseño: `McpServer::handle_message` aún no existe en el crate
//! `ruscadb-mcp`, por lo que la compilación de esta suite falla con E0599
//! (`no method named handle_message`). Los tests fijan el contrato JSON-RPC 2.0
//! de los tools `query`, `tables` e `integrity_check`, los códigos de error y la
//! ausencia de dependencias de red (solo stdio + `serde_json`).

// El oráculo puede fallar ruidosamente: el `allow-expect` del workspace no
// alcanza a los targets de integración del crate.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, ScalarMap, ScalarValue,
};
use ruscadb_mcp::McpServer;
use serde_json::Value;

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Código JSON-RPC de método no encontrado.
const ERR_METHOD_NOT_FOUND: i64 = -32601;

/// Código JSON-RPC de parámetros inválidos.
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
        "ruscadb_mcp_{tag}_{}_{nanos}.db",
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

/// Crea un embedding L2 de la dimensión del vector.
///
/// Args:
///     values: Componentes del vector en `f64`.
///
/// Returns:
///     El embedding L2 construido.
fn embedding(values: &[f64]) -> Embedding {
    let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
    Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "test".to_string(),
            dim: values.len(),
            metric: Metric::L2,
        },
    )
    .expect("embedding válido")
}

/// Construye un `Record` con escalares y vector (sin aristas).
///
/// Args:
///     id: Identificador del registro.
///     scalars: Escalares del registro.
///     vector: Componentes del embedding.
///
/// Returns:
///     El registro listo para `insert_record`.
fn vector_record(id: RecordId, scalars: ScalarMap, vector: &[f64]) -> Record {
    Record {
        id,
        scalars,
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(embedding(vector)),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Siembra `items` con filas vectoriales de ejemplo.
///
/// Args:
///     database: Base destino (debe tener la tabla `items`).
fn seed_vector_rows(database: &mut Database) {
    let data = [
        (1_i64, "origen", [0.0, 0.0, 0.0]),
        (2, "cerca", [0.1, 0.1, 0.1]),
        (3, "lejos", [9.0, 9.0, 9.0]),
        (4, "medio", [3.0, 3.0, 3.0]),
    ];
    for (id, name, vector) in data {
        let record = vector_record(RecordId::new(), row_scalars(id, name), &vector);
        database
            .insert_record("items", record)
            .expect("insert_record");
    }
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

/// AC-0068-01 — `tools/list` devuelve las tres tools con nombre y descripción.
#[test]
// @spec AC-0068-01
fn test_ac_0068_01_tools_list_returns_tools() {
    let (_guard, database) = open_test_db("ac0068_01");
    let mut server = McpServer::new(database);

    let response = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
    );

    assert_eq!(
        response["id"].as_i64(),
        Some(1),
        "el id del request debe ecoarse"
    );
    let tools = response["result"]["tools"]
        .as_array()
        .expect("result.tools debe ser un array");
    assert_eq!(tools.len(), 3, "se esperan exactamente tres tools");
    let mut names: Vec<&str> = tools
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .expect("cada tool debe exponer un name textual")
        })
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["integrity_check", "query", "tables"],
        "las tools mínimas son query, tables e integrity_check"
    );
    for tool in tools {
        let description = tool["description"]
            .as_str()
            .expect("cada tool debe exponer una description");
        assert!(
            !description.trim().is_empty(),
            "la description de la tool no debe estar vacía"
        );
    }
}

/// AC-0068-02 — `tools/call` con `query` devuelve las filas tipadas.
///
/// Shape asumido y fijado aquí: `result.rows` es un array donde cada fila es un
/// objeto `columna -> valor JSON` (la `impl` debe mapear `ScalarValue` a JSON).
#[test]
// @spec AC-0068-02
fn test_ac_0068_02_tools_call_query_returns_rows() {
    let (_guard, mut database) = open_test_db("ac0068_02");
    create_items(&mut database, "items");
    database
        .insert("items", row_scalars(1, "uno"))
        .expect("insert de la fila sembrada");
    let mut server = McpServer::new(database);

    let message = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query","arguments":{"sql":"SELECT * FROM items"}}}"#;
    let response = call(&mut server, message);

    assert_eq!(
        response["id"].as_i64(),
        Some(2),
        "el id del request debe ecoarse"
    );
    let rows = response["result"]["rows"]
        .as_array()
        .expect("result.rows debe ser un array");
    assert_eq!(rows.len(), 1, "solo se sembró una fila");
    assert_eq!(
        rows[0]["id"].as_i64(),
        Some(1),
        "el id insertado debe viajar como número JSON"
    );
    assert_eq!(
        rows[0]["name"].as_str(),
        Some("uno"),
        "el texto insertado debe viajar como string JSON"
    );
}

/// AC-0068-03 — método desconocido y params inválidos dan errores JSON-RPC.
#[test]
// @spec AC-0068-03
fn test_ac_0068_03_unknown_method_and_bad_params_error() {
    let (_guard, database) = open_test_db("ac0068_03");
    let mut server = McpServer::new(database);

    let unknown = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":3,"method":"foo/bar"}"#,
    );
    assert_eq!(
        unknown["error"]["code"].as_i64(),
        Some(ERR_METHOD_NOT_FOUND),
        "un método desconocido debe responder -32601"
    );
    assert!(
        !unknown["error"]["message"]
            .as_str()
            .expect("el error debe traer message")
            .trim()
            .is_empty(),
        "el mensaje de método desconocido debe ser accionable (no vacío)"
    );
    assert!(
        unknown.get("result").is_none(),
        "una respuesta de error no debe incluir result"
    );

    let missing_params = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call"}"#,
    );
    assert_eq!(
        missing_params["error"]["code"].as_i64(),
        Some(ERR_INVALID_PARAMS),
        "tools/call sin params debe responder -32602"
    );
    assert!(
        !missing_params["error"]["message"]
            .as_str()
            .expect("el error debe traer message")
            .trim()
            .is_empty(),
        "el mensaje de params inválidos debe ser accionable (no vacío)"
    );

    let missing_arguments = call(
        &mut server,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"query"}}"#,
    );
    assert_eq!(
        missing_arguments["error"]["code"].as_i64(),
        Some(ERR_INVALID_PARAMS),
        "query sin arguments debe responder -32602"
    );
}

/// AC-0068-04 — `integrity_check` reporta `wal_ok`/`heap_ok`/`vector_ok`.
#[test]
// @spec AC-0068-04
fn test_ac_0068_04_integrity_check_tool_reports() {
    let (_guard, mut database) = open_test_db("ac0068_04");
    create_items(&mut database, "items");
    seed_vector_rows(&mut database);
    let mut server = McpServer::new(database);

    let message = r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"integrity_check","arguments":{}}}"#;
    let response = call(&mut server, message);

    let result = &response["result"];
    assert_eq!(
        result["wal_ok"].as_bool(),
        Some(true),
        "el WAL sano debe reportarse íntegro"
    );
    assert_eq!(
        result["heap_ok"].as_bool(),
        Some(true),
        "el heap sano debe reportarse coherente"
    );
    assert_eq!(
        result["vector_ok"].as_bool(),
        Some(true),
        "la cobertura del índice vectorial debe ser completa"
    );
}

/// AC-0068-05 — el crate es solo stdio + `serde_json` (sin tokio/hyper) y compila.
#[test]
// @spec AC-0068-05
fn test_ac_0068_05_crate_is_stdio_only_and_builds() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("lee el Cargo.toml del crate");
    for banned in ["tokio", "hyper"] {
        assert!(
            !text.contains(banned),
            "el crate no debe depender de {banned} (solo stdio + serde_json)"
        );
    }
    assert!(
        text.contains("serde_json"),
        "el crate debe declarar serde_json para el JSON-RPC"
    );
    // El mero hecho de compilar esta suite prueba que el crate construye.
}
