//! Servidor MCP (Model Context Protocol) transporte stdio (SPEC-0068).
//!
//! Local y sin red: un proceso padre lanza el binario y habla JSON-RPC 2.0 por
//! stdin/stdout. Expone tools de lectura e inspección (`query`, `tables`,
//! `integrity_check`); en esta iteración no hay escritura ni recursos.

use ruscadb::{Database, ScalarValue};
use serde_json::{Value, json};

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
}

impl McpServer {
    /// Crea un servidor sobre una base ya abierta.
    ///
    /// Args:
    ///     db: Base de datos (fachada `ruscadb`) a servir.
    ///
    /// Returns:
    ///     El servidor listo para procesar mensajes.
    pub fn new(db: Database) -> Self {
        Self { db }
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
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        match request.get("method").and_then(Value::as_str) {
            Some("tools/list") => ok_response(id, json!({ "tools": tool_catalog() })),
            Some("tools/call") => self.call_tool(&request, id),
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
                match self.db.execute(sql) {
                    Ok(rows) => ok_response(id, json!({ "rows": rows_to_json(&rows) })),
                    Err(error) => error_response(id, ERR_SERVER, format!("query falló: {error}")),
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
}

/// Catálogo de tools expuesto por `tools/list`.
///
/// Returns:
///     Array JSON con `name` y `description` de cada tool.
fn tool_catalog() -> Value {
    json!([
        {
            "name": "query",
            "description": "Ejecuta una sentencia RQL (SELECT) y devuelve las filas como objetos."
        },
        {
            "name": "tables",
            "description": "Lista los nombres de tabla del catálogo."
        },
        {
            "name": "integrity_check",
            "description": "Verifica WAL, heap e índice vectorial y reporta banderas de integridad."
        }
    ])
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

    /// El catálogo expone exactamente tres tools con descripción.
    #[test]
    fn catalog_has_three_tools() {
        let catalog = tool_catalog();
        let tools = catalog.as_array().expect("array");
        assert_eq!(tools.len(), 3);
        assert!(
            tools
                .iter()
                .all(|tool| tool["description"].as_str().is_some_and(|d| !d.is_empty()))
        );
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
