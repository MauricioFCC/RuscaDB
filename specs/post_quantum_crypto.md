---
id: SPEC-0180
feature: post_quantum_crypto
status: implemented
owner: security-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-crypto]
  files: [crates/ruscadb-crypto/src/lib.rs, crates/ruscadb-crypto/src/keystore.rs]
  out_of_scope: [full TLS, HSM hardware]
fr:
  - { id: FR-0180-01, desc: "ML-KEM (Kyber) key encapsulation (FIPS 203)" }
  - { id: FR-0180-02, desc: "hybrid ECDH + ML-KEM para transicion" }
  - { id: FR-0180-03, desc: "integracion con keystore KMS-lite (SPEC-0088)" }
nf:
  - { id: NF-0180-01, desc: "cero dependencias externas en build por defecto (implementacion pure-Rust)" }
acceptance_criteria:
  - id: AC-0180-01
    given: dos partes
    when: ejecutan ML-KEM encapsulate/decapsulate
    then: comparten el mismo secreto compartido
    test: test_ac_0180_01_shared_secret
  - id: AC-0180-02
    given: un ciphertext alterado
    when: se decapsula
    then: se devuelve error (no secreto incorrecto silencioso)
    test: test_ac_0180_02_tamper
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-crypto --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0180 — Criptografia post-cuantica (ML-KEM / Kyber)

## Contexto

RuscaDB usa XChaCha20-Poly1305 + Argon2id (SPEC-0013/0088). Con la
amenaza cuantica, se anade ML-KEM (FIPS 203) para key encapsulation.

## Superficie

- `crypto/lib.rs`: `MlKem { keygen, encapsulate, decapsulate }`.
- `keystore.rs`: almacenamiento de claves ML-KEM.

## Estado del arte

- **NIST FIPS 203** (agosto 2024): ML-KEM (Kyber).
- **NIST FIPS 204/205**: ML-DSA, SLH-DSA.
- **RuscaDB actual**: XChaCha20 + Argon2id (SPEC-0013).

## Diseño propuesto

1. **Pure-Rust ML-KEM**: implementacion de Kyber-768 (parametros estandar).
2. **Hybrid**: ECDH (X25519) + ML-KEM para transicion.
3. **Feature flag**: `post-quantum` (off por defecto).

## Riesgos

- Complejidad: implementacion criptografica requiere auditoria.
- Performance: Kyber-768 es rapido (~50us).

## ADR requerido

`docs/adr/ADR-0180-post-quantum.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\post_quantum_crypto.md