---
id: SPEC-0011
feature: crypto
status: accepted
owner: security-team
appetite_days: 6
boundaries:
  crates: [ruscadb-crypto, ruscadb-core]
  out_of_scope: [storage, wal, bindings]
fr:
  - { id: FR-0011-01, desc: "AEAD XChaCha20-Poly1305: seal/open con nonce de 24 bytes" }
  - { id: FR-0011-02, desc: "derivacion de clave con Argon2id (passphrase + salt)" }
  - { id: FR-0011-03, desc: "zeroize de material sensible (claves) en memoria" }
nf:
  - { id: NF-0011-01, desc: "integridad: cualquier manipulacion del ciphertext falla la verificacion" }
  - { id: NF-0011-02, desc: "sin panics ante claves/nonce/ciphertext invalidos" }
acceptance_criteria:
  - id: AC-0011-01
    given: una clave, un nonce y un mensaje
    when: se sella y se abre
    then: el mensaje recuperado es identico
    test: test_ac_0011_01_seal_open_roundtrip
  - id: AC-0011-02
    given: un ciphertext valido
    when: se altera un byte
    then: open falla (integridad AEAD)
    test: test_ac_0011_02_tamper_is_detected
  - id: AC-0011-03
    given: una clave o nonce distintos
    when: se abre un ciphertext ajeno
    then: falla la verificacion
    test: test_ac_0011_03_wrong_key_or_nonce_fails
  - id: AC-0011-04
    given: una passphrase y un salt
    when: se deriva la clave dos veces
    then: la clave es identica (determinismo)
    test: test_ac_0011_04_derive_key_is_deterministic
exit_criteria:
  - cargo test -p ruscadb-crypto -- test_ac_0011
  - cargo mutants -p ruscadb-crypto mutation score >= 70%
rollback:
  - revertir ruscadb-crypto al stub
sandbox:
  - cargo test -p ruscadb-crypto
---

# SPEC-0011 — Cifrado en reposo (hardening)

## Contexto

Cifrado opcional de RuscaDB (`docs/RuscaDB-roadmap.md` §6.5). AEAD
XChaCha20-Poly1305 + KDF Argon2id; `zeroize` para el material sensible.

## API esperada

`seal(key: &[u8;32], nonce: &[u8;24], plaintext: &[u8]) -> Vec<u8>`;
`open(key, nonce, ciphertext) -> Result<Vec<u8>, RuscaError>`;
`derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8;32], RuscaError>`.

## Trazabilidad

Tests `test_ac_0011_<nn>_*`; verificado por `cargo xtask trace`.
