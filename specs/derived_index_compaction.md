---
id: SPEC-0101
feature: derived_index_compaction
status: implemented
owner: storage-team
appetite_days: 3
boundaries:
  crates: [ruscadb]
  files: [crates/ruscadb/src/indexes.rs, crates/ruscadb/src/gc.rs, crates/ruscadb/src/database.rs, crates/ruscadb/tests/derived_index_compaction.rs]
  out_of_scope: [formato de índice persistente, compactación incremental por segmento, GC del WAL, shrink del fichero, cambio de la API pública existente]
fr:
  - { id: FR-0101-01, desc: "Database::compact_derived_indexes reconstruye los índices derivados (HNSW vectorial, CSR de grafo e invertido/FTS) de cada tabla desde las filas vivas del heap" }
  - { id: FR-0101-02, desc: "reap() cablea la compactación al final de la purga física, de modo que las filas purgadas no dejan entradas huérfanas en los índices derivados" }
  - { id: FR-0101-03, desc: "la reconstrucción reutiliza rebuild_indexes/rebuild_primary_index (SPEC-0019/0024) sin duplicar la lógica" }
nf:
  - { id: NF-0101-01, desc: "cero regresión: tras compactar, SELECT/KNN/TRAVERSE/MATCH sobre filas vivas devuelven resultados idénticos a antes del reap" }
  - { id: NF-0101-02, desc: "idempotencia: compact_derived_indexes repetido no cambia resultados ni conteos; reap() repetido es no-op" }
  - { id: NF-0101-03, desc: "sin dependencias nuevas; docstrings ES-UTF8; sin unwrap en producción" }
acceptance_criteria:
  - id: AC-0101-01
    given: una tabla con filas vectoriales, de texto y con aristas, una de ellas borrada y purgada por reap()
    when: se reconstruyen los índices derivados desde las filas vivas
    then: KNN/MATCH/TRAVERSE devuelven solo filas vivas y los conteos de vectores/aristas/postings de las filas purgadas desaparecen
    test: test_ac_0101_01_reap_rebuilds_derived_indexes_from_live_rows
  - id: AC-0101-02
    given: una base con filas vivas cuyos índices derivados ya se reconstruyeron
    when: se llama compact_derived_indexes dos veces
    then: los resultados de SELECT/KNN/TRAVERSE/MATCH y los conteos son idénticos (idempotente)
    test: test_ac_0101_02_compaction_is_idempotent
  - id: AC-0101-03
    given: una base con filas vivas y ninguna fila borrada
    when: se ejecuta reap()
    then: no se purga nada y los resultados de SELECT/KNN/TRAVERSE/MATCH no cambian
    test: test_ac_0101_03_reap_without_deletes_preserves_results
  - id: AC-0101-04
    given: una base con filas vivas (vector/texto/aristas) sin borrados
    when: se llama compact_derived_indexes
    then: los resultados de SELECT/KNN/TRAVERSE/MATCH y los conteos son idénticos (cero regresión)
    test: test_ac_0101_04_compaction_preserves_live_results
exit_criteria:
  - cargo test -p ruscadb --test derived_index_compaction
  - cargo test -p ruscadb --all-features   (sin regresión)
  - cargo clippy -p ruscadb --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar Database::compact_derived_indexes y su llamada en reap(); los índices derivados vuelven a mantenerse por inserción/borrado lógico (tombstones) sin rebuild, dejando los huérfanos del reap previos
sandbox:
  - cargo test -p ruscadb --test derived_index_compaction
---

# SPEC-0101 — Compactación de índices derivados tras el reap

## Contexto

`Database::reap` (SPEC-0085) purga físicamente del heap las versiones obsoletas y
libera sus páginas a la free-list. Los índices derivados en memoria —HNSW
vectorial, CSR de grafo e invertido/FTS— son **derivados del heap** y no se
persisten (SPEC-0017): al abrir la base se reconstruyen escaneando las filas
vivas.

Antes de esta spec, la purga solo *podaba selectivamente* los postings del índice
invertido y las aristas del CSR de los `RecordId` purgados (`TableIndexes::compact`),
pero **no tocaba el índice HNSW**: los vectores de las filas purgadas
permanecían en el índice (`vector.ids`/`vector.vectors` y los nodos HNSW),
dejando entradas huérfanas que solo se ocultaban por tombstone. El grafo, además,
podía conservar un nodo huérfano cuando una fila viva tenía una arista hacia una
fila purgada.

Frontera 2026: la compactación por **rebuild desde las filas vivas** es el patrón
robusto (los índices derivados no se parchean entrada a entrada, se reconstruyen
desde la fuente de verdad), reutilizando el mismo camino que la apertura de la
base.

## Superficie

- `crates/ruscadb/src/indexes.rs`: `Database::compact_derived_indexes` (público)
  reutiliza `rebuild_indexes`/`rebuild_primary_index`; `rebuild_indexes` pasa a
  indexar en dos pasadas, añadiendo aristas del CSR solo hacia filas vivas.
- `crates/ruscadb/src/gc.rs`: `reap()` cablea la compactación al final de la
  purga; se expone `Database::vector_index_len(table)` como señal observable.
- `crates/ruscadb/tests/derived_index_compaction.rs`: tests de aceptación.
- **Fuera de alcance**: formato de índice persistente, compactación incremental
  por segmento, GC del WAL, shrink del fichero y cualquier cambio incompatible de
  la API pública.

## Criterios de aceptación

- **AC-0101-01** — reap purga y reconstruye: KNN/MATCH/TRAVERSE solo filas vivas;
  los conteos (vectores/aristas/postings) reflejan solo las filas vivas.
- **AC-0101-02** — `compact_derived_indexes` es idempotente (resultados y
  conteos idénticos al repetirlo).
- **AC-0101-03** — reap sin filas borradas no altera resultados.
- **AC-0101-04** — compactar sin borrados preserva resultados (cero regresión).

## Trazabilidad

Tests `test_ac_0101_<nn>_*` con marcador `@spec AC-0101-NN`; verificado por
`cargo xtask trace`/`contract`.
