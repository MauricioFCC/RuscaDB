---
id: SPEC-0012
feature: query_execution
status: accepted
owner: query-team
appetite_days: 10
boundaries:
  crates: [ruscadb, ruscadb-query, ruscadb-core]
  out_of_scope: [DataFusion, JOIN, GROUP BY, agregaciones, B-tree, MVCC, vector/graph]
fr:
  - { id: FR-0012-01, desc: "catalogo persistente de tablas (nombre, columnas, paginas) en pagina 0 + extension" }
  - { id: FR-0012-02, desc: "heap de filas con paginas ranuradas (slotted pages) y localizador (PageId, slot)" }
  - { id: FR-0012-03, desc: "indice secundario persistente de una columna por tabla con busqueda Eq" }
  - { id: FR-0012-04, desc: "planificador FullScan vs IndexScan ante filtro col = literal" }
  - { id: FR-0012-05, desc: "ejecutor SELECT con filtro, proyeccion y LIMIT sobre Database" }
  - { id: FR-0012-06, desc: "insert auto-commit con validacion de esquema y mantenimiento del indice" }
nf:
  - { id: NF-0012-01, desc: "catalogo + filas sobreviven a reopen (durabilidad heredada del WAL)" }
  - { id: NF-0012-02, desc: "comparaciones estrictas: tipos incompatibles devuelven TypeMismatch, NULL excluye la fila" }
acceptance_criteria:
  - id: AC-0012-01
    given: una base nueva
    when: se crea la tabla t(a INT, b TEXT), se insertan 3 filas y se ejecuta "SELECT * FROM t"
    then: devuelve las 3 filas con sus escalares
    test: test_ac_0012_01_create_insert_select_all
  - id: AC-0012-02
    given: la tabla t con filas
    when: se ejecuta "SELECT a FROM t WHERE b = 'x' AND a > 1 LIMIT 1"
    then: devuelve solo la fila coincidente proyectada y limitada
    test: test_ac_0012_02_where_projection_limit
  - id: AC-0012-03
    given: la tabla t con indice en a
    when: se planifica "SELECT * FROM t WHERE a = 2"
    then: el plan es IndexScan y devuelve las mismas filas que FullScan
    test: test_ac_0012_03_index_scan_matches_full_scan
  - id: AC-0012-04
    given: una base con tablas y filas confirmadas
    when: se cierra y se reabre
    then: catalogo, filas e indice siguen disponibles y consultables
    test: test_ac_0012_04_catalog_survives_reopen
  - id: AC-0012-05
    given: la tabla t(a INT)
    when: se consulta una tabla inexistente o se inserta un tipo incompatible
    then: devuelve TableNotFound / TypeMismatch accionables
    test: test_ac_0012_05_schema_errors_are_actionable
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0012
  - cargo mutants -p ruscadb mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir los modulos catalog/heap/index/executor de la fachada
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0012 — Ejecución end-to-end (catálogo + heap + índice + SELECT)

## Contexto

Primera ruta vertical completa de RuscaDB: del texto RQL a las filas
(`docs/RuscaDB-roadmap.md` §4 y ADR-001/ADR-007). El parser (SPEC-0005)
produce el IR; esta spec añade el resto dentro de la fachada `ruscadb`
(composition root: sin cambios en el guard de arquitectura):

- `catalog.rs` — `Catalog` + `TableDef { name, columns, heap_start, row_count, index }`,
  persistido con `postcard` desde la **página 0** (superblock: magic u64
  distinto de cualquier otro, versión, nº de páginas del catálogo,
  siguiente página libre con bump allocator documentado; sin free-list).
- `heap.rs` — páginas ranuradas: `[slot_count u16 LE | slots (offset u16, len u16)* | filas]`;
  cada fila = `postcard(Record)` con solo `scalars` poblados; localizador `(PageId, slot)`.
- `index.rs` — índice secundario persistente de **una columna por tabla**:
  array ordenado de `(key_bytes, localizador)` en páginas propias, mantenido
  en cada insert; `key_bytes` = codificación canónica que preserva el orden;
  búsqueda `Eq` por búsqueda binaria.
- `executor.rs` — `Database::execute(&str) -> Vec<Row>` (`Row = BTreeMap<String, ScalarValue>`):
  parse → resolución en catálogo → plan (`IndexScan` si el filtro contiene
  `col = literal` sobre columna indexada, si no `FullScan`) → evaluación del
  filtro con coerción numérica Int↔Float → proyección → LIMIT.
- `Database::create_table`, `Database::insert` (auto-commit documentado),
  `Database::create_index`; nuevas variantes de error en `ruscadb-core`:
  `TableNotFound { table }`, `ColumnNotFound { column }`, `TypeMismatch { .. }`.

## Criterios de aceptación

- **AC-0012-01/02** — ruta completa crear → insertar → consultar con filtro/proyección/LIMIT.
- **AC-0012-03** — el plan usa el índice y coincide con el full scan.
- **AC-0012-04** — durabilidad: todo sobrevive a reopen vía WAL-first.
- **AC-0012-05** — errores de esquema accionables (WHAT+WHERE).

## Trazabilidad

Tests `test_ac_0012_<nn>_*` en `crates/ruscadb` + proptest de roundtrip
insert→select; verificado por `cargo xtask trace`.
