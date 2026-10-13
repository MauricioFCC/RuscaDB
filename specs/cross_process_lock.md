---
id: SPEC-0076
feature: cross_process_lock
status: implemented
owner: storage-team
appetite_days: 3
boundaries:
  crates: [ruscadb, ruscadb-core]
  out_of_scope: [lectores concurrentes compartidos, compatibilidad de lock con SQLite, lock sobre NFS/remoto, byte-range fino WAL vs data]
fr:
  - { id: FR-0076-01, desc: "Database::open adquiere lock exclusivo no-bloqueante en el sidecar <data>.lock (std::fs::File::try_lock) y lo mantiene mientras la base esté abierta" }
  - { id: FR-0076-02, desc: "si el lock lo tiene otra instancia, open falla con RuscaError::DatabaseInUse { path } sin bloquear ni tocar datos" }
  - { id: FR-0076-03, desc: "al cerrar (drop) la Database se libera el lock y una reapertura posterior tiene éxito" }
nf:
  - { id: NF-0076-01, desc: "cero dependencias nuevas: solo std::fs (File::try_lock estable desde Rust 1.89); el MSRV del workspace sube a 1.89" }
  - { id: NF-0076-02, desc: "el lock se toma sobre un sidecar <data>.lock, nunca sobre el fichero de datos (evita el LockFileEx mandatory de Windows)" }
  - { id: NF-0076-03, desc: "errores con WHAT+WHY+WHERE; sin panics; docstrings ES-UTF8; sin unwrap/expect en producción" }
acceptance_criteria:
  - id: AC-0076-01
    given: dos procesos sobre la misma base
    when: el primero la tiene abierta y el segundo intenta Database::open
    then: el segundo recibe DatabaseInUse con la ruta (proceso hijo real, no simulado en memoria)
    test: test_ac_0076_01_second_process_fails_in_use
  - id: AC-0076-02
    given: un proceso con una Database abierta
    when: se intenta una segunda Database::open sobre la misma ruta en ese mismo proceso
    then: DatabaseInUse (el lock es exclusivo por instancia de apertura)
    test: test_ac_0076_02_second_open_same_process_fails
  - id: AC-0076-03
    given: una Database abierta que se cierra (drop)
    when: se vuelve a abrir la misma ruta
    then: la reapertura tiene éxito y la base sigue operativa (insert+select)
    test: test_ac_0076_03_drop_releases_lock_for_reopen
exit_criteria:
  - cargo test -p ruscadb --test process_lock
  - cargo test -p ruscadb  (suite completa, sin regresión)
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar lock.rs, el campo guard de Database y la subida de MSRV (volver a abrir sin lock)
sandbox:
  - cargo test -p ruscadb --test process_lock
---

# SPEC-0076 — Lock cross-proceso (Ola 0 del plan de producción)

## Contexto

Auditoría *production readiness* 2026-10-12 (riesgo 1): `MVP.md` declara
single-writer y el layout promete un fichero `LOCK`, pero **no existe lock
entre procesos**: dos procesos que abran el mismo `.data`/WAL pueden corromper
la base sin aviso.

Frontera 2026 (§3 del roadmap): `std::fs::File::try_lock` está estabilizado
desde Rust 1.89 (flock en POSIX, `LockFileEx` en Windows); redb 4.0 usa locks
byte-range con error `DatabaseAlreadyOpen`; `fs2` está muerto desde 2018.
Decisión: **lock exclusivo no-bloqueante sobre un sidecar `<data>.lock` con
`std` puro** (cero dependencias), nunca sobre el fichero de datos (en Windows
`LockFileEx` es mandatory y bloquearía los reads/mmap).

## Superficie

- `crates/ruscadb/src/lock.rs` (nuevo): `LockGuard` que abre `<data>.lock`,
  hace `try_lock()` y libera el fichero al dropearse.
- `Database` guarda el guard (`_lock: LockGuard`) mientras está viva.
- `RuscaError::DatabaseInUse { path }` (ya en `ruscadb-core`, prep Ola 0).
- MSRV del workspace: `rust-version = "1.89"` (justificado por `try_lock`).

## Criterios de aceptación

- **AC-0076-01** — segundo proceso real recibe `DatabaseInUse`.
- **AC-0076-02** — segunda apertura en el mismo proceso también falla.
- **AC-0076-03** — drop libera el lock; reapertura funcional.

## Trazabilidad

Tests `test_ac_0076_<nn>_*` con marcador `@spec AC-0076-NN`; verificado por
`cargo xtask trace`/`contract`.
