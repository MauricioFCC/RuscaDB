---
id: SPEC-0128
feature: s3_backup
status: implemented
maturity: real
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/s3_backup.rs]
  out_of_scope: [AWS SDK real, autenticacion IAM, multipart upload]
fr:
  - { id: FR-0128-01, desc: "feature flag `s3-backup` (on por defecto; pure-Rust) habilita backup S3" }
  - { id: FR-0128-02, desc: "S3Backup::snapshot genera un manifiesto de backup" }
nf:
  - { id: NF-0128-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0128-01
    given: la feature `s3-backup` activada
    when: se crea un manifiesto de backup
    then: el manifiesto contiene los ficheros de la base
    test: test_ac_0128_01_s3_manifest
exit_criteria:
  - cargo test -p ruscadb --all-features --test s3_backup
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0128 — Checkpoint a S3 (backup/restore)

## Contexto

Frontier 2026 (FFS): checkpoint a S3-compatible object storage.
RuscaDB tiene backup local (SPEC-0116) e incremental (SPEC-0120).
Esta spec añade backup a S3 detrás de un feature flag.

## Superficie

- Feature `s3-backup` en `ruscadb` (on por defecto; pure-Rust).
- `crates/ruscadb/src/s3_backup.rs` — `S3Backup` con `manifest`.
- Integración futura: AWS SDK, multipart upload, restore.

## Notas

- Implementación inicial: manifiesto JSON de ficheros (sin upload real).
- Upload real queda como trabajo futuro.
---
