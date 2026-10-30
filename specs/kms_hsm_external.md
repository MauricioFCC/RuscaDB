---
id: SPEC-0143
feature: kms_hsm_external
status: implemented
maturity: stub
owner: security-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-crypto]
  files: [crates/ruscadb/src/kms.rs]
  out_of_scope: [implementacion real de AWS KMS, HashiCorp Vault]
fr:
  - { id: FR-0143-01, desc: "feature flag `kms` (off por defecto) habilita interfaz KMS/HSM" }
  - { id: FR-0143-02, desc: "KmsProvider::wrap y ::unwrap cifran/descifran claves" }
nf:
  - { id: NF-0143-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0143-01
    given: la feature `kms` activada
    when: se envuelve y desenvuelve una clave
    then: el resultado es identico a la original
    test: test_ac_0143_01_kms_roundtrip
exit_criteria:
  - cargo test -p ruscadb --all-features --test kms
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0143 â€” KMS/HSM externo

## Contexto

Frontier 2026 (AWS KMS, HashiCorp Vault): custodia externa de claves.
RuscaDB tiene keystore KMS-lite local (SPEC-0088). Esta spec aÃ±ade una
interfaz KMS/HSM detrÃ¡s de un feature flag.

## Superficie

- Feature `kms` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/kms.rs` â€” `KmsProvider` con `wrap`/`unwrap`.
- IntegraciÃ³n futura: AWS KMS, HashiCorp Vault, Azure Key Vault.

## Estado del arte

- **AWS KMS**: envelope encryption, key rotation, audit.
- **HashiCorp Vault**: transit engine, key versioning.
- **RuscaDB actual**: keystore KMS-lite local con rotaciÃ³n (SPEC-0088).

## DiseÃ±o propuesto

1. **KmsProvider trait**: interfaz para wrap/unwrap de claves.
2. **LocalProvider**: implementaciÃ³n local (XOR, para tests).
3. **AwsKmsProvider**: implementaciÃ³n AWS (futuro, requiere SDK).
4. **VaultProvider**: implementaciÃ³n Vault (futuro, requiere SDK).

## Notas

- ImplementaciÃ³n inicial: proveedor local (XOR) sin deps externas.
- IntegraciÃ³n real con AWS/Vault queda como trabajo futuro.
---
