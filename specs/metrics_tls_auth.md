---
id: SPEC-0202
feature: metrics_tls_auth
status: proposed
maturity: stub
owner: platform-team
appetite_days: 4
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/observability.rs
    - crates/ruscadb/Cargo.toml
    - crates/ruscadb/tests/metrics_security.rs
    - docs/operacion.md
  out_of_scope: [mTLS, OIDC, rotación en caliente de certificados, rate limiting]
fr:
  - { id: FR-0202-01, desc: "GET /metrics exige Bearer token cuando esta configurado (401 si falta o no coincide)" }
  - { id: FR-0202-02, desc: "comparacion de token en tiempo constante; el token no se registra en logs ni errores" }
  - { id: FR-0202-03, desc: "TLS opcional con rustls (cert/key PEM); sin fallback a texto plano" }
  - { id: FR-0202-04, desc: "configuracion invalida (solo cert o solo key) falla al arrancar, nunca degrada a inseguro" }
nf:
  - { id: NF-0202-01, desc: "auth O(|token|) por request; TLS handshake O(1) por conexion" }
  - { id: NF-0202-02, desc: "feature metrics-http-tls OFF por defecto; metrics-http sigue sin dependencias nuevas" }
  - { id: NF-0202-03, desc: "token desde variable de entorno o archivo con permisos; nunca en argv ni en el repo" }
acceptance_criteria:
  - id: AC-0202-01
    given: un servidor con token configurado
    when: se hace GET /metrics sin token, con token erroneo y con token correcto
    then: responde 401, 401 y 200 respectivamente; nunca expone metricas sin auth
    test: test_ac_0202_01_metrics_bearer_auth
  - id: AC-0202-02
    given: dos tokens de distinta longitud y contenido
    when: se comparan con la funcion de auth
    then: solo el igual devuelve true; la comparacion no corta por el primer byte distinto
    test: test_ac_0202_02_constant_time_compare
  - id: AC-0202-03
    given: certificado autofirmado y servidor con TLS
    when: un cliente rustls hace GET /metrics
    then: handshake y respuesta 200; una conexion en claro no obtiene respuesta HTTP valida
    test: test_ac_0202_03_metrics_tls_handshake
  - id: AC-0202-04
    given: configuracion TLS incompleta o PEM invalido
    when: se construye el servidor
    then: falla con error accionable y no abre el puerto
    test: test_ac_0202_04_tls_fail_closed
exit_criteria:
  - cargo test -p ruscadb --features metrics-http,metrics-http-tls --test metrics_security
  - cargo clippy -p ruscadb --all-targets --all-features -- -D warnings
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - desactivar metrics-http-tls y el token; el scrape vuelve a loopback sin auth
sandbox:
  - cargo test -p ruscadb --features metrics-http,metrics-http-tls --test metrics_security
---
# SPEC-0202 — TLS y autenticación del endpoint de métricas

## Contexto

`metrics-http` (SPEC-0197) expone `GET /metrics` en texto plano y sin auth;
solo es seguro en loopback. Para scrape desde otra máquina/Pod hace falta TLS y
un token. El endpoint es de solo lectura y de baja cardinalidad, así que la
solución debe ser simple: Bearer token + TLS opcional, sin meter un servidor web
completo.

## Diseño

1. **Auth (siempre disponible en `metrics-http`)**:
   - token de `RUSCADB_METRICS_TOKEN` o `MetricsSecurity::token(SecretString)`;
   - cabecera `Authorization: Bearer <token>`;
   - sin token configurado ⇒ comportamiento actual (loopback);
   - token configurado y ausente/erróneo ⇒ `401 Unauthorized` +
     `WWW-Authenticate: Bearer`, sin cuerpo de métricas;
   - comparación XOR acumulada sobre longitud máxima + comparación de longitud
     (sin early-exit); el token nunca aparece en logs/errores.
2. **TLS (feature `metrics-http-tls`)**:
   - dependencias opcionales `rustls` + `rustls-pemfile`;
   - `TlsConfig { cert_pem, key_pem }`; `MetricsServer::bind_tls(...)`;
   - TLS 1.2/1.3, ALPN `http/1.1`;
   - si falta cert o key, o el PEM no parsea ⇒ error al construir, sin bind.
3. **Orden**: TLS handshake → parseo HTTP → auth → ruta → respuesta.
4. **Operación**: documentar generación de token/cert y rotación con reinicio
   en `docs/operacion.md`; nunca commitear secretos.

## Complejidad

```text
auth:      O(|token|) por request
TLS:       O(1) handshake por conexión (amortizado)
memoria:   O(1) adicional por conexión
```

## TDD adversarial

- Matriz 401/401/200 sin filtrar métricas.
- Comparación constante: longitudes distintas, prefijo común y token correcto.
- Handshake real con certificado autofirmado (rustls client) y rechazo de claro.
- Fail-closed: cert sin key, key sin cert, PEM corrupto.
- Test de no-log: el mensaje de error no contiene el token.

## Riesgos

- Token en entorno visible para el proceso: se admite archivo con permisos y se
  documenta; rotación = reinicio.
- Certificados caducados: el handshake falla; se documenta renovación.
- TLS añade dependencias solo bajo feature; `metrics-http` puro sigue en std.

## Estado

Propuesta (2026-11-04). Sin implementación; tests scaffold `#[ignore]` en
`crates/ruscadb/tests/proposed_specs.rs`.
