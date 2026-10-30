//! Tests de aceptacion de la feature `pgwire` (SPEC-0113).

#![cfg(feature = "pgwire")]
#![allow(clippy::expect_used)]

use ruscadb::{FrontendMessage, PgWireServer};

#[test] // @spec AC-0113-01
fn test_ac_0113_01_pgwire_server_create() {
    let server = PgWireServer::new(5432);
    assert_eq!(server.port(), 5432);

    // Query simple con longitud big-endian (protocolo v3).
    let mut query = vec![b'Q'];
    query.extend_from_slice(&13i32.to_be_bytes());
    query.extend_from_slice(b"SELECT 1\0");
    let (message, consumed) = PgWireServer::decode_frontend(&query).expect("query");
    assert_eq!(consumed, query.len());
    assert_eq!(message, FrontendMessage::Query("SELECT 1".to_string()));

    let response = PgWireServer::handle_query(&["id"], &[vec![Some("1".to_string())]], "SELECT 1");
    assert_eq!(response[0], b'T');
    assert_eq!(response.last(), Some(&b'I'));
}
