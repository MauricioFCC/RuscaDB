---
id: SPEC-0186
feature: surreal_session_expressions
status: implemented
maturity: real
owner: query-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-query]
  files:
    - crates/ruscadb-query/src/ast.rs
    - crates/ruscadb-query/src/parser.rs
    - crates/ruscadb/src/table_api.rs
  out_of_scope: [LIVE SELECT, DEFINE completo, permisos]
fr:
  - { id: FR-0186-01, desc: "LET variable = expr y resolucion de variables" }
  - { id: FR-0186-02, desc: "IF cond THEN expr ELSE expr END en proyeccion" }
  - { id: FR-0186-03, desc: "RETURN expr como sentencia de script" }
nf:
  - { id: NF-0186-01, desc: "entorno por query; sin estado global ni red" }
acceptance_criteria:
  - id: AC-0186-01
    given: un script con LET y una variable en WHERE
    when: se ejecuta execute_script
    then: el SELECT usa el valor de la variable
    test: test_ac_0186_01_let_variable_sql
  - id: AC-0186-02
    given: una proyeccion con IF ... THEN ... ELSE ... END
    when: se ejecuta el SELECT
    then: cada fila recibe la rama elegida
    test: test_ac_0186_02_if_projection_sql
  - id: AC-0186-03
    given: un script con RETURN de una variable
    when: se ejecuta execute_script
    then: se devuelve una fila con el valor
    test: test_ac_0186_03_return_sql
  - id: AC-0186-04
    given: una variable definida en un script previo
    when: otro script la usa sin definirla
    then: error accionable (sin fugas de contexto)
    test: test_ac_0186_04_variable_isolation_sql
---

# SPEC-0186 — Variables y expresiones de sesión estilo SurrealQL (propuesta)

## Contexto

SurrealQL permite `LET $x = expr;`, `IF ... THEN ... ELSE ... END` y `RETURN`
para scripts multi-sentencia. RQL ejecuta una sentencia por llamada y no tiene
variables ni `IF`; los clientes concatenan SQL o calculan en Rust.

## Diseño propuesto

- `Statement::Let { name, expr }` y un `SessionContext` por `execute` (o
  `execute_script`) con `BTreeMap<String, ScalarValue>`.
- `Expr::Variable(String)` y `Expr::If { condition, then, otherwise }`.
- `Statement::Return(Expr)` para devolver el valor de una expresión.
- Resolución de variables en `eval_scalar`/`eval_operand` (prefijo `$`).
- Sin estado global: el contexto vive en la llamada o en un `PreparedHandle`.

## Complejidad

```text
LET:      O(1) por variable (BTreeMap)
IF:       O(1) + coste de la rama elegida
RETURN:   O(1) + evaluacion de la expresion
script:   O(sentencias) con el mismo snapshot MVCC
```

## Estado de implementación (2026-11-02)

- `parse_script` + `Database::execute_script`: separa sentencias por `;` y
  comparte un `BTreeMap<String, ScalarValue>` por llamada.
- `Statement::Let`/`Statement::Return`; `$nombre` se lexa como
  `Token::Variable` y se sustituye por su literal antes de ejecutar.
- `IF ... THEN ... ELSE ... END` se normaliza a `Expr::Case` (sin variante
  nueva de IR) y se proyecta por fila.
- Variable no definida → `InvalidConfig` con el nombre de la variable.
- Sin estado global: cada `execute_script` crea su propio contexto.

## Criterios de aceptación

- `LET $min = 10; SELECT * FROM t WHERE x > $min;` en un solo `execute_script`.
- `IF x > 0 THEN 'up' ELSE 'down' END` en proyección.
- Variables no definidas → error accionable, nunca `NULL` silencioso.
- Sin fugas entre ejecuciones (test de aislamiento de contexto).

## Referencias

- SurrealDB, *LET*, *IF ELSE* y *RETURN* (docs oficiales).
- Pratt/recursive-descent parsing (Aho et al., *Compilers*, 2ª ed.).
