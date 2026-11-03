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

    /// Incrementa un contador (satura en `u64::MAX`, no hace panic).
    pub fn inc(&mut self, name: &str) {
        let counter = self.counters.entry(name.to_string()).or_insert(0);
        *counter = counter.saturating_add(1);
    }

    /// Incrementa un contador por un valor (satura en `u64::MAX`).
    pub fn inc_by(&mut self, name: &str, value: u64) {
        let counter = self.counters.entry(name.to_string()).or_insert(0);
        *counter = counter.saturating_add(value);
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
    registry: std::sync::Arc<std::sync::RwLock<MetricsRegistry>>,
}

#[cfg(feature = "metrics-http")]
impl MetricsServer {
    /// Enlaza el servidor a `addr` (p. ej. `127.0.0.1:0`).
    ///
    /// Errors:
    ///     [`std::io::Error`] si el bind falla.
    pub fn bind(addr: &str, registry: MetricsRegistry) -> std::io::Result<Self> {
        Self::bind_shared(addr, std::sync::Arc::new(std::sync::RwLock::new(registry)))
    }

    /// Enlaza con un registry compartido (scrapes ven incrementos posteriores).
    ///
    /// Args:
    ///     addr: Dirección de bind (`127.0.0.1:0` en tests).
    ///     registry: Registry vivo compartido con el motor.
    ///
    /// Errors:
    ///     [`std::io::Error`] si el bind falla.
    pub fn bind_shared(
        addr: &str,
        registry: std::sync::Arc<std::sync::RwLock<MetricsRegistry>>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            listener: std::net::TcpListener::bind(addr)?,
            registry,
        })
    }

    /// Dirección local efectiva (útil con puerto 0 en tests).
    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    /// Atiende conexiones hasta que `shutdown` sea `true`.
    ///
    /// La concurrencia está acotada: como máximo `MAX_IN_FLIGHT` handlers
    /// simultáneos; el exceso recibe `503 Service Unavailable` sin crear hilos.
    ///
    /// Errors:
    ///     [`std::io::Error`] si el accept falla de forma no recuperable.
    pub fn serve_until(
        self,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::io::Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let in_flight = std::sync::Arc::new(AtomicUsize::new(0));
        self.listener.set_nonblocking(true)?;
        while !shutdown.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let current = in_flight.fetch_add(1, Ordering::Relaxed) + 1;
                    if current > MAX_IN_FLIGHT {
                        in_flight.fetch_sub(1, Ordering::Relaxed);
                        let _ = respond_overloaded(stream);
                        continue;
                    }
                    let registry = std::sync::Arc::clone(&self.registry);
                    let permit = InFlight(std::sync::Arc::clone(&in_flight));
                    std::thread::spawn(move || {
                        let _ = handle_client(stream, &registry);
                        drop(permit);
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

/// Máximo de handlers HTTP simultáneos (anti-slowloris).
#[cfg(feature = "metrics-http")]
const MAX_IN_FLIGHT: usize = 8;

/// Permiso RAII que decrementa el contador de handlers al soltarse.
#[cfg(feature = "metrics-http")]
struct InFlight(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[cfg(feature = "metrics-http")]
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Responde `503` cuando se supera el límite de concurrencia.
#[cfg(feature = "metrics-http")]
fn respond_overloaded(mut stream: std::net::TcpStream) -> std::io::Result<()> {
    use std::io::Write;
    let body = "overloaded\n";
    let response = format!(
        "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

/// Responde una petición HTTP de scrape (SPEC-0197).
#[cfg(feature = "metrics-http")]
fn handle_client(
    mut stream: std::net::TcpStream,
    registry: &std::sync::RwLock<MetricsRegistry>,
) -> std::io::Result<()> {
    use std::io::{Read, Write};
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buffer = [0u8; 1024];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut parts = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    let head = method == "HEAD";
    let (status, body, extra) = match (method, path) {
        ("GET" | "HEAD", "/metrics") => {
            let body = registry
                .read()
                .map(|registry| registry.export())
                .unwrap_or_else(|poison| poison.into_inner().export());
            ("200 OK", body, "")
        }
        ("GET" | "HEAD", _) => ("404 Not Found", "not found\n".to_string(), ""),
        _ => (
            "405 Method Not Allowed",
            "method not allowed\n".to_string(),
            "Allow: GET, HEAD\r\n",
        ),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\n{extra}Content-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    if !head {
        stream.write_all(body.as_bytes())?;
    }
    Ok(())
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

    /// Los contadores saturan: un proceso de larga vida no puede romper el
    /// endpoint de métricas por overflow.
    #[test]
    fn metrics_counters_saturate() {
        let mut reg = MetricsRegistry::new();
        reg.inc_by("hits", u64::MAX);
        reg.inc("hits");
        assert_eq!(reg.get("hits"), u64::MAX);
        reg.inc_by("hits", 5);
        assert_eq!(reg.get("hits"), u64::MAX);
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
    fn request_with(addr: std::net::SocketAddr, method: &str, path: &str) -> String {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream
            .write_all(format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        response
    }

    #[cfg(feature = "metrics-http")]
    fn request(addr: std::net::SocketAddr, path: &str) -> String {
        request_with(addr, "GET", path)
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

    /// AC-0197-04 — un método distinto de GET/HEAD responde 405 y no filtra
    /// métricas.
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0197-04
    fn test_ac_0197_04_metrics_method_not_allowed() {
        let (addr, shutdown, handle) = start_server();
        let response = request_with(addr, "POST", "/metrics");
        assert!(
            response.starts_with("HTTP/1.1 405 Method Not Allowed"),
            "{response}"
        );
        assert!(response.contains("Allow: GET, HEAD"), "{response}");
        assert!(!response.contains("queries_total"), "{response}");
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join");
    }

    /// AC-0197-05 — el registry compartido refleja incrementos posteriores al
    /// bind (scrape vivo, no una foto congelada).
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0197-05
    fn test_ac_0197_05_metrics_live_registry() {
        let registry = std::sync::Arc::new(std::sync::RwLock::new(MetricsRegistry::new()));
        let server = MetricsServer::bind_shared("127.0.0.1:0", std::sync::Arc::clone(&registry))
            .expect("bind");
        let addr = server.local_addr().expect("addr");
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = std::sync::Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || server.serve_until(signal).expect("serve"));

        let first = request(addr, "/metrics");
        assert!(!first.contains("queries_total"), "{first}");

        registry.write().expect("write").inc("queries_total");
        registry.write().expect("write").inc("queries_total");
        let second = request(addr, "/metrics");
        assert!(second.contains("queries_total 2"), "{second}");

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join");
    }
}
