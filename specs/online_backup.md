---
id: SPEC-0116
feature: online_backup
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core]
  files: [crates/ruscadb/src/backup.rs]
  out_of_scope: [backup remoto, cifrado del backup, incremental]
fr:
  - { id: FR-0116-01, desc: "feature flag `backup` (off por defecto) habilita backup online" }
  - { id: FR-0116-02, desc: "Backup::snapshot copia los ficheros de la base a un directorio destino" }
nf:
  - { id: NF-0116-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0116-01
    given: la feature `backup` activada
    when: se crea un backup de una base
    then: los ficheros se copian al destino
    test: test_ac_0116_01_backup_snapshot
exit_criteria:
  - cargo test -p ruscadb --all-features --test backup
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0116 — Backup online

## Contexto

Frontier 2026: backup online (hot backup) para operaciones. RuscaDB no tiene
backup integrado. Esta spec añade un backup básico detrás de un feature flag.

## Superficie

- Feature `backup` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/backup.rs` — `Backup::snapshot` copia los ficheros.

## Notas

- Implementación inicial: copia de ficheros (.data, .wal, .manifest).
- Backup incremental y cifrado quedan como trabajo futuro.
---
