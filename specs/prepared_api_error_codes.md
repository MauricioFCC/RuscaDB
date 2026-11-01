---
id: SPEC-0184
feature: prepared_api_error_codes
status: implemented
maturity: real
owner: query-team
appetite_days: 3
boundaries:
  crates: [ruscadb, ruscadb-core]
  files:
    - crates/ruscadb/src/prepared_api.rs
    - crates/ruscadb/src/prepared_real.rs
    - crates/ruscadb-core/src/error.rs
  out_of_scope: [protocolo wire de bind, cursores, portal suspend/resume]
fr:
  - { id: FR-0184-01, desc: "API publica de prepared statements y codigos estables" }
nf:
  - { id: NF-0184-01, desc: "Sin SQL concatenado por el cliente; literales escapados" }
acceptance_criteria:
  - id: AC-0184-01
    given: una consulta con marcadores
    when: se prepara una vez y se ejecuta con valores
    then: reutiliza el mismo plan y devuelve las filas correctas
    test: test_ac_0184_01_prepare_execute
  - id: AC-0184-02
    given: errores tipados del motor
    when: se consulta RuscaError::code
    then: el codigo es estable y no depende del texto
    test: test_ac_0184_02_error_codes
  - id: AC-0184-03
    given: un handle preparado
    when: se enlaza una aridad o tipo incorrecto
    then: error accionable (INVALID_CONFIG) sin ejecutar SQL parcial
    test: test_ac_0184_03_bind_errors
exit_criteria:
  - cargo test -p ruscadb --all-features --test spec_0183_0184
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir prepared_api/prepared_real/error.rs
sandbox:
  - cargo test -p ruscadb --all-features --test spec_0183_0184
---

# SPEC-0184 — Prepared statements públicos y códigos de error estables

## Contexto

`PreparedStmtReal` validaba marcadores y escapaba literales, pero no era
accesible desde `Database`; los clientes concatenaban SQL. Además
`RuscaError` solo ofrecía prosa, obligando a parsear mensajes.

## Diseño

- `Database::prepare(sql)` → `PreparedHandle` con `param_count` y
  `plan_cache_key` (FNV-1a 64 del texto normalizado).
- `Database::prepare_typed(sql, tipos)` valida tipos por posición.
- `Database::execute_prepared(handle, &[ParamValue])` enlaza valores y ejecuta;
  nunca acepta SQL del cliente después del `prepare`.
- `RuscaError::code()` devuelve `UPPER_SNAKE_CASE` estable
  (`RQL_PARSE`, `COLUMN_NOT_FOUND`, `TYPE_MISMATCH`, ...).

## Complejidad

- `prepare`: `O(L)` (escaneo de marcadores y hash del texto), una sola vez.
- `bind`: `O(p)` (p = parámetros) con renderizado escapado.
- `execute`: el coste del plan, sin reparsear (`O(L)` ahorrado por ejecución).
- El cache de plan usa una clave de 64 bits; una colisión solo provoca
  revalidación, nunca resultados incorrectos (la clave no sustituye el texto).

## Referencias

- PostgreSQL 18, "Extended Query" (`Parse`/`Bind`/`Execute`) y "Error Codes"
  (SQLSTATE), documentación oficial.
- ISO/IEC 9075-2:2023, prepared statements.
