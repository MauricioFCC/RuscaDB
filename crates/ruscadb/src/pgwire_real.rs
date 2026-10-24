//! PostgreSQL wire protocol real (SPEC-0127): stub sin dependencias externas.

/// Servidor pgwire real (stub).
#[derive(Debug, Clone)]
pub struct PgWireRealServer {
    /// Puerto de escucha.
    port: u16,
}

impl PgWireRealServer {
    /// Crea un servidor pgwire real (stub).
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

    #[test] // @spec AC-0127-01
    fn test_ac_0127_01_pgwire_real_create() {
        let server = PgWireRealServer::new(5432);
        assert_eq!(server.port(), 5432);
    }
}
