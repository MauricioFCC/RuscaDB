---
id: SPEC-0166
feature: prepared_statements_real
status: implemented
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-query]
  files: [crates/ruscadb/src/prepared.rs, crates/ruscadb/src/database.rs, crates/ruscadb-query/src/parser.rs]
  out_of_scope: [plan caching, server-side prepare]
fr:
  - { id: FR-0166-01, desc: "placeholders $1, $2, ? en SQL" }
  - { id: FR-0166-02, desc: "PreparedStatement::bind params tipados" }
  - { id: FR-0166-03, desc: "reuso del AST parseado en execute" }
  - { id: FR-0166-04, desc: "escape de SQL injection (params no interpolados)" }
nf:
  - { id: NF-0166-01, desc: "cero dependencias nuevas" }
acceptance_criteria:
  - id: AC-0166-01
    given: prepare(\"SELECT * FROM t WHERE id = $1\")
    when: execute con param 42
    then: se devuelve la fila con id=42
    test: test_ac_0166_01_bind
  - id: AC-0166-02
    given: el mismo prepared statement ejecutado dos veces con params distintos
    when: se ejecuta
    then: los resultados difieren segun el param
    test: test_ac_0166_02_reuse
  - id: AC-0166-03
    given: un param con comillas/SQL malicioso
    when: se ejecuta
    then: se trata como valor literal (sin inyeccion)
    test: test_ac_0166_03_injection
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-query --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0166 — Sentencias preparadas reales + placeholders

## Contexto

RuscaDB parsea en cada `execute` y no soporta placeholders. Esta spec
reemplaza el stub `PreparedStatement` (SPEC-0148) con implementacion real.

## Superficie

- `parser.rs`: placeholders `$n` y `?` en expresiones literales.
- `prepared.rs`: `PreparedStatement { ast, param_count }`, `bind`, `execute`.
- `database.rs`: API `prepare`/`execute_prepared`.

## Estado del arte

- **PostgreSQL**: `PREPARE`/`EXECUTE` con plan cache.
- **SQLite**: `sqlite3_prepare_v2` con bytecode + `sqlite3_bind_*`.
- **RuscaDB actual**: stub (SPEC-0148) sin placeholders.

## Diseño propuesto

1. **Parse**: detectar placeholders y asignar indices.
2. **Bind**: almacenar params en un Vec tipado (ScalarValue).
3. **Execute**: sustituir placeholders por params al evaluar (no interpolar
   texto), reusando el AST.

## Riesgos

- Plan cache: fuera de alcance; re-ejecutar el plan cada vez.

## ADR requerido

`docs/adr/ADR-0166-prepared-statements.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\prepared_statements_real.md