---
id: SPEC-0088
feature: p2_hardening
status: implemented
owner: security-team
appetite_days: 3
boundaries:
  crates: [ruscadb-mcp, ruscadb-crypto, ruscadb]
  files: [crates/ruscadb-crypto/src/keystore.rs, crates/ruscadb-crypto/src/lib.rs, crates/ruscadb/src/lib.rs, crates/ruscadb-mcp/src/lib.rs, crates/ruscadb-mcp/tests/mcp_auth.rs, SECURITY.md]
  out_of_scope: [transport HTTP/TLS del MCP, KMS remoto o en la nube, rotación automática por tiempo (TTL), autenticación a nivel SO, FIPS, cifrado en reposo del keystore con passphrase]
fr:
  - { id: FR-0088-01, desc: "handshake `session/auth`: todos los métodos de datos (tools/* y resources/*) exigen sesión autenticada; sin ella o con token inválido → error JSON-RPC -32001" }
  - { id: FR-0088-02, desc: "token de sesión = `v1.<kid>.<nonce>.<ciphertext>`: payload sellado con XChaCha20-Poly1305 bajo la key activa del keystore (AEAD = autenticación; sin secretos ni digests en claro)" }
  - { id: FR-0088-03, desc: "keystore KMS-lite en `ruscadb-crypto`: keys versionadas (kid activo/retirado), creación con 0600 en unix, `rotate()` retira la key activa y crea la siguiente" }
  - { id: FR-0088-04, desc: "la rotación invalida las sesiones existentes (kid retirado no valida) y un token emitido con el kid nuevo autentica de nuevo" }
nf:
  - { id: NF-0088-01, desc: "seguro por defecto: `McpServer::new` y `::read_write` arrancan con sesión NO autenticada" }
  - { id: NF-0088-02, desc: "sin fugas: los mensajes de error jamás incluyen el token ni el material de key (enmascarado)" }
  - { id: NF-0088-03, desc: "sin red y sin dependencias nuevas fuera del workspace (getrandom ya está en el árbol del lockfile)" }
  - { id: NF-0088-04, desc: "estado persistido: con keystore en disco, el token sigue validando en un proceso nuevo (sin estado en memoria del servidor)" }
acceptance_criteria:
  - id: AC-0088-01
    given: servidor MCP recién creado sin autenticar
    when: llega `tools/list` (o `tools/call`) sin `session/auth`
    then: error JSON-RPC -32001 y ningún dato expuesto; tras `session/auth` con token válido los métodos funcionan
    test: test_ac_0088_01_unauthenticated_session_rejected_by_default
  - id: AC-0088-02
    given: token corrupto o con kid desconocido
    when: `session/auth`
    then: rechazo -32001 y la respuesta NO contiene el token ni el material de key
    test: test_ac_0088_02_invalid_token_rejected_without_leak
  - id: AC-0088-03
    given: sesión autenticada y funcionando
    when: se rota el keystore
    then: la sesión previa queda invalidada (-32001) y un token del kid nuevo vuelve a autenticar
    test: test_ac_0088_03_rotation_invalidates_existing_sessions
  - id: AC-0088-04
    given: keystore persistido en disco y un servidor que emite un token
    when: un servidor nuevo abre el mismo keystore
    then: el mismo token valida (estado persistido) y el fichero declara versión + kids con permisos 0600 en unix
    test: test_ac_0088_04_keystore_persists_across_reloads
exit_criteria:
  - cargo test -p ruscadb-mcp --all-features
  - cargo test -p ruscadb-crypto
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - python scripts/check_architecture.py
  - cargo xtask trace / contract verdes
rollback:
  - retirar session/auth y el keystore: el servidor vuelve al comportamiento sin auth (SPEC-0068/0075)
sandbox:
  - cargo test -p ruscadb-mcp --test mcp_auth
---

# SPEC-0088 — Hardening P2: auth de sesión MCP + KMS-lite (Ola 4)

## Contexto

El análisis de flujos (2026-10-13) y SECURITY.md: el servidor MCP
(SPEC-0068/0075) es local y sin red, pero **no autentica al cliente** —
cualquier proceso que hable por el stdio del hijo obtiene tools de
lectura/escritura. La frontera de servidores MCP (spec 2025-2026)
recomienda sesión explícita con revocación; la revocación natural aquí
es **rotación de keys** (KMS-lite: keystore local versionado, sin KMS
remoto). La auditoría clasifica esto como P2 (no bloquea producción
local single-user, sí endurece el camino a multi-proceso).

## Superficie

- `ruscadb-crypto/src/keystore.rs` — `Keystore` (load_or_create,
  issue_token, validate, rotate) sobre `seal`/`open` XChaCha20-Poly1305
  existentes; dependencia de cara a la fachada (la composición vive en
  `ruscadb`: re-export del keystore).
- `ruscadb-mcp` — método `session/auth`, guardia en tools/resources,
  token emitido por el padre vía `issue_session_token()`.
- `SECURITY.md` — modelo de amenaza local + keystore + rotación.

## Notas de diseño

- **Estado del servidor reducido a cero**: la validación es
  criptográfica (AEAD abre con la key del kid), no un registro en
  memoria — por eso el token sobrevive a un proceso nuevo (AC-04) y la
  rotación lo invalida a todos (AC-03).
- Token transmitido por el padre al cliente por el mismo canal de
  arranque (env/CLI del cliente): fuera de alcance del servidor.
- Sin `unwrap`/`expect`; errores WHAT+WHY+WHERE; comparación de tag
  delegada al AEAD (constante por diseño).
- El keystore en disco guarda keys en claro a propósito (KMS-lite):
  permisos 0600 y documento de amenazas en SECURITY.md; cifrarlo con
  passphrase queda fuera de alcance.
