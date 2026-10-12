//! Servidor MCP (Model Context Protocol) transporte stdio (SPEC-0068).
//!
//! Local y sin red: un proceso padre lanza el binario y habla JSON-RPC 2.0 por
//! stdin/stdout. Expone tools de lectura e inspección (`query`, `tables`,
//! `integrity_check`); en esta iteración no hay escritura ni recursos.

use ruscadb::{Database, ScalarValue};
use serde_json::{Value, json};

/// Esquema de URI de los recursos de tabla MCP (SPEC-0075).
const TABLE_URI_SCHEME: &str = "ruscadb://table/";

/// Código JSON-RPC: parámetros inválidos.
const ERR_INVALID_PARAMS: i64 = -32602;
/// Código JSON-RPC: método no encontrado.
const ERR_METHOD_NOT_FOUND: i64 = -32601;
/// Código JSON-RPC: error de ejecución del servidor.
const ERR_SERVER: i64 = -32000;
/// Código JSON-RPC: JSON de entrada inválido.
const ERR_PARSE: i64 = -32700;

/// Servidor MCP que mantiene la base y procesa mensajes JSON-RPC.
pub struct McpServer {
    /// Base de datos servida por el servidor.
    db: Database,
    /// Modo escritura opt-in (SPEC-0075): `false` = solo lectura.
    read_write: bool,
}

impl McpServer {
    /// Crea un servidor **de solo lectura** sobre una base ya abierta.
    ///
    /// Args:
    ///     db: Base de datos (fachada `ruscadb`) a servir.
    ///
    /// Returns:
    ///     El servidor listo para procesar mensajes (DML rechazado).
    pub fn new(db: Database) -> Self {
        Self {
            db,
            read_write: false,
        }
    }

    /// Crea un servidor con **escritura opt-in** (SPEC-0075).
    ///
    /// La escritura sigue delegando el permiso real al RBAC de la `Database`
    /// (SPEC-0066): con rol `Read` las escrituras devuelven `PermissionDenied`.
    ///
    /// Args:
    ///     db: Base de datos a servir.
    ///
    /// Returns:
    ///     El servidor con DML y tool `insert` habilitados.
    pub fn read_write(db: Database) -> Self {
        Self {
            db,
            read_write: true,
        }
    }

    /// Procesa un mensaje JSON-RPC 2.0 y devuelve la respuesta serializada.
    ///
    /// Nunca entra en panic: JSON inválido, métodos desconocidos o parámetros
    /// mal formados se responden como errores JSON-RPC con mensaje accionable.
    ///
    /// Args:
    ///     json: Mensaje JSON-RPC de entrada (una línea).
    ///
    /// Returns:
    ///     La respuesta JSON-RPC (`result` o `error`) como texto JSON.
    pub fn handle_message(&mut self, json: &str) -> String {
        let request: Value = match serde_json::from_str(json) {
            Ok(value) => value,
            Err(error) => {
                return error_response(Value::Null, ERR_PARSE, format!("JSON inválido: {error}"));
            }
        };
        // Batch JSON-RPC (SPEC-0071): un array devuelve una respuesta por
        // elemento, serializadas juntas como array.
        if let Value::Array(items) = request {
            let responses: Vec<String> =
                items.iter().map(|item| self.handle_single(item)).collect();
            return format!("[{}]", responses.join(","));
        }
        self.handle_single(&request)
    }

    /// Procesa un request JSON-RPC individual (notificación → sin respuesta).
    ///
    /// Args:
    ///     request: Request JSON-RPC ya parseado.
    ///
    /// Returns:
    ///     La respuesta JSON, o cadena vacía si es una notificación (sin `id`).
    fn handle_single(&mut self, request: &Value) -> String {
        // JSON-RPC 2.0: las notificaciones (sin `id`) NO se responden.
        let Some(id) = request.get("id").cloned() else {
            return String::new();
        };
        match request.get("method").and_then(Value::as_str) {
            Some("tools/list") => ok_response(id, json!({ "tools": self.tool_catalog() })),
            Some("tools/call") => self.call_tool(request, id),
            Some("resources/list") => self.list_resources(id),
            Some("resources/read") => self.read_resource(request, id),
            Some(other) => error_response(
                id,
                ERR_METHOD_NOT_FOUND,
                format!(
                    "método desconocido '{other}': solo se admiten 'tools/list' y 'tools/call'"
                ),
            ),
            None => error_response(
                id,
                ERR_INVALID_PARAMS,
                "el request debe incluir el campo 'method' (string)".to_string(),
            ),
        }
    }

    /// Ejecuta `tools/call` con sus validaciones de params.
    ///
    /// Args:
    ///     request: Request JSON-RPC completo.
    ///     id: Identificador a ecoar en la respuesta.
    ///
    /// Returns:
    ///     La respuesta serializada (result o error JSON-RPC).
    fn call_tool(&mut self, request: &Value, id: Value) -> String {
        let Some(params) = request.get("params") else {
            return error_response(
                id,
                ERR_INVALID_PARAMS,
                "tools/call requiere 'params' con 'name' y 'arguments'".to_string(),
            );
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return error_response(
                id,
                ERR_INVALID_PARAMS,
                "tools/call requiere 'params.name' (string)".to_string(),
            );
        };
        let Some(arguments) = params.get("arguments") else {
            return error_response(
                id,
                ERR_INVALID_PARAMS,
                format!("la tool '{name}' requiere 'params.arguments'"),
            );
        };
        match name {
            "query" => {
                let Some(sql) = arguments.get("sql").and_then(Value::as_str) else {
                    return error_response(
                        id,
                        ERR_INVALID_PARAMS,
                        "la tool 'query' requiere 'arguments.sql' (string)".to_string(),
                    );
                };
                // El transporte MCP es de solo lectura salvo opt-in (SPEC-0070/0075).
                if is_write_sql(sql) && !self.read_write {
                    return error_response(
                        id,
                        ERR_SERVER,
                        "permiso denegado: el servidor MCP es de solo lectura (usa \
                         McpServer::read_write o el SDK embebido para escribir)"
                            .to_string(),
                    );
                }
                match self.db.execute(sql) {
                    Ok(rows) => ok_response(id, json!({ "rows": rows_to_json(&rows) })),
                    Err(error) => error_response(id, ERR_SERVER, format!("query falló: {error}")),
                }
            }
            "insert" if self.read_write => {
                let Some(table) = arguments.get("table").and_then(Value::as_str) else {
                    return error_response(
                        id,
                        ERR_INVALID_PARAMS,
                        "la tool 'insert' requiere 'arguments.table' (string)".to_string(),
                    );
                };
                let Some(values) = arguments.get("values").and_then(Value::as_object) else {
                    return error_response(
                        id,
                        ERR_INVALID_PARAMS,
                        "la tool 'insert' requiere 'arguments.values' (objeto columna->valor)"
                            .to_string(),
                    );
                };
                let scalars: ruscadb::ScalarMap = values
                    .iter()
                    .map(|(column, value)| (column.clone(), json_to_scalar(value)))
                    .collect();
                match self.db.insert(table, scalars) {
                    Ok(record) => {
                        ok_response(id, json!({ "inserted": true, "id": format!("{record}") }))
                    }
                    Err(error) => error_response(id, ERR_SERVER, format!("insert falló: {error}")),
                }
            }
            "tables" => match self.db.tables() {
                Ok(tables) => ok_response(id, json!({ "tables": tables })),
                Err(error) => error_response(id, ERR_SERVER, format!("tables falló: {error}")),
            },
            "integrity_check" => match self.db.integrity_check() {
                Ok(report) => ok_response(
                    id,
                    json!({
                        "wal_ok": report.wal_ok,
                        "heap_ok": report.heap_ok,
                        "vector_ok": report.vector_ok,
                        "details": report.details,
                    }),
                ),
                Err(error) => {
                    error_response(id, ERR_SERVER, format!("integrity_check falló: {error}"))
                }
            },
            other => error_response(
                id,
                ERR_INVALID_PARAMS,
                format!("tool desconocida '{other}': disponibles query, tables, integrity_check"),
            ),
        }
    }

    /// Lista las tablas como recursos MCP (`ruscadb://table/<name>`).
    ///
    /// Args:
    ///     id: Identificador del request (ecoado).
    ///
    /// Returns:
    ///     La respuesta con `result.resources`.
    fn list_resources(&mut self, id: Value) -> String {
        match self.db.tables() {
            Ok(tables) => {
                let resources: Vec<Value> = tables
                    .iter()
                    .map(|name| {
                        json!({
                            "uri": format!("{TABLE_URI_SCHEME}{name}"),
                            "name": name,
                        })
                    })
                    .collect();
                ok_response(id, json!({ "resources": resources }))
            }
            Err(error) => error_response(id, ERR_SERVER, format!("resources/list falló: {error}")),
        }
    }

    /// Lee un recurso de tabla y devuelve sus filas como JSON.
    ///
    /// Args:
    ///     request: Request JSON-RPC (con `params.uri`).
    ///     id: Identificador del request (ecoado).
    ///
    /// Returns:
    ///     La respuesta con `result.contents[0].text` (filas serializadas).
    fn read_resource(&mut self, request: &Value, id: Value) -> String {
        let Some(uri) = request
            .get("params")
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
        else {
            return error_response(
                id,
                ERR_INVALID_PARAMS,
                "resources/read requiere 'params.uri' (string)".to_string(),
            );
        };
        let Some(table) = uri.strip_prefix(TABLE_URI_SCHEME) else {
            return error_response(
                id,
                ERR_INVALID_PARAMS,
                format!("uri desconocido '{uri}': se espera '{TABLE_URI_SCHEME}<tabla>'"),
            );
        };
        let sql = format!("SELECT * FROM {table}");
        match self.db.execute(&sql) {
            Ok(rows) => ok_response(
                id,
                json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": rows_to_json(&rows).to_string(),
                    }]
                }),
            ),
            Err(error) => error_response(
                id,
                ERR_INVALID_PARAMS,
                format!("recurso '{uri}' no legible: {error}"),
            ),
        }
    }

    /// Catálogo de tools expuesto por `tools/list`.
    ///
    /// Returns:
    ///     Array JSON con `name` y `description` de cada tool; incluye `insert`
    ///     solo en modo escritura (SPEC-0075).
    fn tool_catalog(&self) -> Value {
        let mut tools = vec![
            json!({
                "name": "query",
                "description": "Ejecuta una sentencia RQL (SELECT) y devuelve las filas como objetos."
            }),
            json!({
                "name": "tables",
                "description": "Lista los nombres de tabla del catálogo."
            }),
            json!({
                "name": "integrity_check",
                "description": "Verifica WAL, heap e índice vectorial y reporta banderas de integridad."
            }),
        ];
        if self.read_write {
            tools.push(json!({
                "name": "insert",
                "description": "Inserta una fila (arguments.table y arguments.values columna->valor)."
            }));
        }
        Value::Array(tools)
    }
}

/// Convierte un valor JSON a un escalar del dominio (tool `insert`).
///
/// Args:
///     value: Valor JSON (número, string, bool o null).
///
/// Returns:
///     El `ScalarValue` equivalente.
fn json_to_scalar(value: &Value) -> ScalarValue {
    match value {
        Value::Null => ScalarValue::Null,
        Value::Bool(inner) => ScalarValue::Bool(*inner),
        Value::Number(inner) => {
            if let Some(int) = inner.as_i64() {
                ScalarValue::Int(int)
            } else {
                ScalarValue::Float(inner.as_f64().unwrap_or_default())
            }
        }
        Value::String(inner) => ScalarValue::Text(inner.clone()),
        other => ScalarValue::Text(other.to_string()),
    }
}

/// Indica si una sentencia SQL es de escritura (guard del transporte MCP).
///
/// Args:
///     sql: Texto SQL.
///
/// Returns:
///     `true` si el primer verbo es `INSERT`/`UPDATE`/`DELETE`/`CREATE`/`DROP`.
fn is_write_sql(sql: &str) -> bool {
    let verb = sql
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(
        verb.as_str(),
        "INSERT" | "UPDATE" | "DELETE" | "CREATE" | "DROP"
    )
}

/// Convierte filas de la fachada a JSON (objetos `columna -> valor`).
///
/// Args:
///     rows: Filas devueltas por `Database::execute`.
///
/// Returns:
///     Array JSON con un objeto por fila.
fn rows_to_json(rows: &[ruscadb::Row]) -> Value {
    Value::Array(
        rows.iter()
            .map(|row| {
                let object: serde_json::Map<String, Value> = row
                    .iter()
                    .map(|(column, value)| (column.clone(), scalar_to_json(value)))
                    .collect();
                Value::Object(object)
            })
            .collect(),
    )
}

/// Mapea un `ScalarValue` del dominio a un valor JSON.
///
/// Args:
///     value: Escalar del dominio.
///
/// Returns:
///     El valor JSON equivalente (números, strings, booleanos o null).
fn scalar_to_json(value: &ScalarValue) -> Value {
    match value {
        ScalarValue::Null => Value::Null,
        ScalarValue::Bool(inner) => Value::Bool(*inner),
        ScalarValue::Int(inner) => json!(inner),
        ScalarValue::UInt(inner) => json!(inner),
        ScalarValue::Float(inner) => json!(inner),
        ScalarValue::Text(inner) => Value::String(inner.clone()),
        ScalarValue::TimestampMillis(inner) => json!(inner),
        other => Value::String(format!("{other:?}")),
    }
}

/// Compone una respuesta JSON-RPC con `result`.
///
/// Args:
///     id: Identificador del request (ecoado).
///     result: Resultado a serializar.
///
/// Returns:
///     La respuesta JSON como texto.
fn ok_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// Compone una respuesta JSON-RPC con `error` (sin `result`).
///
/// Args:
///     id: Identificador del request (ecoado).
///     code: Código JSON-RPC.
///     message: Mensaje accionable.
///
/// Returns:
///     La respuesta JSON como texto.
fn error_response(id: Value, code: i64, message: String) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// Un JSON inválido responde con el código de parseo.
    #[test]
    fn invalid_json_reports_parse_error() {
        let response = error_response(Value::Null, ERR_PARSE, "x".to_string());
        let parsed: Value = serde_json::from_str(&response).expect("json");
        assert_eq!(parsed["error"]["code"], json!(ERR_PARSE));
        assert!(parsed.get("result").is_none());
    }

    /// Los escalares se mapean a tipos JSON nativos.
    #[test]
    fn scalar_mapping() {
        assert_eq!(scalar_to_json(&ScalarValue::Int(7)), json!(7));
        assert_eq!(
            scalar_to_json(&ScalarValue::Text("x".to_string())),
            json!("x")
        );
        assert_eq!(scalar_to_json(&ScalarValue::Bool(true)), json!(true));
        assert_eq!(scalar_to_json(&ScalarValue::Null), Value::Null);
    }
}
