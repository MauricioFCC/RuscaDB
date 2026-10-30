//! Protocolo PostgreSQL wire opcional (SPEC-0113): stub sin dependencias externas.

/// Servidor pgwire (stub).
#[derive(Debug, Clone)]
pub struct PgWireServer {
    /// Puerto de escucha (no usado en el stub).
    port: u16,
}

impl PgWireServer {
    /// Crea un servidor pgwire (stub).
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Puerto configurado.
    pub fn port(&self) -> u16 {
        self.port
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0113-01
    fn test_ac_0113_01_pgwire_server_create() {
        let server = PgWireServer::new(5432);
        assert_eq!(server.port(), 5432);
    }
}
