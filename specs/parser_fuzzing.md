---
id: SPEC-0042
feature: parser_fuzzing
status: accepted
owner: quality-team
appetite_days: 4
boundaries:
  crates: []
  out_of_scope: [correr libFuzzer localmente (requiere nightly), fuzzing de red]
fr:
  - { id: FR-0042-01, desc: "target cargo-fuzz del parser RQL reforzado (cubre todas las clausulas: SELECT/WHERE/MATCH/KNN/TRAVERSE/EXPLAIN/ORDER BY/LIMIT)" }
  - { id: FR-0042-02, desc: "corpus inicial con semillas (gramaticales y de borde) versionado" }
  - { id: FR-0042-03, desc: "job nightly de fuzzing del parser (libFuzzer, alert-only) validado por check_ci_config" }
  - { id: FR-0042-04, desc: "el target del parser no tiene rutas de panic (sin unwrap/expect sobre el resultado) y cubre todas las clausulas" }
nf:
  - { id: NF-0042-01, desc: "el target compila en stable (cargo check del workspace fuzz)" }
  - { id: NF-0042-02, desc: "YAML valido; sin secretos" }
acceptance_criteria:
  - id: AC-0042-01
    given: el target query_parse
    when: se revisa
    then: cubre todas las clausulas del lenguaje (referencia a ORDER BY/GROUP BY/KNN/TRAVERSE/EXPLAIN/MATCH)
    test: test_ac_0042_01_fuzz_target_covers_grammar
  - id: AC-0042-02
    given: el corpus del parser
    when: se inspecciona
    then: contiene >= 5 semillas versionadas con consultas validas
    test: test_ac_0042_02_fuzz_corpus_seeded
  - id: AC-0042-03
    given: nightly.yml
    when: se valida
    then: existe el job fuzz del parser (query_parse) alert-only
    test: test_ac_0042_03_nightly_fuzz_job
  - id: AC-0042-04
    given: el target query_parse
    when: se revisa su cuerpo
    then: no usa unwrap/expect sobre el resultado del parser (contrato no-panic)
    test: test_ac_0042_04_fuzz_target_is_panic_free
exit_criteria:
  - cargo check --manifest-path fuzz/Cargo.toml
  - python scripts/check_ci_config.py
  - cargo test -p xtask -- test_ac_0042
rollback:
  - revertir fuzz/ y nightly.yml
sandbox:
  - cargo check --manifest-path fuzz/Cargo.toml
---

# SPEC-0042 — Fuzzing continuo del parser RQL

## Contexto

F2 exige "fuzz parser 0 crash". El scaffold `fuzz/` ya existe. Se refuerza:
target `query_parse` que cubre toda la gramática (incluidas `ORDER BY`/`GROUP BY`/
`KNN`/`TRAVERSE`/`EXPLAIN`/`MATCH`), corpus inicial versionado, job nightly
(ya presente) validado, y un test de propiedad en `ruscadb-query` que garantiza
"nunca panic" (ejecutable en stable, sin nightly).

## Criterios de aceptación

- **AC-0042-01/02** — target completo y corpus.
- **AC-0042-03** — job nightly.
- **AC-0042-04** — invariante no-panic (stable).

## Trazabilidad

Tests `test_ac_0042_<nn>_*`; verificado por `cargo xtask trace`.
