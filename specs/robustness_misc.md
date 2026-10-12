---
id: SPEC-0071
feature: robustness_misc
status: implemented
owner: storage-team
appetite_days: 3
boundaries:
  crates: [ruscadb, ruscadb-mcp]
  out_of_scope: [overflow pages del heap (linea futura), persistencia del audit]
fr:
  - { id: FR-0071-01, desc: "hybrid_search con k > MAXSIM_RERANK_DEPTH devuelve scores monótonos no crecientes (el re-scoring no mezcla escalas)" }
  - { id: FR-0071-02, desc: "la escritura atomica del registry usa un temporal con nombre correcto y limpia residuos; un JSON invalido no deja registry parcial" }
  - { id: FR-0071-03, desc: "el contador de jobs del pipeline satura (saturating_add) sin panic ni wrap" }
  - { id: FR-0071-04, desc: "el servidor MCP responde a batch JSON-RPC (array) con una respuesta por elemento" }
nf:
  - { id: NF-0071-01, desc: "cero regresion: SPEC-0063/0064/0067/0068 siguen verdes" }
  - { id: NF-0071-02, desc: "sin panics; sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0071-01
    given: una lista fusionada de mas de 20 candidatos donde el re-scoring reordena
    when: se aplica el re-scoring con k = 25
    then: todos los scores del top-k pertenecen a la MISMA escala (overlap) y son no crecientes
    test: test_ac_0071_01_hybrid_scores_monotonic_beyond_depth
    note: "unit test interno en hybrid.rs (listas artificiales; el caso no es disparable por integracion porque RRF domina el orden)"
  - id: AC-0071-02
    given: un register_model en un directorio limpio
    when: se registra un modelo y (en otro caso) se corrompe el JSON del registry
    then: no quedan ficheros .tmp y un registry invalido no registra entradas parciales
    test: test_ac_0071_02_registry_atomic_and_no_partial
  - id: AC-0071-03
    given: el contador interno de jobs del pipeline al maximo
    when: se encola un trabajo mas
    then: no hay panic ni wrap (satura en u64::MAX)
    test: test_ac_0071_03_job_counter_saturates
    note: "unit test interno en blob_pipeline.rs (no observable por la API publica)"
  - id: AC-0071-04
    given: un McpServer
    when: se envia un batch JSON-RPC (array con tools/list y un metodo desconocido)
    then: la respuesta es un array con una respuesta por elemento (exito y error -32601)
    test: test_ac_0071_04_mcp_batch_returns_array
exit_criteria:
  - cargo test -p ruscadb --test robustness_misc -- test_ac_0071
  - cargo test -p ruscadb-mcp  (SPEC-0068 sigue verde)
  - cargo clippy -p ruscadb -p ruscadb-mcp --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - revertir los fixes (scores, atomic write, saturating, batch)
sandbox:
  - cargo test -p ruscadb --test robustness_misc
---

# SPEC-0071 — Robustez miscelánea (retrospectiva 3)

## Contexto

**Hallazgos P2/P3 de la retrospectiva 3**: (1) `hybrid_search` con
`k > MAXSIM_RERANK_DEPTH` mezcla escalas (overlap en el top-20 y RRF en la
cola) → scores no monótonos; (2) `atomic_write` del registry usa un temporal
con nombre engañoso (`db.models.models.json.tmp`) y deja residuo si falla el
rename, y `restore_registry` puede quedar parcial; (3) el contador de jobs del
pipeline puede envolver; (4) el MCP no soporta batch JSON-RPC.

## Criterios de aceptación

- **AC-0071-01** — scores híbridos monótonos con `k > depth`.
- **AC-0071-02** — registry atómico y sin estado parcial.
- **AC-0071-03** — contador de jobs saturante.
- **AC-0071-04** — batch JSON-RPC soportado.

## Trazabilidad

Tests `test_ac_0071_<nn>_*`; verificado por `cargo xtask trace`/`contract`.