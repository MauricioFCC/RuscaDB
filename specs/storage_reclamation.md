---
id: SPEC-0085
feature: storage_reclamation
status: implemented
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb-storage, ruscadb, ruscadb-graph, ruscadb-fts, ruscadb-txn]
  files: [crates/ruscadb-storage/src/paged_file.rs, crates/ruscadb/src/gc.rs, crates/ruscadb/src/heap.rs, crates/ruscadb/src/index.rs, crates/ruscadb-graph/src/csr.rs, crates/ruscadb-fts/src/index.rs, crates/ruscadb-txn/src/manifest.rs, crates/ruscadb/tests/storage_reclamation.rs]
  out_of_scope: [cambio de tamaño de página, VACUUM en línea durante DML, free-list compartida entre bases, compacción del WAL, shrink del fichero (truncate a la cola) ]
fr:
  - { id: FR-0085-01, desc: "PagedFile expone free-list: allocate() reutiliza páginas liberadas antes de crecer el fichero" }
  - { id: FR-0085-02, desc: "la free-list se persiste (manifest o equivalente) y sobrevive a reapertura sin corromper datos vivos" }
  - { id: FR-0085-03, desc: "reap() devuelve al heap las páginas cuya fila quedó purgada (además de purgar filas, como hoy)" }
  - { id: FR-0085-04, desc: "GC de grafo/FTS: tras reap, las estructuras derivadas de filas purgadas se eliminan (tombstones fuera) y las consultas de filas vivas no cambian" }
nf:
  - { id: NF-0085-01, desc: "crash-safety: un crash entre la purga y la persistencia de la free-list puede perder como mucho esa corrida (páginas en fuga detectables), nunca reasignar una página viva" }
  - { id: NF-0085-02, desc: "idempotencia: reap() repetido sin cambios no altera la free-list (write-ahead thinking, sin doble free)" }
  - { id: NF-0085-03, desc: "sin dependencias nuevas; docstrings ES-UTF8; sin unwrap en producción" }
acceptance_criteria:
  - id: AC-0085-01
    given: una base con filas insertadas y posteriormente purgadas por reap()
    when: se vuelve a insertar (allocate)
    then: se reutilizan páginas libres y page_count() no crece respecto del pico previo
    test: test_ac_0085_01_allocate_reuses_freed_pages
  - id: AC-0085-02
    given: una base con free-list tras reap
    when: se cierra y reabre
    then: la free-list persiste, allocate sigue reutilizando y todas las filas vivas se leen intactas
    test: test_ac_0085_02_freelist_survives_reopen
  - id: AC-0085-03
    given: filas con aristas (grafo) y términos (FTS) purgadas por reap
    when: se ejecuta el GC de derivados y después KNN/TRAVERSE/MATCH sobre filas vivas
    then: las estructuras huérfanas se eliminan y los resultados de filas vivas son idénticos a antes del GC
    test: test_ac_0085_03_derived_indexes_compact_without_losing_live_results
exit_criteria:
  - cargo test -p ruscadb --test storage_reclamation
  - cargo test -p ruscadb-storage -p ruscadb-graph -p ruscadb-fts
  - cargo test --workspace --all-features   (sin regresión)
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar free-list (allocate vuelve a solo-append) y el GC de derivados (siguen los tombstones lógicos)
sandbox:
  - cargo test -p ruscadb --test storage_reclamation
---

# SPEC-0085 — Reclamación de storage: free-list + GC derivado (Ola 2)

## Contexto

Deuda §MVP y auditoría *readiness*: `PagedFile::allocate` es **append-only**
(`paged_file.rs` — sin free-list), `reap()` purga filas obsoletas pero sus
páginas nunca vuelven al allocator (el fichero solo crece), y grafo/FTS
"ya excluyen" filas borradas vía tombstones **sin compactar nunca**
(`gc.rs`, `MVP.md` §borrado lógico): la base acumula huérfanos para siempre.

Frontera 2026 (§3): free-list persistente con reutilización (patrón
SQLite freelist / redb), y GC de derivados por rebuild selectivo — el CSR y
el índice invertido admiten reconstrucción por segmento sin tocar filas vivas.

## Superficie

- `crates/ruscadb-storage/src/paged_file.rs`: free-list (allocate reutiliza;
  `free_page(id)`/API equivalente) persistida (diseño del agente: manifest
  `ruscadb-txn/src/manifest.json` u otra pieza ya existente — sin formato
  nuevo incompatible).
- `crates/ruscadb/src/gc.rs` + `heap.rs` + `index.rs`: `reap()` libera las
  páginas de las filas que purga (hoy solo `heap_remove`).
- `crates/ruscadb-graph/src/csr.rs` y `crates/ruscadb-fts/src/index.rs`: GC
  de aristas/postings huérfanos (rebuild/compact selectivo) cableado desde
  `reap()`.
- Test: `crates/ruscadb/tests/storage_reclamation.rs`.

## Criterios de aceptación

- **AC-0085-01** — allocate reutiliza páginas purgadas (page_count estable).
- **AC-0085-02** — la free-list sobrevive a reapertura con filas intactas.
- **AC-0085-03** — GC de grafo/FTS sin cambiar resultados de filas vivas.

## Trazabilidad

Tests `test_ac_0085_<nn>_*` con marcador `@spec AC-0085-NN`; verificado por
`cargo xtask trace`/`contract`.
