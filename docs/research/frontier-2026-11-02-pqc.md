# Frontier research — ML-KEM FIPS 203 e híbrido X25519 (2026-11-02)

> Tarea explícita: cerrar SPEC-0180 (criptografía post-cuántica) sin simular
> seguridad. Paper trail con URLs leídas; lo no verificado se marca
> `[UNVERIFIED]`.

## 1. Estado del arte (fuentes leídas)

| Fuente | Aporte | URL |
|---|---|---|
| NIST FIPS 203 (final, ago-2024) | ML-KEM (antes Kyber): parámetros 512/768/1024, encapsulate/decapsulate, implicit rejection. | https://csrc.nist.gov/pubs/fips/203/final |
| NIST PQC | Proceso de estandarización; FIPS 204/205 (ML-DSA/SLH-DSA) fuera de alcance. | https://csrc.nist.gov/projects/post-quantum-cryptography |
| libcrux-ml-kem 0.0.11 | Implementación pure-Rust **formalmente verificada** (hax/F*); API `generate_key_pair`, `encapsulate`, `decapsulate`, validación de claves. | https://github.com/cryspen/libcrux |
| RustCrypto `ml-kem` 0.3.4 | Pure-Rust, pero el README advierte explícitamente: **"never independently audited"**. | https://github.com/RustCrypto/KEMs/tree/master/ml-kem |
| ACVP-Server — ML-KEM encapDecap | Vectores oficiales usados como KAT (ML-KEM-768, encapsulación y decapsulación). | https://github.com/usnistgov/ACVP-Server/tree/master/gen-val/json-files/ML-KEM-encapDecap-FIPS203 |

## 2. Hipótesis comparadas

Criterios: **A** exactitud (FIPS 203), **S** seguridad/verificación, **L**
latencia, **D** dependencias, **R** reproducibilidad.

| # | Hipótesis | A | S | L | D | R | Veredicto |
|---|---|---|---|---|---|---|---|
| H1 | Implementar ML-KEM a mano | Media (riesgo de bugs) | Baja | Alta | 0 | Media | Descartada: criptografía propia no auditada. |
| H2 | RustCrypto `ml-kem` | Alta | Media (sin auditoría externa) | Alta | 1 crate | Alta | Descartada como primaria por la advertencia explícita de no auditoría. |
| H3 | `libcrux-ml-kem` (hax/F*) + X25519 | Alta | Alta (verificación formal) | Alta | 3 crates opcionales | Alta (KAT ACVP) | **Elegida**. |
| H4 | `pqcrypto-kyber` (PQClean, C) | Alta | Media | Alta | FFI C | Media | Descartada: el proyecto evita motores C y no hay auditoría propia. |

## 3. Gap analysis

- RuscaDB solo tenía XChaCha20-Poly1305 + Argon2id (simétrico).
- El placeholder `MlKemStub` devolvía bytes dummy: no era FIPS 203.
- No había híbrido ni persistencia de claves PQC en el keystore.
- **Gap**: KEM post-cuántico real, verificable con KAT, con transición híbrida
  y sin dependencias en el build por defecto.

## 4. Decisión

1. ML-KEM-768 de `libcrux-ml-kem` (verificado formalmente) tras feature
   `post-quantum` (OFF por defecto).
2. Híbrido `ss = SHA-256(ml_kem_ss || x25519_ss)`; ciphertext
   `ml_kem_ct || x25519_ephemeral_pk`.
3. Keystore KMS-lite: mapa `pq_keys` retrocompatible (`#[serde(default)]`).
4. KAT oficial ACVP/FIPS 203 en tests; `encapsulate_deterministic` para
   reproducibilidad.

## 5. Lo no aplicado y por qué

- **Auditoría externa**: no hay proveedor/credenciales en esta sesión; queda
  como riesgo residual y la feature sigue OFF por defecto.
- **ML-DSA/SLH-DSA**: fuera de alcance (firma, no KEM).
- **TLS/HSM**: fuera de alcance; el keystore es local.
- **ACVP end-to-end en CI**: el KAT puntual está en tests; un harness ACVP
  completo requiere descargar vectores en CI (no se añadió por peso).
