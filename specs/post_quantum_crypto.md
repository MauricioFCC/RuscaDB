---
id: SPEC-0180
feature: post_quantum_crypto
status: implemented
maturity: real
owner: security-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-crypto]
  files:
    - crates/ruscadb-crypto/src/ml_kem.rs
    - crates/ruscadb-crypto/src/keystore.rs
    - crates/ruscadb-crypto/tests/post_quantum.rs
    - crates/ruscadb-crypto/tests/fixtures/mlkem768_kat.json
    - crates/ruscadb/src/post_quantum.rs
    - crates/ruscadb/tests/post_quantum.rs
  out_of_scope: [TLS completo, HSM hardware, ML-DSA/SLH-DSA, auditoria externa]
fr:
  - { id: FR-0180-01, desc: "ML-KEM (Kyber) key encapsulation (FIPS 203)" }
  - { id: FR-0180-02, desc: "hybrid ECDH + ML-KEM para transicion" }
  - { id: FR-0180-03, desc: "integracion con keystore KMS-lite (SPEC-0088)" }
nf:
  - { id: NF-0180-01, desc: "cero dependencias externas en build por defecto (feature off)" }
  - { id: NF-0180-02, desc: "implementacion pure-Rust formalmente verificada + KAT ACVP" }
acceptance_criteria:
  - id: AC-0180-01
    given: dos partes
    when: ejecutan ML-KEM encapsulate/decapsulate
    then: comparten el mismo secreto compartido de 32 bytes
    test: test_ac_0180_01_shared_secret
  - id: AC-0180-02
    given: un ciphertext alterado
    when: se decapsula
    then: FIPS 203 implicit rejection devuelve un secreto distinto (no silencioso)
    test: test_ac_0180_02_tamper
  - id: AC-0180-03
    given: un vector ACVP/FIPS 203 de ML-KEM-768
    when: se encapsula/decapsula de forma determinista
    then: ciphertext y secreto coinciden con el vector
    test: test_ac_0180_03_acvp_kat
  - id: AC-0180-04
    given: dos partes con claves hibridas X25519+ML-KEM
    when: encapsulan/decapsulan
    then: comparten el mismo secreto y un tamper X25519 lo cambia
    test: test_ac_0180_04_hybrid_shared_secret
  - id: AC-0180-05
    given: un keystore KMS-lite con un par ML-KEM
    when: se guarda, se recarga y se lee
    then: el par recuperado es identico
    test: test_ac_0180_05_keystore_roundtrip
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-crypto --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - desactivar la feature `post-quantum` (vuelve al build sin deps PQC); el keystore ignora `pq_keys` y los datos simetricos siguen intactos
sandbox:
  - cargo test -p ruscadb-crypto --all-features --test post_quantum
  - cargo test -p ruscadb --features post-quantum --test post_quantum
---

# SPEC-0180 — Criptografía post-cuántica (ML-KEM / Kyber)

## Contexto

RuscaDB usa XChaCha20-Poly1305 + Argon2id (SPEC-0013/0088). Con la amenaza
cuántica, se añade ML-KEM (FIPS 203) para key encapsulation y un modo híbrido
X25519 + ML-KEM para la transición.

## Superficie

- `ruscadb-crypto::ml_kem`: `MlKem` (`keygen`, `encapsulate`, `decapsulate`,
  variantes deterministas para KAT) y `HybridKem` (X25519 + ML-KEM-768).
- `ruscadb-crypto::keystore`: `store_ml_kem_keypair` / `load_ml_kem_keypair`
  sobre un mapa `pq_keys` retrocompatible (`#[serde(default)]`).
- `ruscadb::post_quantum`: re-export de la fachada.
- Feature `post-quantum` **OFF por defecto**.

## Estado del arte

- **NIST FIPS 203** (agosto 2024): ML-KEM (Kyber). Tamaños ML-KEM-768:
  `ek=1184`, `dk=2400`, `ct=1088`, `ss=32`.
- **NIST FIPS 204/205**: ML-DSA, SLH-DSA (fuera de alcance).
- **libcrux-ml-kem 0.0.11**: implementación pure-Rust **formalmente verificada**
  con hax/F* (aritmética, NTT, serialización y algoritmos de alto nivel).
- **RustCrypto `ml-kem` 0.3.4**: pure-Rust, pero su README advierte que **nunca
  ha sido auditado independientemente**; se descartó como dependencia primaria.
- **ACVP/FIPS 203**: vectores oficiales usados como KAT (AC-0180-03).

## Diseño

1. **ML-KEM-768**: `libcrux_ml_kem::mlkem768` con `generate_key_pair`,
   `encapsulate`, `decapsulate`; validación de claves FIPS 203 §7.2/§7.3.
2. **Híbrido**: `ss = SHA-256(ml_kem_ss || x25519_ss)`; el ciphertext es
   `ml_kem_ct || x25519_ephemeral_pk`. X25519 vía `x25519-dalek` 3.0.
3. **Keystore**: `pq_keys: BTreeMap<kid, {public_key, private_key, status}>`
   en el JSON versionado; las claves se guardan en claro (KMS-lite, mismo
   modelo de amenazas que SPEC-0088).
4. **Feature flag**: `post-quantum` activa las deps opcionales; el build por
   defecto sigue sin dependencias PQC.

## Complejidad

```text
keygen ML-KEM-768:        O(k² log q) ≈ coste del NTT (μs)
encapsulate/decapsulate:  O(k² log q + |m|)
híbrido:                  + O(32) SHA-256 + X25519
keystore:                 O(log K) lookup + O(|clave|) hex
```

## Estado de seguridad

- Implementación **formalmente verificada** (hax/F*) y contrastada con KAT
  ACVP/FIPS 203.
- **No** ha pasado una auditoría externa independiente. Por eso la feature
  está OFF por defecto y el estado `real` se refiere a funcionalidad
  verificable, no a una certificación de seguridad.
- Próximo paso recomendado: auditoría externa + ACVP end-to-end en CI.

## Referencias

- NIST FIPS 203: https://csrc.nist.gov/pubs/fips/203/final
- NIST PQC: https://csrc.nist.gov/projects/post-quantum-cryptography
- libcrux-ml-kem: https://github.com/cryspen/libcrux
- RustCrypto ml-kem (advertencia de auditoría): https://github.com/RustCrypto/KEMs/tree/master/ml-kem
- ACVP-Server ML-KEM: https://github.com/usnistgov/ACVP-Server/tree/master/gen-val/json-files/ML-KEM-encapDecap-FIPS203

## ADR

`docs/adr/ADR-0180-post-quantum.md` (local-only, no se pushea).
