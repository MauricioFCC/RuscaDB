---
id: SPEC-0013
feature: encrypted_at_rest
status: accepted
owner: security-team
appetite_days: 8
boundaries:
  crates: [ruscadb-wal, ruscadb-multimodal, ruscadb-crypto, ruscadb]
  out_of_scope: [cifrado de paginas del heap, rotacion de claves, KMS/HSM]
fr:
  - { id: FR-0013-01, desc: "EncryptionConfig con clave de 32 B (directa o derivada por Argon2id) y zeroize" }
  - { id: FR-0013-02, desc: "WAL cifrado: envelope versionado [0x01 | nonce 24 B | seal(payload)] por frame" }
  - { id: FR-0013-03, desc: "nonce determinista del WAL derivado del LSN (unicidad por monotonia, sin RNG)" }
  - { id: FR-0013-04, desc: "blob store cifrado: envelope con nonce derivado del hash de contenido" }
  - { id: FR-0013-05, desc: "compatibilidad: sin clave se opera en claro; clave erronea/ausente ante datos cifrados es error" }
nf:
  - { id: NF-0013-01, desc: "tamper en frame o blob se detecta (CRC/AEAD) sin panics" }
  - { id: NF-0013-02, desc: "el ciphertext no expone el claro; mismo contenido deduplica igual que en claro" }
acceptance_criteria:
  - id: AC-0013-01
    given: una clave de 32 B
    when: se abre un WAL cifrado, se escriben commits y se reabre con la misma clave
    then: el replay recupera los payloads exactos
    test: test_ac_0013_01_encrypted_wal_roundtrip
  - id: AC-0013-02
    given: un WAL cifrado
    when: se abre sin clave o con clave erronea
    then: el recovery falla con error accionable (sin claro expuesto)
    test: test_ac_0013_02_wrong_or_missing_key_fails
  - id: AC-0013-03
    given: un blob store cifrado
    when: se hace put/get/get_range y se reabre con la misma clave
    then: los bytes recuperados son exactos y el ref-count funciona
    test: test_ac_0013_03_encrypted_blob_roundtrip
  - id: AC-0013-04
    given: un frame del WAL o un blob cifrado manipulado en disco
    when: se lee o se recupera
    then: se detecta la manipulacion sin panics
    test: test_ac_0013_04_tamper_is_detected
  - id: AC-0013-05
    given: una Database con encryption configurada
    when: se opera (create/insert/commit/reopen) con la misma clave
    then: el end-to-end funciona y en disco no hay claro
    test: test_ac_0013_05_encrypted_database_end_to_end
exit_criteria:
  - cargo test -p ruscadb-wal -p ruscadb-multimodal -p ruscadb -- test_ac_0013
  - cargo mutants -p ruscadb-wal -p ruscadb-multimodal mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir Wal/blob cifrados; la fachada conserva el modo claro
sandbox:
  - cargo test -p ruscadb-wal -p ruscadb-multimodal
---

# SPEC-0013 — Cifrado en reposo aplicado (WAL + blob store)

## Contexto

Aplica SPEC-0011 al dato durable (`docs/RuscaDB-roadmap.md` §6.5, canon
OWASP/STRIDE, patrón SQLCipher de cifrado por página con IV+HMAC — aquí en
versión AEAD moderna: el tag Poly1305 sustituye al HMAC separado):

- `EncryptionConfig { key: Zeroizing<[u8; 32]> }` en `DbConfig.encryption`
  (`Option`); constructor `from_passphrase(passphrase, salt)` vía
  `ruscadb-crypto::derive_key` (Argon2id).
- **WAL** (`ruscadb-wal`): `Wal::open_encrypted(path, key)`; el formato del
  frame no cambia, el **payload** se envuelve en
  `[0x01 | nonce 24 B | seal(payload)]` con nonce determinista
  `lsn.to_le_bytes() + [0; 16]` (unicidad por monotonía del LSN, sin
  dependencia RNG); `read_records`/`recover` aceptan clave opcional; sin
  clave ante payload versionado → `WalCorrupt`; con clave errónea el AEAD
  falla → `WalCorrupt` (el CRC del frame puede fallar antes: defensa en
  profundidad, el error es el mismo).
- **Blob store** (`ruscadb-multimodal`): `BlobStore::open_encrypted(root, key)`;
  cada blob se guarda como `[0x01 | nonce 24 B | seal(bytes)]` con nonce =
  primeros 24 B del SHA-256 del contenido (determinista, preserva la
  deduplicación CAS; documentado que igualdad de contenido ⇒ igualdad de
  ciphertext); `get`/`get_range` descifran; abrir cifrado en claro (o al
  revés) es error, no basura silenciosa.
- **Fachada**: `Database::open` propaga `DbConfig.encryption` al WAL; el
  heap hereda el cifrado porque viaja dentro de los payloads del WAL
  (documentar esta propiedad + su límite: las páginas del `.data` en claro
  hasta el próximo commit que las reescriba; el WAL es la fuente de verdad).
- **Arquitectura**: `ruscadb-crypto` es hoja pura (sin E/S), misma categoría
  de confianza que `ruscadb-core`; se añaden las aristas
  `wal → crypto`, `multimodal → crypto`, `fachada → crypto` en
  `scripts/check_architecture.py` (ya aplicado en el setup).

## Criterios de aceptación

- **AC-0013-01/03** — roundtrips cifrados de WAL y blob con reopen.
- **AC-0013-02** — clave errónea/ausente = error accionable, 0 claro expuesto.
- **AC-0013-04** — tamper detectado sin panics.
- **AC-0013-05** — end-to-end cifrado en `Database` sin claro en disco.

## Trazabilidad

Tests `test_ac_0013_<nn>_*` en `ruscadb-wal`, `ruscadb-multimodal` y
`ruscadb` + proptest de roundtrip; verificado por `cargo xtask trace`.
