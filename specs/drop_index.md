---
id: SPEC-0205
feature: drop_index
status: implemented
maturity: real
owner: query-team
appetite_days: 1
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/catalog.rs
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/tests/drop_index.rs
  out_of_scope: [reclamar paginas del indice, DROP TABLE, DROP INDEX en RQL]
fr:
  - { id: FR-0205-01, desc: "drop_index elimina el indice del catalogo y devuelve si existia" }
  - { id: FR-0205-02, desc: "el planner deja de usar el indice y la cache de planes se invalida" }
nf:
  - { id: NF-0205-01, desc: "sin cambios de formato; paginas no referenciadas (bump allocator)" }
  - { id: NF-0205-02, desc: "idempotente: un segundo drop devuelve false" }
acceptance_criteria:
  - id: AC-0205-01
    given: una tabla con indice secundario
    when: se ejecuta drop_index
    then: el indice desaparece, EXPLAIN usa FullScan y repetir es no-op
    test: test_ac_0205_01_drop_index_falls_back
  - id: AC-0205-02
    given: un indice eliminado
    when: se vuelve a crear
    then: el planner lo usa, los datos siguen intactos y la cache se invalida
    test: test_ac_0205_02_recreate_index
exit_criteria:
  - cargo test -p ruscadb --test drop_index
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar drop_index; el indice sigue en el catalogo
sandbox:
  - cargo test -p ruscadb --test drop_index
---
# SPEC-0205 — DROP INDEX (API)

## Contexto

El catálogo permite crear índices (SPEC-0104) pero no eliminarlos; un índice
obsoleto penaliza escrituras y ocupa páginas.

## Diseño

1. `Database::drop_index(table, column)` valida write/rol.
2. `Catalog::remove_index` retira el `IndexDef`; segundo drop `false`.
3. Persistencia con `catalog.save`/`save_pages`.
4. `mark_mutated` invalida stats y plan cache.

## Complejidad

```text
drop: O(I) por tabla + O(1) catálogo
memoria: O(1)
```

## TDD adversarial

Plan antes/después (IndexScan→FullScan), segundo drop idempotente, recrear y
verificar datos/plan.

## Riesgos

Páginas no reclamadas (bump allocator); `DROP INDEX` en RQL y `DROP TABLE`
fuera de alcance (este último en SPEC-0207).
