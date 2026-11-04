//! SPEC-0202 — TLS y auth del endpoint de métricas.
//!
//! Los tests TLS requieren la feature `metrics-http-tls`; el fixture es un
//! certificado autofirmado de test (`tests/fixtures/tls`, no es un secreto).

#![cfg(feature = "metrics-http-tls")]
#![allow(clippy::expect_used)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ruscadb::{MetricsAuth, MetricsRegistry, MetricsServer, TlsConfig};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

/// Ruta de un fixture TLS.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tls")
        .join(name)
}

/// Configuración de cliente que confía en el certificado de test.
fn client_config() -> Arc<ClientConfig> {
    let pem = std::fs::read(fixture("cert.pem")).expect("cert pem");
    let certs: Vec<_> = rustls_pemfile::certs(&mut &pem[..])
        .collect::<Result<_, _>>()
        .expect("certs");
    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots.add(cert).expect("root cert");
    }
    Arc::new(
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// Arranca un servidor TLS en loopback.
fn start_tls_server() -> (
    std::net::SocketAddr,
    Arc<AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let mut registry = MetricsRegistry::new();
    registry.inc("queries_total");
    registry.inc("queries_total");
    let config = TlsConfig::from_paths(fixture("cert.pem"), fixture("key.pem")).expect("tls");
    let server = MetricsServer::bind_tls("127.0.0.1:0", registry, None, config).expect("bind tls");
    let addr = server.local_addr().expect("addr");
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&shutdown);
    let handle = std::thread::spawn(move || server.serve_until(signal).expect("serve"));
    (addr, shutdown, handle)
}

/// GET por TLS y devuelve la respuesta cruda.
fn https_get(addr: std::net::SocketAddr, path: &str) -> String {
    let server_name = ServerName::try_from("localhost".to_string()).expect("server name");
    let connection =
        ClientConnection::new(client_config(), server_name).expect("client connection");
    let stream = TcpStream::connect(addr).expect("connect");
    let mut tls = StreamOwned::new(connection, stream);
    tls.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .expect("write");
    let mut response = String::new();
    tls.read_to_string(&mut response).expect("read");
    response
}

/// AC-0202-03 — handshake real con certificado autofirmado y 200; una conexión
/// en claro no obtiene una respuesta HTTP válida.
#[test]
// @spec AC-0202-03
fn test_ac_0202_03_metrics_tls_handshake() {
    let (addr, shutdown, handle) = start_tls_server();
    let response = https_get(addr, "/metrics");
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("queries_total 2"), "{response}");

    let mut plain = TcpStream::connect(addr).expect("plain connect");
    plain
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .expect("timeout");
    let _ = plain.write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let mut buffer = [0u8; 256];
    let read = plain.read(&mut buffer).unwrap_or(0);
    let text = String::from_utf8_lossy(&buffer[..read]);
    assert!(
        !text.starts_with("HTTP/1.1 200"),
        "texto plano obtuvo respuesta HTTP: {text}"
    );

    shutdown.store(true, Ordering::Relaxed);
    handle.join().expect("join");
}

/// AC-0202-04 — configuración TLS incompleta o corrupta falla al arrancar y
/// no abre el puerto (fail-closed).
#[test]
// @spec AC-0202-04
fn test_ac_0202_04_tls_fail_closed() {
    let empty = TlsConfig {
        cert_pem: Vec::new(),
        key_pem: Vec::new(),
    };
    assert!(
        MetricsServer::bind_tls("127.0.0.1:0", MetricsRegistry::new(), None, empty).is_err(),
        "cert/key vacíos deben fallar"
    );

    let only_cert = TlsConfig {
        cert_pem: std::fs::read(fixture("cert.pem")).expect("cert"),
        key_pem: Vec::new(),
    };
    assert!(
        MetricsServer::bind_tls("127.0.0.1:0", MetricsRegistry::new(), None, only_cert).is_err(),
        "cert sin key debe fallar"
    );

    let corrupt = TlsConfig {
        cert_pem: b"no es pem".to_vec(),
        key_pem: b"no es pem".to_vec(),
    };
    assert!(
        MetricsServer::bind_tls("127.0.0.1:0", MetricsRegistry::new(), None, corrupt).is_err(),
        "PEM corrupto debe fallar"
    );
}

/// `MetricsAuth` no filtra el token en `Debug`.
#[test]
fn metrics_auth_debug_redacts_token() {
    let auth = MetricsAuth::new("super-secret");
    let rendered = format!("{auth:?}");
    assert!(!rendered.contains("super-secret"), "{rendered}");
    assert!(rendered.contains("redacted"), "{rendered}");
}
