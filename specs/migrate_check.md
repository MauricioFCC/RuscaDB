---
id: SPEC-0191
feature: migrate_check
status: implemented
maturity: real
owner: core-team
appetite_days: 1
boundaries:
  crates: [ruscadb-cli]
  files: [crates/ruscadb-cli/src/main.rs]
  out_of_scope: [migrate --apply con backup y downgrade]
fr:
  - { id: FR-0191-01, desc: "migrate --check reporta schema_version y compatibilidad" }
nf:
  - { id: NF-0191-01, desc: "sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0191-01
    given: una base con schema_version soportada
    when: se ejecuta rusadb migrate --db <ruta> check
    then: imprime schema_version y ok=true (exit 0)
    test: test_ac_0191_01_migrate_check
exit_criteria:
  - cargo test -p ruscadb-cli --test migrate_cli
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir el subcomando migrate
sandbox:
  - cargo test -p ruscadb-cli --test migrate_cli
---

# SPEC-0191 — `migrate --check` (P2.6 fase 1)

## Implementado

- `ruscadb migrate --db <ruta> check`: compara `manifest().schema_version` con
  `CURRENT_SCHEMA_VERSION`; error accionable si el fichero es mas nuevo.
- `apply`: informa que abrir la base aplica las migraciones disponibles
  (`Manifest::load`); backup/downgrade quedan fuera de alcance.

## Complejidad

`O(1)` tras abrir la base.
