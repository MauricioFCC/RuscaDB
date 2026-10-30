//! Protocolo PostgreSQL wire v3 (SPEC-0113): codec de mensajes.
//!
//! Implementa la capa de serializacion del protocolo (frontend/backend) sin
//! servidor TCP ni dependencias: `Startup`, `Query`, `Terminate`, `Sync` y las
//! respuestas `AuthenticationOk`, `RowDescription`, `DataRow`,
//! `CommandComplete`, `ErrorResponse` y `ReadyForQuery`. El transporte
//! (`TcpListener`) queda como adapter del consumidor.

use std::collections::BTreeMap;

/// Version 3.0 del protocolo (`196608`).
pub const PROTOCOL_VERSION_3: i32 = 196_608;

/// Mensaje del cliente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendMessage {
    /// Handshake inicial con parametros (`user`, `database`, ...).
    Startup(BTreeMap<String, String>),
    /// Consulta simple (`Q`).
    Query(String),
    /// Cierre (`X`).
    Terminate,
    /// Sincronizacion (`S`).
    Sync,
}

/// Mensaje del servidor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendMessage {
    /// Autenticacion aceptada.
    AuthenticationOk,
    /// Parametro de sesion (`S`).
    ParameterStatus(String, String),
    /// Descripcion de columnas (`T`).
    RowDescription(Vec<(String, i32)>),
    /// Fila de datos (`D`).
    DataRow(Vec<Option<String>>),
    /// Fin de comando (`C`).
    CommandComplete(String),
    /// Error (`E`).
    ErrorResponse(String),
    /// Listo para la siguiente consulta (`Z`).
    ReadyForQuery,
}

/// Servidor pgwire (codec).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgWireServer {
    /// Puerto configurado (el bind lo hace el consumidor).
    port: u16,
}

impl PgWireServer {
    /// Crea el servidor.
    ///
    /// Args:
    ///     port: Puerto de escucha.
    ///
    /// Returns:
    ///     Servidor (codec) listo.
    pub const fn new(port: u16) -> Self {
        Self { port }
    }

    /// Puerto configurado.
    ///
    /// Returns:
    ///     Puerto.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Decodifica un mensaje del cliente.
    ///
    /// Args:
    ///     bytes: Buffer recibido (puede contener varios mensajes).
    ///
    /// Returns:
    ///     `(mensaje, bytes_consumidos)`.
    ///
    /// Errors:
    ///     Texto accionable si el buffer esta truncado o el tipo es invalido.
    pub fn decode_frontend(bytes: &[u8]) -> Result<(FrontendMessage, usize), String> {
        if bytes.len() < 5 {
            return Err("pgwire: cabecera truncada".to_string());
        }
        let tag = bytes[0];
        if matches!(tag, b'Q' | b'X' | b'S') {
            let length = i32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
            if length < 4 {
                return Err("pgwire: longitud invalida".to_string());
            }
            let total = 1 + length as usize;
            if bytes.len() < total {
                return Err("pgwire: mensaje truncado".to_string());
            }
            return match tag {
                b'Q' => {
                    let payload = &bytes[5..total];
                    let text = payload.strip_suffix(&[0]).unwrap_or(payload);
                    Ok((
                        FrontendMessage::Query(String::from_utf8_lossy(text).into_owned()),
                        total,
                    ))
                }
                b'X' => Ok((FrontendMessage::Terminate, total)),
                _ => Ok((FrontendMessage::Sync, total)),
            };
        }
        // Startup: longitud (incluida) + protocolo + pares key\0value\0 + \0.
        let length = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if length < 8 {
            return Err("pgwire: startup truncado".to_string());
        }
        let total = length as usize;
        if bytes.len() < total {
            return Err("pgwire: startup truncado".to_string());
        }
        let protocol = i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if protocol != PROTOCOL_VERSION_3 {
            return Err(format!("pgwire: version {protocol} no soportada"));
        }
        let mut parameters = BTreeMap::new();
        let payload = &bytes[8..total.saturating_sub(1)];
        let mut parts = payload
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty());
        while let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            parameters.insert(
                String::from_utf8_lossy(key).into_owned(),
                String::from_utf8_lossy(value).into_owned(),
            );
        }
        Ok((FrontendMessage::Startup(parameters), total))
    }

    /// Codifica un mensaje del servidor.
    ///
    /// Args:
    ///     message: Mensaje a serializar.
    ///
    /// Returns:
    ///     Bytes listos para el socket.
    pub fn encode_backend(message: &BackendMessage) -> Vec<u8> {
        match message {
            BackendMessage::AuthenticationOk => frame(b'R', &0i32.to_be_bytes()),
            BackendMessage::ParameterStatus(key, value) => {
                let mut payload = Vec::new();
                payload.extend_from_slice(key.as_bytes());
                payload.push(0);
                payload.extend_from_slice(value.as_bytes());
                payload.push(0);
                frame(b'S', &payload)
            }
            BackendMessage::RowDescription(columns) => {
                let mut payload = Vec::new();
                payload.extend_from_slice(&(columns.len() as i16).to_be_bytes());
                for (name, type_oid) in columns {
                    payload.extend_from_slice(name.as_bytes());
                    payload.push(0);
                    payload.extend_from_slice(&0i32.to_be_bytes()); // table oid
                    payload.extend_from_slice(&0i16.to_be_bytes()); // attr
                    payload.extend_from_slice(&type_oid.to_be_bytes());
                    payload.extend_from_slice(&(-1i16).to_be_bytes()); // size
                    payload.extend_from_slice(&(-1i32).to_be_bytes()); // modifier
                    payload.extend_from_slice(&0i16.to_be_bytes()); // text format
                }
                frame(b'T', &payload)
            }
            BackendMessage::DataRow(values) => {
                let mut payload = Vec::new();
                payload.extend_from_slice(&(values.len() as i16).to_be_bytes());
                for value in values {
                    match value {
                        Some(text) => {
                            payload.extend_from_slice(&(text.len() as i32).to_be_bytes());
                            payload.extend_from_slice(text.as_bytes());
                        }
                        None => payload.extend_from_slice(&(-1i32).to_be_bytes()),
                    }
                }
                frame(b'D', &payload)
            }
            BackendMessage::CommandComplete(tag) => {
                let mut payload = tag.as_bytes().to_vec();
                payload.push(0);
                frame(b'C', &payload)
            }
            BackendMessage::ErrorResponse(detail) => {
                let mut payload = vec![b'S'];
                payload.extend_from_slice(b"ERROR\0");
                payload.push(b'M');
                payload.extend_from_slice(detail.as_bytes());
                payload.push(0);
                payload.push(0);
                frame(b'E', &payload)
            }
            BackendMessage::ReadyForQuery => frame(b'Z', b"I"),
        }
    }

    /// Respuesta completa a una consulta simple.
    ///
    /// Args:
    ///     columns: Nombres de columnas (tipo texto, oid 25).
    ///     rows: Filas (`None` = NULL).
    ///     tag: Etiqueta de `CommandComplete` (p. ej. `SELECT 3`).
    ///
    /// Returns:
    ///     Concatenacion de `RowDescription + DataRow* + CommandComplete +
    ///     ReadyForQuery`.
    pub fn handle_query(columns: &[&str], rows: &[Vec<Option<String>>], tag: &str) -> Vec<u8> {
        let description = BackendMessage::RowDescription(
            columns
                .iter()
                .map(|name| ((*name).to_string(), 25))
                .collect(),
        );
        let mut out = Self::encode_backend(&description);
        for row in rows {
            out.extend(Self::encode_backend(&BackendMessage::DataRow(row.clone())));
        }
        out.extend(Self::encode_backend(&BackendMessage::CommandComplete(
            tag.to_string(),
        )));
        out.extend(Self::encode_backend(&BackendMessage::ReadyForQuery));
        out
    }
}

/// Enmarca un payload: tipo + longitud big-endian (incluida) + payload.
///
/// El protocolo PostgreSQL usa orden de red (big-endian) para todas las
/// longitudes y contadores enteros.
fn frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    out.push(tag);
    out.extend_from_slice(&((payload.len() + 4) as i32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::{BackendMessage, FrontendMessage, PROTOCOL_VERSION_3, PgWireServer};

    #[test] // @spec AC-0113-01
    fn test_ac_0113_01_pgwire_server_create() {
        let server = PgWireServer::new(5432);
        assert_eq!(server.port(), 5432);
    }

    /// Startup y Query se decodifican con el tamano consumido.
    #[test]
    fn decode_startup_and_query() {
        let mut startup = Vec::new();
        startup.extend_from_slice(&0i32.to_be_bytes()); // placeholder de longitud
        startup.extend_from_slice(&PROTOCOL_VERSION_3.to_be_bytes());
        startup.extend_from_slice(b"user\0ana\0database\0db\0\0");
        let length = startup.len() as i32;
        startup[0..4].copy_from_slice(&length.to_be_bytes());

        let (message, consumed) = PgWireServer::decode_frontend(&startup).expect("startup");
        assert_eq!(consumed, startup.len());
        match message {
            FrontendMessage::Startup(parameters) => {
                assert_eq!(parameters.get("user"), Some(&"ana".to_string()));
                assert_eq!(parameters.get("database"), Some(&"db".to_string()));
            }
            other => panic!("se esperaba Startup: {other:?}"),
        }

        let mut query = vec![b'Q'];
        query.extend_from_slice(&13i32.to_be_bytes());
        query.extend_from_slice(b"SELECT 1\0");
        let (message, consumed) = PgWireServer::decode_frontend(&query).expect("query");
        assert_eq!(consumed, query.len());
        assert_eq!(message, FrontendMessage::Query("SELECT 1".to_string()));

        assert!(PgWireServer::decode_frontend(b"Q\x01").is_err());
        assert!(PgWireServer::decode_frontend(&[0, 0, 0, 4]).is_err());
    }

    /// Codificacion de auth/ready/data y respuesta completa a una consulta.
    #[test]
    fn encode_backend_messages() {
        let auth = PgWireServer::encode_backend(&BackendMessage::AuthenticationOk);
        assert_eq!(auth[0], b'R');
        assert_eq!(&auth[1..5], &8i32.to_be_bytes());
        let ready = PgWireServer::encode_backend(&BackendMessage::ReadyForQuery);
        assert_eq!(ready, vec![b'Z', 0, 0, 0, 5, b'I']);

        let row = PgWireServer::encode_backend(&BackendMessage::DataRow(vec![
            Some("x".to_string()),
            None,
        ]));
        assert_eq!(row[0], b'D');
        assert!(
            row.windows(4)
                .any(|window| window == (-1i32).to_be_bytes().as_slice())
        );

        let response = PgWireServer::handle_query(
            &["id", "name"],
            &[
                vec![Some("1".to_string()), Some("ana".to_string())],
                vec![Some("2".to_string()), None],
            ],
            "SELECT 2",
        );
        assert_eq!(response[0], b'T');
        assert!(response.contains(&b'D'));
        assert!(response.contains(&b'C'));
        assert_eq!(response.last(), Some(&b'I'));
        // ErrorResponse bien formado.
        let error =
            PgWireServer::encode_backend(&BackendMessage::ErrorResponse("boom".to_string()));
        assert_eq!(error[0], b'E');
        assert!(error.windows(4).any(|window| window == b"boom"));
    }
}
