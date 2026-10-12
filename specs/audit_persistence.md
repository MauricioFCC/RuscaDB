---
id: SPEC-0073
feature: audit_persistence
status: implemented
owner: core-team
appetite_days: 4
boundaries:
  crates: [ruscadb]
  out_of_scope: [cifrado del audit, replicacion, consultas sobre el audit, retencion por tiempo]
fr:
  - { id: FR-0073-01, desc: "audit persistente append-only en <data>.audit.jsonl con CRC32C por linea (patron WAL): sobrevive al reopen" }
  - { id: FR-0073-02, desc: "carga al abrir descartando lineas corruptas (sin panic) y rotacion por tamano (MAX_AUDIT_BYTES -> .1)" }
nf:
  - { id: NF-0073-01, desc: "cero regresion: audit OFF por defecto (sin fichero); clear_audit borra el fichero y el trail" }
  - { id: NF-0073-02, desc: "sin panics; errores accionables WHAT+WHY+WHERE; sin dependencias nuevas (CRC del crate ya en el workspace)" }
acceptance_criteria:
  - id: AC-0073-01
    given: audit habilitado con tres operaciones registradas
    when: se cierra y reabre la base
    then: audit_trail recupera las tres entradas desde <data>.audit.jsonl (persistencia)
    test: test_ac_0073_01_audit_survives_reopen
  - id: AC-0073-02
    given: un audit.jsonl con una linea con CRC alterado
    when: se reabre la base
    then: se cargan las lineas validas y la corrupta se descarta sin panic
    test: test_ac_0073_02_corrupt_line_is_discarded
  - id: AC-0073-03
    given: audit OFF (por defecto) con operaciones ejecutadas
    when: se reabre la base
    then: audit_trail esta vacio y no hay fichero de audit con entradas
    test: test_ac_0073_03_audit_off_creates_no_entries
  - id: AC-0073-04
    given: un audit habilitado cuyo fichero supera MAX_AUDIT_BYTES
    when: se registra una operacion mas
    then: el fichero activo rota a .1 y el activo continua con la nueva entrada
    test: test_ac_0073_04_audit_rotates_by_size
  - id: AC-0073-05
    given: audit habilitado con entradas persistidas
    when: se ejecuta clear_audit
    then: el trail en memoria y el fichero quedan vacios, y la captura sigue activa
    test: test_ac_0073_05_clear_audit_wipes_file
exit_criteria:
  - cargo test -p ruscadb --test audit_persistence -- test_ac_0073
  - cargo test -p ruscadb --test audit_rbac --test write_path_hardening
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - volver al audit solo en memoria (eliminar fichero y carga)
sandbox:
  - cargo test -p ruscadb --test audit_persistence
---

# SPEC-0073 — Persistencia del audit (append-only con CRC, patrón WAL)

## Contexto

Línea futura «persistencia de caché/audit». La caché persistente se descarta
por diseño (cada mutación invalida todas las entradas → valor cero); el **audit**
sí gana: es forense post-mortem. Se persiste con el mismo patrón del WAL
(append-only + CRC32C por registro + tolerancia a cola corrupta) y rotación por
tamaño, sin dependencias nuevas.

## Superficie

- Ruta derivada: `<data>.audit.jsonl` (mismo patrón que `models_path`).
- Constantes: `MAX_AUDIT_BYTES = 1 MiB`, `MAX_AUDIT_ENTRIES = 10_000`.
- `note_audit` con audit ON → append + flush de una línea
  `{"action":..,"detail":..,"nanos":..,"crc":..}`.
- `open` → carga (descarta corruptas); `clear_audit` → vacía fichero y trail.

## Criterios de aceptación

- **AC-0073-01/02** — persistencia y tolerancia a corrupción.
- **AC-0073-03/05** — OFF por defecto y limpieza coherente.
- **AC-0073-04** — rotación por tamaño.

## Trazabilidad

Tests `test_ac_0073_<nn>_*`; verificado por `cargo xtask trace`/`contract`.