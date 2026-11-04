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
            let name = sanitize_metric_name(name);
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

/// Sanea un nombre de métrica al charset legacy de Prometheus.
///
/// Args:
///     name: Nombre original (potencialmente con caracteres inválidos).
///
/// Returns:
///     Nombre con `[a-zA-Z_:][a-zA-Z0-9_:]*`; los caracteres inválidos se
///     sustituyen por `_` y un nombre vacío pasa a `_`.
fn sanitize_metric_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (index, character) in name.chars().enumerate() {
        let valid = character.is_ascii_alphabetic()
            || character == '_'
            || character == ':'
            || (index > 0 && character.is_ascii_digit());
        out.push(if valid { character } else { '_' });
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// Autenticación Bearer opcional del scrape (SPEC-0202).
///
/// El token se guarda como bytes y nunca aparece en `Debug` ni en errores.
#[cfg(feature = "metrics-http")]
#[derive(Clone, Default)]
pub struct MetricsAuth {
    token: Option<Vec<u8>>,
}

#[cfg(feature = "metrics-http")]
impl MetricsAuth {
    /// Activa auth con el token dado (un token vacío no autoriza nada).
    pub fn new(token: impl AsRef<[u8]>) -> Self {
        Self {
            token: Some(token.as_ref().to_vec()),
        }
    }

    /// Lee `RUSCADB_METRICS_TOKEN` del entorno; `None` si no está definido.
    pub fn from_env() -> Option<Self> {
        std::env::var_os("RUSCADB_METRICS_TOKEN")
            .map(|value| Self::new(value.to_string_lossy().as_bytes()))
    }

    /// Indica si hay auth configurada.
    pub fn is_enabled(&self) -> bool {
        self.token.is_some()
    }

    /// Comprueba `Authorization: Bearer <token>` en tiempo constante.
    fn allows(&self, request: &str) -> bool {
        let Some(expected) = &self.token else {
            return true;
        };
        let Some(header) = header_value(request, "authorization") else {
            return false;
        };
        let Some((scheme, token)) = header.split_once(' ') else {
            return false;
        };
        scheme.eq_ignore_ascii_case("bearer") && constant_time_eq(token.trim().as_bytes(), expected)
    }
}

#[cfg(feature = "metrics-http")]
impl std::fmt::Debug for MetricsAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MetricsAuth")
            .field("token", &"<redacted>")
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

/// Configuración TLS del scrape (SPEC-0202).
#[cfg(feature = "metrics-http-tls")]
#[derive(Clone)]
pub struct TlsConfig {
    /// Certificado(s) PEM (cadena + intermedios).
    pub cert_pem: Vec<u8>,
    /// Clave privada PEM (PKCS#8, PKCS#1 o SEC1).
    pub key_pem: Vec<u8>,
}

#[cfg(feature = "metrics-http-tls")]
impl std::fmt::Debug for TlsConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TlsConfig")
            .field("cert_pem", &format_args!("<{} bytes>", self.cert_pem.len()))
            .field("key_pem", &"<redacted>")
            .finish()
    }
}

#[cfg(feature = "metrics-http-tls")]
impl TlsConfig {
    /// Carga certificado y clave desde dos ficheros PEM.
    ///
    /// Errors:
    ///     [`std::io::Error`] si no se pueden leer los ficheros.
    pub fn from_paths(
        cert: impl AsRef<std::path::Path>,
        key: impl AsRef<std::path::Path>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            cert_pem: std::fs::read(cert)?,
            key_pem: std::fs::read(key)?,
        })
    }
}

/// Construye la configuración rustls (falla cerrado si el PEM es inválido).
#[cfg(feature = "metrics-http-tls")]
fn build_tls_config(config: &TlsConfig) -> std::io::Result<std::sync::Arc<rustls::ServerConfig>> {
    use std::io::Cursor;

    let certs =
        rustls_pemfile::certs(&mut Cursor::new(&config.cert_pem)).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "TLS: el PEM de certificado no contiene certificados",
        ));
    }
    let key = rustls_pemfile::private_key(&mut Cursor::new(&config.key_pem))?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "TLS: el PEM no contiene clave privada",
        )
    })?;
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(std::io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(std::io::Error::other)?;
    Ok(std::sync::Arc::new(server))
}

/// Servidor HTTP mínimo de scrape Prometheus (SPEC-0197, opt-in).
#[cfg(feature = "metrics-http")]
pub struct MetricsServer {
    listener: std::net::TcpListener,
    registry: std::sync::Arc<std::sync::RwLock<MetricsRegistry>>,
    auth: Option<MetricsAuth>,
    #[cfg(feature = "metrics-http-tls")]
    tls: Option<std::sync::Arc<rustls::ServerConfig>>,
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
        Self::bind_shared_with_auth(addr, registry, None)
    }

    /// Enlaza con registry compartido y auth Bearer opcional.
    ///
    /// Errors:
    ///     [`std::io::Error`] si el bind falla.
    pub fn bind_shared_with_auth(
        addr: &str,
        registry: std::sync::Arc<std::sync::RwLock<MetricsRegistry>>,
        auth: Option<MetricsAuth>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            listener: std::net::TcpListener::bind(addr)?,
            registry,
            auth,
            #[cfg(feature = "metrics-http-tls")]
            tls: None,
        })
    }

    /// Enlaza con TLS (y auth opcional). Falla cerrado si el PEM no es válido.
    ///
    /// Errors:
    ///     [`std::io::Error`] si el bind o la configuración TLS fallan.
    #[cfg(feature = "metrics-http-tls")]
    pub fn bind_tls(
        addr: &str,
        registry: MetricsRegistry,
        auth: Option<MetricsAuth>,
        tls: TlsConfig,
    ) -> std::io::Result<Self> {
        let config = build_tls_config(&tls)?;
        Ok(Self {
            listener: std::net::TcpListener::bind(addr)?,
            registry: std::sync::Arc::new(std::sync::RwLock::new(registry)),
            auth,
            tls: Some(config),
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
                    let auth = self.auth.clone();
                    #[cfg(feature = "metrics-http-tls")]
                    let tls = self.tls.clone();
                    let permit = InFlight(std::sync::Arc::clone(&in_flight));
                    std::thread::spawn(move || {
                        #[cfg(feature = "metrics-http-tls")]
                        if let Some(config) = tls {
                            let _ = handle_tls(stream, &registry, auth.as_ref(), config);
                            drop(permit);
                            return;
                        }
                        let _ = handle_client(stream, &registry, auth.as_ref());
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

/// Extrae una cabecera HTTP (case-insensitive) del request.
#[cfg(feature = "metrics-http")]
fn header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// Compara dos tokens sin cortar por el primer byte distinto.
#[cfg(feature = "metrics-http")]
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..max {
        let first = left.get(index).copied().unwrap_or(0);
        let second = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(first ^ second);
    }
    difference == 0
}

/// Lee la petición HTTP hasta `\r\n\r\n` (o hasta el tope de 8 KiB).
#[cfg(feature = "metrics-http")]
fn read_request<R: std::io::Read>(stream: &mut R) -> std::io::Result<String> {
    const MAX_REQUEST: usize = 8 * 1024;
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n")
                    || buffer.len() >= MAX_REQUEST
                {
                    break;
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// Responde un request ya leído (plain o TLS comparten esta lógica).
#[cfg(feature = "metrics-http")]
fn serve_io<R: std::io::Read + std::io::Write>(
    stream: &mut R,
    registry: &std::sync::RwLock<MetricsRegistry>,
    auth: Option<&MetricsAuth>,
) -> std::io::Result<()> {
    let request = read_request(stream)?;
    let mut parts = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    let head = method == "HEAD";
    let authorized = auth.is_none_or(|auth| auth.allows(&request));
    let (status, body, extra) = match (method, path) {
        ("GET" | "HEAD", "/metrics") if !authorized => (
            "401 Unauthorized",
            "unauthorized\n".to_string(),
            "WWW-Authenticate: Bearer\r\n",
        ),
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

/// Prepara y atiende una conexión en claro.
#[cfg(feature = "metrics-http")]
fn handle_client(
    mut stream: std::net::TcpStream,
    registry: &std::sync::RwLock<MetricsRegistry>,
    auth: Option<&MetricsAuth>,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    serve_io(&mut stream, registry, auth)
}

/// Prepara y atiende una conexión TLS (handshake perezoso en la primera E/S).
#[cfg(feature = "metrics-http-tls")]
fn handle_tls(
    stream: std::net::TcpStream,
    registry: &std::sync::RwLock<MetricsRegistry>,
    auth: Option<&MetricsAuth>,
    config: std::sync::Arc<rustls::ServerConfig>,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let connection = rustls::ServerConnection::new(config).map_err(std::io::Error::other)?;
    let mut tls = rustls::StreamOwned::new(connection, stream);
    let result = serve_io(&mut tls, registry, auth);
    tls.conn.send_close_notify();
    let _ = std::io::Write::flush(&mut tls);
    result
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

    #[cfg(feature = "metrics-http")]
    fn start_server_with_auth(
        token: &str,
    ) -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let mut registry = MetricsRegistry::new();
        registry.inc("queries_total");
        registry.inc("queries_total");
        let server = MetricsServer::bind_shared_with_auth(
            "127.0.0.1:0",
            std::sync::Arc::new(std::sync::RwLock::new(registry)),
            Some(MetricsAuth::new(token)),
        )
        .expect("bind");
        let addr = server.local_addr().expect("addr");
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = std::sync::Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || server.serve_until(signal).expect("serve"));
        (addr, shutdown, handle)
    }

    #[cfg(feature = "metrics-http")]
    fn request_auth(addr: std::net::SocketAddr, path: &str, token: Option<&str>) -> String {
        use std::io::{Read, Write};
        let mut request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n");
        if let Some(token) = token {
            request.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        request.push_str("\r\n");
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        stream.write_all(request.as_bytes()).expect("write");
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

    /// AC-0197-06 — una request partida en dos paquetes TCP se ensambla y
    /// responde 200 (no 404 por lectura truncada).
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0197-06
    fn test_ac_0197_06_metrics_incremental_request() {
        use std::io::{Read, Write};
        let (addr, shutdown, handle) = start_server();
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        let request = "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let split = request.len() / 2;
        stream
            .write_all(&request.as_bytes()[..split])
            .expect("write1");
        std::thread::sleep(std::time::Duration::from_millis(20));
        stream
            .write_all(&request.as_bytes()[split..])
            .expect("write2");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains("queries_total 2"), "{response}");
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join");
    }

    /// AC-0197-07 — los nombres inválidos se sanean: no se pueden inyectar
    /// líneas en la exposición Prometheus.
    #[test]
    // @spec AC-0197-07
    fn test_ac_0197_07_metrics_name_sanitized() {
        let mut reg = MetricsRegistry::new();
        reg.inc("bad name\n# TYPE injected");
        let exported = reg.export();
        assert!(exported.contains("bad_name___TYPE_injected"), "{exported}");
        assert_eq!(exported.lines().count(), 2, "{exported}");
        assert!(!exported.contains("\n# TYPE injected"), "{exported}");
    }

    /// AC-0202-01 — sin token / token erróneo responden 401 sin métricas; el
    /// token correcto responde 200.
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0202-01
    fn test_ac_0202_01_metrics_bearer_auth() {
        let (addr, shutdown, handle) = start_server_with_auth("s3cret");
        for token in [None, Some("nope")] {
            let response = request_auth(addr, "/metrics", token);
            assert!(
                response.starts_with("HTTP/1.1 401 Unauthorized"),
                "{response}"
            );
            assert!(response.contains("WWW-Authenticate: Bearer"), "{response}");
            assert!(!response.contains("queries_total"), "{response}");
        }
        let response = request_auth(addr, "/metrics", Some("s3cret"));
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains("queries_total 2"), "{response}");
        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join");
    }

    /// AC-0202-02 — comparación de token sin cortes tempranos.
    #[cfg(feature = "metrics-http")]
    #[test]
    // @spec AC-0202-02
    fn test_ac_0202_02_constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }
}
