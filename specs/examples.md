---
id: SPEC-0087
feature: examples
status: implemented
owner: docs-team
appetite_days: 2
boundaries:
  crates: [ruscadb]
  files: [crates/ruscadb/examples/*.rs, crates/ruscadb/tests/examples_smoke.rs, docs/api_guide.md]
  out_of_scope: [ejemplos en bindings Python/Node/WASM, playground web, benchmarks como ejemplos, ejemplos de concurrencia multi-proceso]
fr:
  - { id: FR-0087-01, desc: "ejemplo quickstart: crear base, tabla, INSERT/SELECT/UPDATE/DELETE con WHERE sobre la fachada `ruscadb`, ejecutable con `cargo run --example quickstart`" }
  - { id: FR-0087-02, desc: "ejemplo vector_search: embeddings + `KNN` + `MATCH` (BM25) sobre la misma tabla, ejecutable con `cargo run --example vector_search`" }
  - { id: FR-0087-03, desc: "ejemplo graph_traverse: nodos con aristas + `TRAVERSE ... DEPTH`, ejecutable con `cargo run --example graph_traverse`" }
  - { id: FR-0087-04, desc: "guía de API para terceros `docs/api_guide.md` que enlaza y explica cada ejemplo con su comando verificable" }
nf:
  - { id: NF-0087-01, desc: "cada ejemplo es determinista, sin red y usa un directorio temporal propio (no toca el directorio de trabajo)" }
  - { id: NF-0087-02, desc: "los ejemplos se EJECUTAN en CI como binarios reales desde el test de humo (`cargo run --example` = `main` → `run()`), no solo compilan" }
  - { id: NF-0087-03, desc: "docstrings ES en `run()`; `main` sin `unwrap`/`expect` (exit code 1 + mensaje WHAT+WHY+WHERE)" }
acceptance_criteria:
  - id: AC-0087-01
    given: el ejemplo quickstart compilado (binario del profile actual)
    when: el test de humo lo ejecuta
    then: hace DML completo (INSERT, SELECT con WHERE, UPDATE, DELETE) y su salida contiene las filas esperadas
    test: test_ac_0087_01_quickstart_runs_full_dml_flow
  - id: AC-0087-02
    given: el ejemplo vector_search compilado
    when: el test de humo lo ejecuta
    then: inserta vectores, `KNN` devuelve el top-k ordenado por distancia y `MATCH` devuelve resultados BM25 esperados
    test: test_ac_0087_02_vector_search_returns_ranked_results
  - id: AC-0087-03
    given: el ejemplo graph_traverse compilado
    when: el test de humo lo ejecuta
    then: `TRAVERSE` con `DEPTH` respeta la profundidad y excluye el nodo aislado
    test: test_ac_0087_03_graph_traverse_respects_depth
  - id: AC-0087-04
    given: `docs/api_guide.md`
    when: el test verifica la guía
    then: nombra los tres ejemplos con su comando `cargo run --example ...` y documenta la API de fachada usada
    test: test_ac_0087_04_api_guide_documents_each_example
exit_criteria:
  - cargo run --example quickstart / vector_search / graph_traverse (run local)
  - cargo build --examples -p ruscadb && cargo test -p ruscadb --test examples_smoke
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - python scripts/check_architecture.py
  - cargo xtask trace / contract verdes
rollback:
  - borrar crates/ruscadb/examples/, tests/examples_smoke.rs y docs/api_guide.md (sin estado persistido: los ejemplos solo escriben en temporales)
sandbox:
  - cargo build --examples -p ruscadb && cargo test -p ruscadb --test examples_smoke
---

# SPEC-0087 — Ejemplos ejecutables + guía de API (Ola 4)

## Contexto

La auditoría *readiness* y el análisis de flujos (2026-10-13): RuscaDB no
tiene ningún `examples/` ni guía de API para terceros — la única vía de
aprender la fachada es leer los tests internos. La frontera (docs de
embedding/SQLite/DuckDB) exige **ejemplos ejecutables que CI ejecuta**,
no snippets que se pudren. Cada ejemplo expone su lógica como `run()`
para que el test de humo la ejecute de verdad (anti-patrón "ejemplo que
solo compila").

## Superficie

- `crates/ruscadb/examples/{quickstart,vector_search,graph_traverse}.rs`
  — sobre la fachada `ruscadb` (sin tocar crates internos).
- `crates/ruscadb/tests/examples_smoke.rs` — localiza y **ejecuta los
  binarios** de los ejemplos (mismo artefacto que `cargo run --example`)
  y valida sus salidas en CI.

## Notas de diseño

- Sintaxis RQL verificada contra tests existentes: `SELECT ... WHERE`,
  `INSERT INTO ... VALUES`, `UPDATE ... SET ... WHERE`, `DELETE FROM`,
  `KNN embedding <|k|> [...]`, `WHERE MATCH(col, 't')`,
  `TRAVERSE edges DEPTH n`.
- **Subproceso en vez de `include!`**: incluir los ejemplos en el test
  obligaría a quitar los `//!` (crate docs) que exige `missing_docs`
  con `-D warnings`, o a rasurarlos antes del include. Ejecutar el
  binario real además prueba el `main` (exit code, manejo de errores) —
  exactamente lo que hace un tercero al clonar el repo.
- Los ejemplos usan `tempfile` (dev-dependency ya presente) y cierran
  la base antes de limpiar el temporal.
- `main` delega en `run()` y maneja el `Result` con `eprintln!` +
  `exit(1)`: sin `unwrap`/`expect` (lints del workspace).
