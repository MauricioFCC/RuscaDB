//! Tests de aceptación de la feature `pgwire-real` (SPEC-0127).

#![allow(clippy::expect_used)]

use ruscadb::PgWireRealServer;

#[test] // @spec AC-0127-01
fn test_ac_0127_01_pgwire_real_create() {
    let server = PgWireRealServer::new(5432);
    assert_eq!(server.port(), 5432);
}
