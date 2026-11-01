---
id: SPEC-0186
feature: surreal_session_expressions
status: proposed
maturity: stub
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

## Criterios de aceptación (pendientes de implementar)

- `LET $min = 10; SELECT * FROM t WHERE x > $min;` en un solo `execute_script`.
- `IF x > 0 THEN 'up' ELSE 'down' END` en proyección.
- Variables no definidas → error accionable, nunca `NULL` silencioso.
- Sin fugas entre ejecuciones (test de aislamiento de contexto).

## Referencias

- SurrealDB, *LET*, *IF ELSE* y *RETURN* (docs oficiales).
- Pratt/recursive-descent parsing (Aho et al., *Compilers*, 2ª ed.).
