//! Observabilidad opcional (SPEC-0119): mÃ©tricas en memoria.

use std::collections::BTreeMap;

/// Registry de mÃ©tricas en memoria.
#[derive(Debug, Clone, Default)]
pub struct MetricsRegistry {
    /// Contadores por nombre.
    counters: BTreeMap<String, u64>,
}

impl MetricsRegistry {
    /// Crea un registry vacÃ­o.
    pub fn new() -> Self {
        Self::default()
    }

    /// Incrementa un contador.
    pub fn inc(&mut self, name: &str) {
        *self.counters.entry(name.to_string()).or_insert(0) += 1;
    }

    /// Incrementa un contador por un valor.
    pub fn inc_by(&mut self, name: &str, value: u64) {
        *self.counters.entry(name.to_string()).or_insert(0) += value;
    }

    /// Obtiene el valor de un contador.
    pub fn get(&self, name: &str) -> u64 {
        self.counters.get(name).copied().unwrap_or(0)
    }

    /// Exporta las mÃ©tricas en formato texto Prometheus.
    pub fn export(&self) -> String {
        let mut out = String::new();
        for (name, value) in &self.counters {
            out.push_str(&format!("# TYPE {name} counter\n"));
            out.push_str(&format!("{name} {value}\n"));
        }
        out
    }

    /// NÃºmero de mÃ©tricas registradas.
    pub fn len(&self) -> usize {
        self.counters.len()
    }

    /// Indica si estÃ¡ vacÃ­o.
    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
    }
}

/// Servidor HTTP mínimo de scrape Prometheus (SPEC-0197, opt-in).
#[cfg(feature = "metrics-http")]
pub struct MetricsServer {
    listener: std::net::TcpListener,
    registry: std::sync::Arc<MetricsRegistry>,
}

#[cfg(feature = "metrics-http")]
impl MetricsServer {
    /// Enlaza el servidor a `addr` (p. ej. `127.0.0.1:0`).
    ///
    /// Errors:
    ///     [`std::io::Error`] si el bind falla.
    pub fn bind(addr: &str, registry: MetricsRegistry) -> std::io::Result<Self> {
        Ok(Self {
            listener: std::net::TcpListener::bind(addr)?,
            registry: std::sync::Arc::new(registry),
        })
    }

    /// Dirección local efectiva (útil con puerto 0 en tests).
    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    /// Atiende conexiones hasta que `shutdown` sea `true`.
    ///
    /// Errors:
    ///     [`std::io::Error`] si el accept falla de forma no recuperable.
    pub fn serve_until(
        self,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::io::Result<()> {
        self.listener.set_nonblocking(true)?;
        while !shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let registry = std::sync::Arc::clone(&self.registry);
                    std::thread::spawn(move || {
                        let _ = handle_client(stream, &registry);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

/// Responde una petición HTTP de scrape (SPEC-0197).
#[cfg(feature = "metrics-http")]
fn handle_client(
    mut stream: std::net::TcpStream,
    registry: &MetricsRegistry,
) -> std::io::Result<()> {
    use std::io::{Read, Write};
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buffer = [0u8; 1024];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let (status, body) = if path == "/metrics" {
        ("200 OK", registry.export())
    } else {
        ("404 Not Found", "not found\n".to_string())
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0119-01
    fn test_ac_0119_01_metrics_export() {
        let mut reg = MetricsRegistry::new();
        reg.inc("queries_total");
        reg.inc("queries_total");
        reg.inc_by("errors_total", 3);
        let exported = reg.export();
        assert!(exported.contains("queries_total 2"));
        assert!(exported.contains("errors_total 3"));
        assert_eq!(reg.get("queries_total"), 2);
        assert_eq!(reg.len(), 2);
    }

    #[cfg(feature = "metrics-http")]
    fn start_server() -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let mut registry = MetricsRegistry::new();
        registry.inc("queries_total");
        registry.inc("queries_total");
        let server = MetricsServer::bind("127.0.0.1:0", registry).expect("bind");
        let addr = server.local_addr().expect("addr");
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = std::sync::Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || server.serve_until(signal).expect("serve"));
        (addr, shutdown, handle)
    }

    #[cfg(feature = "metrics-http")]
    fn request(addr: std::net::SocketAddr, path: &str) -> String {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        response
    }

    /// AC-0197-01 — GET /metrics responde 200 y el texto Prometheus.
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0197-01
    fn test_ac_0197_01_metrics_http_ok() {
        let (addr, shutdown, handle) = start_server();
        let response = request(addr, "/metrics");
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Type: text/plain; version=0.0.4"));
        assert!(response.contains("queries_total 2"));
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join");
    }

    /// AC-0197-02 — una ruta distinta responde 404.
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0197-02
    fn test_ac_0197_02_metrics_404() {
        let (addr, shutdown, handle) = start_server();
        let response = request(addr, "/health");
        assert!(response.starts_with("HTTP/1.1 404 Not Found"));
        assert!(!response.contains("queries_total"));
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join");
    }

    /// AC-0197-03 — shutdown termina el accept loop y libera el puerto.
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0197-03
    fn test_ac_0197_03_metrics_shutdown() {
        let (addr, shutdown, handle) = start_server();
        std::thread::sleep(std::time::Duration::from_millis(20));
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join");
        assert!(std::net::TcpListener::bind(addr).is_ok(), "puerto liberado");
    }
}
