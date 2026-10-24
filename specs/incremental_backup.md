---
id: SPEC-0120
feature: incremental_backup
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/incremental_backup.rs]
  out_of_scope: [backup remoto, cifrado del backup, compresion]
fr:
  - { id: FR-0120-01, desc: "feature flag `incremental-backup` (off por defecto) habilita backup incremental" }
  - { id: FR-0120-02, desc: "IncrementalBackup::snapshot copia solo ficheros modificados desde el ultimo backup" }
nf:
  - { id: NF-0120-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0120-01
    given: la feature `incremental-backup` activada
    when: se crea un backup incremental
    then: solo los ficheros nuevos o modificados se copian
    test: test_ac_0120_01_incremental_backup
exit_criteria:
  - cargo test -p ruscadb --all-features --test incremental_backup
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0120 — Backup incremental

## Contexto

Frontier 2026: backup incremental (hot backup) para operaciones. RuscaDB tiene
backup básico (SPEC-0116). Esta spec añade backup incremental detrás de un
feature flag.

## Superficie

- Feature `incremental-backup` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/incremental_backup.rs` — `IncrementalBackup` con `snapshot`.
- Integración futura: backup remoto, cifrado, compresión.

## Notas

- Implementación inicial: copia basada en timestamp de modificación.
- Backup remoto y cifrado quedan como trabajo futuro.
---
