---
id: SPEC-0157
feature: backup_verification
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/backup_verify.rs]
  out_of_scope: [backup remoto, cifrado del backup]
fr:
  - { id: FR-0157-01, desc: "feature flag `backup-verify` (off por defecto) habilita verificacion de backup" }
  - { id: FR-0157-02, desc: "BackupVerifier::verify compara checksums de origen y destino" }
nf:
  - { id: NF-0157-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0157-01
    given: la feature `backup-verify` activada
    when: se verifica un backup identico
    then: la verificacion es exitosa
    test: test_ac_0157_01_backup_verify
exit_criteria:
  - cargo test -p ruscadb --all-features --test backup_verify
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0157 — Verificación de backup (checksums)

## Contexto

Frontier 2026: verificación de integridad de backups. RuscaDB tiene backup
básico (SPEC-0116) e incremental (SPEC-0120). Esta spec añade verificación
de checksums detrás de un feature flag.

## Superficie

- Feature `backup-verify` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/backup_verify.rs` — `BackupVerifier` con `verify`.
- Integración futura: verificación remota, cifrado.

## Estado del arte

- **RocksDB**: `db.checkpoint()` + verificación.
- **PostgreSQL**: `pg_verify_checksums`.
- **RuscaDB actual**: backup por copia de ficheros (SPEC-0116).

## Diseño propuesto

1. **BackupVerifier**: compara checksums SHA-256 de origen y destino.
2. **verify**: retorna Ok si todos los ficheros coinciden.
3. **report**: genera un reporte de diferencias.

## Notas

- Implementación inicial: comparación de checksums por fichero.
- Verificación remota queda como trabajo futuro.
---
