---
id: SPEC-0174
feature: learned_index
status: implemented
owner: storage-team
appetite_days: 7
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-btree]
  files: [crates/ruscadb-btree/src/lib.rs]
  out_of_scope: [updates masivos, multi-dim]
fr:
  - { id: FR-0174-01, desc: "PGM-index: piecewise geometric model para rangos" }
  - { id: FR-0174-02, desc: "ALEX: adaptive learned index con splits" }
  - { id: FR-0174-03, desc: "fallback a B+tree cuando el error supera umbral" }
  - { id: FR-0174-04, desc: "metricas de error de prediccion" }
nf:
  - { id: NF-0174-01, desc: "cero dependencias nuevas" }
  - { id: NF-0174-02, desc: "lookup O(log log n) esperado" }
acceptance_criteria:
  - id: AC-0174-01
    given: un dataset ordenado de N claves
    when: se construye el PGM-index
    then: la prediccion de posicion tiene error acotado (epsilon)
    test: test_ac_0174_01_build
  - id: AC-0174-02
    given: una clave no presente
    when: se busca
    then: se devuelve None (con busqueda local en el segmento)
    test: test_ac_0174_02_lookup
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-btree --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0174 — Learned index (PGM/ALEX) para rangos

## Contexto

B+tree (SPEC-0020) da O(log n). Learned indexes predicen la posicion de una
clave con un modelo lineal por segmento, dando O(log log n) + busqueda local.

## Superficie

- `btree/lib.rs`: `LearnedIndex { segments, epsilon, predict }`,
  `build(keys)`, `lookup(key) -> Option<usize>`.

## Estado del arte

- **"The Case for Learned Index Structures"** (Kraska et al., SIGMOD 2018).
- **PGM-index** (Ferragina & Vinciguerra, VLDB 2020): piecewise geometric.
- **ALEX** (Ding et al., SIGMOD 2020): adaptive learned index.
- **RuscaDB actual**: B+tree propio (SPEC-0020).

## Diseño propuesto

1. **Build**: segmentar claves en segmentos lineales con error <= epsilon.
2. **Predict**: posicion estimada por segmento + busqueda binaria local.
3. **Fallback**: si error > umbral, usar B+tree tradicional.

## Riesgos

- Updates: PGM es read-only; ALEX soporta updates con splits (costoso).

## ADR requerido

`docs/adr/ADR-0174-learned-index.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\learned_index.md