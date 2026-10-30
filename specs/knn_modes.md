---
id: SPEC-0181
feature: knn_modes
status: implemented
maturity: real
owner: query-team
appetite_days: 3
boundaries:
  crates: [ruscadb, ruscadb-query, ruscadb-ffi]
  files:
    - crates/ruscadb-query/src/ast.rs
    - crates/ruscadb-query/src/parser.rs
    - crates/ruscadb/src/executor.rs
    - crates/ruscadb-ffi/src/lib.rs
  out_of_scope: [nuevos indices ANN, GPU, dependencias externas]
fr:
  - { id: FR-0181-01, desc: "Resolver KNN con estrategia declarada en RQL" }
nf:
  - { id: NF-0181-01, desc: "Sin dependencias nuevas ni cambios de ABI incompatibles" }
acceptance_criteria:
  - id: AC-0181-01
    given: una tabla con embeddings y una consulta KNN USING EXACT
    when: se ejecuta la consulta
    then: devuelve el vecino mas cercano primero (fuerza bruta exacta)
    test: test_ac_0181_01_exact_nearest_first
  - id: AC-0181-02
    given: una tabla con embeddings y una consulta KNN USING QUANTIZED
    when: se ejecuta la consulta
    then: usa el indice cuantizado y conserva el mejor vecino
    test: test_ac_0181_02_quantized_finds_nearest
  - id: AC-0181-03
    given: una consulta KNN USING FILTERED con WHERE
    when: se ejecuta la consulta
    then: solo devuelve filas que cumplen el predicado
    test: test_ac_0181_03_filtered_respects_where
  - id: AC-0181-04
    given: una consulta KNN sin USING
    when: se ejecuta la consulta
    then: se conserva el comportamiento AUTO previo
    test: test_ac_0181_04_default_mode_is_auto
  - id: AC-0181-05
    given: una base abierta con embeddings
    when: se invoca ruscadb_search_quantized
    then: devuelve JSON de resultados o el codigo de error del contrato ABI
    test: test_ac_0181_05_search_quantized_contract
  - id: AC-0181-06
    given: una base abierta con embeddings
    when: se invoca ruscadb_search_filtered con un conjunto allowed
    then: devuelve solo ids permitidos o el codigo de error del contrato ABI
    test: test_ac_0181_06_search_filtered_contract
  - id: AC-0181-07
    given: una consulta KNN con un modo desconocido
    when: se parsea
    then: error de sintaxis que nombra los modos validos
    test: test_ac_0181_07_unknown_mode_is_parse_error
exit_criteria:
  - cargo test -p ruscadb --all-features --test vector_knn_modes
  - cargo test -p ruscadb-ffi --test vector_search_abi
  - cargo test -p ruscadb-query --lib
  - cargo xtask trace
  - cargo xtask contract
rollback:
  - revertir ast.rs, parser.rs, executor.rs y lib.rs del FFI
sandbox:
  - cargo test -p ruscadb --all-features --test vector_knn_modes
---

# SPEC-0181 — Modos de resolución KNN (`USING`)

## Contexto

El cableado de la fachada ya ofrecía `search_vectors_filtered` (ACORN,
SPEC-0172) y `search_vectors_quantized` (RaBitQ + re-rank, SPEC-0171), pero RQL
no permitía elegirlas: el executor siempre resolvía con FVS/KNN clásico y el
ABI no las exponía. Esta spec cierra el gap sin dependencias nuevas ni cambios
incompatibles de gramática (el modificador es opcional).

## Superficie

- `KNN <col> <|k|> [v...] USING AUTO|QUANTIZED|FILTERED|EXACT`
- `Database::search_vectors_quantized` / `search_vectors_filtered` (ya existentes)
- `ruscadb_search_quantized` y `ruscadb_search_filtered` en la C-ABI.

`USING` se acepta como identificador (no reservado) para no romper columnas
llamadas `using`.

## Semántica

| Modo | Resolución |
|---|---|
| `AUTO` (defecto) | FVS con `WHERE` o KNN clásico sin él (comportamiento previo). |
| `QUANTIZED` | ANN en dos etapas (RaBitQ + re-rank) y recorte a `allowed`. |
| `FILTERED` | ACORN restringido al conjunto `allowed` del `WHERE`. |
| `EXACT` | Fuerza bruta exacta sobre los candidatos visibles (oráculo). |

Todos los modos verifican el top-k servido contra el oráculo exacto cuando las
shadow reads están activas.

## Contrato ABI

- `ruscadb_search_quantized(handle, table, query, dim, k, out_json, len)`
- `ruscadb_search_filtered(handle, table, query, dim, k, allowed_json, out_json, len)`

`allowed_json` es un array de ids (`["01H...", ...]`); un JSON invalido devuelve
`RC_DOMAIN_ERROR` y un puntero nulo `RC_NULL_POINTER`.

## Coste

- `EXACT`: `O(n·d)` sobre los candidatos visibles.
- `QUANTIZED`: `O(n·d/8 + k·d)` (estimacion binaria + re-rank).
- `FILTERED`: recorrido best-first de ACORN, `O(log n + k)` esperado.
- `AUTO`: el de FVS (pre/in/post según selectividad).
