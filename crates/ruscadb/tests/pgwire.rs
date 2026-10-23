//! Tests de aceptación de la feature `pgwire` (SPEC-0113).

#![allow(clippy::expect_used)]

use ruscadb::PgWireServer;

#[test] // @spec AC-0113-01
fn test_ac_0113_01_pgwire_server_create() {
    let server = PgWireServer::new(5432);
    assert_eq!(server.port(), 5432);
}
