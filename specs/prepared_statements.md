---
id: SPEC-0148
feature: prepared_statements
status: implemented
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb/src/prepared.rs]
  out_of_scope: [parametrized queries con placeholders, plan caching]
fr:
  - { id: FR-0148-01, desc: "feature flag `prepared` (off por defecto) habilita sentencias preparadas" }
  - { id: FR-0148-02, desc: "PreparedStatement::prepare parsea una vez, ::execute reutiliza" }
nf:
  - { id: NF-0148-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0148-01
    given: la feature `prepared` activada
    when: se prepara una sentencia SELECT
    then: el resultado del execute es correcto
    test: test_ac_0148_01_prepared_select
exit_criteria:
  - cargo test -p ruscadb --all-features --test prepared
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0148 — Sentencias preparadas (pre-compiled queries)

## Contexto

Frontier 2026 (PostgreSQL, SQLite): sentencias preparadas evitan re-parsear
la misma consulta. RuscaDB parsea en cada `execute`. Esta spec añade
preparación de sentencias detrás de un feature flag.

## Superficie

- Feature `prepared` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/prepared.rs` — `PreparedStatement` con `prepare` + `execute`.
- Integración futura: placeholders (`$1`, `?`), plan caching.

## Estado del arte

- **PostgreSQL**: `PREPARE` / `EXECUTE` con plan caching.
- **SQLite**: `sqlite3_prepare_v2` con bytecode.
- **RuscaDB actual**: parsea en cada `execute`.

## Diseño propuesto

1. **PreparedStatement**: parsea una vez, almacena el AST.
2. **execute**: reutiliza el AST parseado.
3. **Placeholders**: `$1`, `?` para parámetros (futuro).

## Notas

- Implementación inicial: preparación de SELECT sin placeholders.
- Placeholders y plan caching quedan como trabajo futuro.
---
