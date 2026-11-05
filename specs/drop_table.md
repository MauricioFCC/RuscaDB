---
id: SPEC-0207
feature: drop_table
status: implemented
maturity: real
owner: query-team
appetite_days: 1
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/catalog.rs
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/tests/drop_table.rs
  out_of_scope: [reclamar paginas del heap, DROP TABLE en RQL, CASCADE]
fr:
  - { id: FR-0207-01, desc: "drop_table elimina la tabla del catalogo y devuelve si existia" }
  - { id: FR-0207-02, desc: "limpia indices/primary/stats/plan cache; recrear empieza limpio" }
nf:
  - { id: NF-0207-01, desc: "idempotente; un segundo drop devuelve false" }
  - { id: NF-0207-02, desc: "paginas no referenciadas (bump allocator), sin corromper el formato" }
acceptance_criteria:
  - id: AC-0207-01
    given: una tabla con filas e indice
    when: se ejecuta drop_table
    then: la tabla desaparece, las consultas fallan y repetir es no-op
    test: test_ac_0207_01_drop_table_removes_table
  - id: AC-0207-02
    given: una tabla eliminada
    when: se recrea con el mismo nombre
    then: empieza limpia (sin filas ni indices viejos)
    test: test_ac_0207_02_recreate_table_is_clean
exit_criteria:
  - cargo test -p ruscadb --test drop_table
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar drop_table; la tabla sigue en el catalogo
sandbox:
  - cargo test -p ruscadb --test drop_table
---
# SPEC-0207 — DROP TABLE (API)

## Contexto

El catálogo permite crear tablas e índices pero no eliminarlos; un esquema
obsoleto ocupa páginas y aparece en `list_tables`.

## Diseño

1. `Database::drop_table` valida write/rol y marca mutación.
2. `Catalog::remove_table` retira la tabla; segundo drop `false`.
3. Limpia `indexes`, `primary`, `stats`, `stats_epoch`, `plan_cache` y reescribe
   el sidecar de stats.
4. Páginas no referenciadas (bump allocator).

## Complejidad

```text
drop: O(log T) catálogo + O(1) por mapa
memoria: O(1)
```

## TDD adversarial

Consulta posterior falla con `TableNotFound`; segundo drop idempotente; recrear
no hereda filas/índices.

## Riesgos

Sin `CASCADE`; reclamación de páginas fuera de alcance.
