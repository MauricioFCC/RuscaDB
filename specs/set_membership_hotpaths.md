---
id: SPEC-0194
feature: set_membership_hotpaths
status: proposed
maturity: stub
owner: storage-team
appetite_days: 2
boundaries:
  crates: [ruscadb, ruscadb-storage]
  files:
    - crates/ruscadb/src/gc.rs
    - crates/ruscadb-storage/src/paged_file.rs
    - crates/ruscadb/src/columnar.rs
    - benches/facade_smoke.rs
  out_of_scope: [rediseño del free-list, compactación de páginas]
fr:
  - { id: FR-0194-01, desc: "gc de páginas reclamadas usa BTreeSet y evita O(P²)" }
  - { id: FR-0194-02, desc: "PagedFile::seed_free_pages deduplica con BTreeSet y evita O(N²)" }
  - { id: FR-0194-03, desc: "ColumnarTable::from_bytes valida duplicados con HashSet y evita O(C²)" }
nf:
  - { id: NF-0194-01, desc: "cero dependencias nuevas; std::collections" }
  - { id: NF-0194-02, desc: "O(n log n) u O(n) en los tres hot paths; memoria O(n)" }
acceptance_criteria:
  - id: AC-0194-01
    given: una tabla con P páginas reclamadas
    when: GC filtra las páginas del catálogo
    then: usa pertenencia O(log P) y el resultado es idéntico al oráculo actual
    test: test_ac_0194_01_gc_pages_set
  - id: AC-0194-02
    given: N páginas liberadas persistidas
    when: PagedFile::seed_free_pages las inyecta
    then: no hay dobles free y el coste es O(N log N)
    test: test_ac_0194_02_free_list_set
  - id: AC-0194-03
    given: un payload columnar con C columnas
    when: se valida el esquema
    then: los duplicados se detectan en O(C) y se conserva el error accionable
    test: test_ac_0194_03_columnar_set
  - id: AC-0194-04
    given: el oráculo congelado (Vec) y la implementación con set
    when: se comparan N/2N/4N operaciones
    then: los resultados son equivalentes y el escalado deja de ser cuadrático
    test: test_ac_0194_04_equivalence_gate
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-storage --all-features
  - cargo bench -p ruscadb --bench facade_smoke -- --test
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - revertir el commit; los sets son internos y no cambian el formato persistido
sandbox:
  - cargo test -p ruscadb --lib gc::tests
  - cargo test -p ruscadb-storage --lib paged_file::tests
---

# SPEC-0194 — Pertenencia en sets para hot paths de storage

## Contexto

Tres rutas usan `Vec::contains` dentro de un bucle, con coste cuadrático:

```text
gc.rs:           definition.pages.retain(|raw| !pages.contains(raw))
paged_file.rs:   if !self.free.iter().any(|id| id.0 == *raw) { ... }
columnar.rs:     if columns.iter().any(|existing| existing == &name) { ... }
```

En bases con muchas páginas/columnas esto domina el tiempo de reclaim o de
carga. La sustitución por `BTreeSet`/`HashSet` preserva la semántica y baja la
complejidad.

## Diseño

1. `gc`: construir `BTreeSet<u64>` de páginas reclamadas y usarlo en `retain`.
2. `seed_free_pages`: `BTreeSet<u64>` de ids ya libres; insertar solo nuevos.
3. `columnar`: `HashSet<&str>` para detectar duplicados; el mensaje de error no
   cambia.

Invariantes: mismo conjunto de páginas libres, mismo orden de `free` si el
formato lo exige, mismos errores de columna duplicada.

## Complejidad

```text
gc:              O(P²)      -> O(P log P)
seed_free_pages: O(N²)      -> O(N log N)
columnar:        O(C²)      -> O(C)   (HashSet)
memoria:         O(P)/O(N)/O(C)
```

## TDD adversarial

- Tests de equivalencia contra la implementación `Vec` congelada para N/2N/4N.
- Test de doble free idempotente.
- Test de duplicado columnar con mensaje exacto.
- Bench `facade_smoke` (o `criterion` dedicado) para verificar el escalado; el
  gate de tiempo es relativo (N/2N/4N), no un umbral absoluto.

## Riesgos

- `BTreeSet` ordena; si el orden de `free`/`pages` es observable, se conserva
  el orden de inserción con un `Vec` + `BTreeSet` de pertenencia.
- `HashSet` no es determinista en iteración; solo se usa para pertenencia, no
  para serializar.
