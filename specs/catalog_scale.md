---
id: SPEC-0086
feature: catalog_scale
status: implemented
owner: catalog-team
appetite_days: 3
boundaries:
  crates: [ruscadb]
  files: [crates/ruscadb/src/catalog.rs, crates/ruscadb/tests/catalog_scale.rs]
  out_of_scope: [partición del catálogo en fichero aparte, caché de catálogo en memoria, migración de formatos previos a SPEC-0071, límite de tablas por aplicación]
fr:
  - { id: FR-0086-01, desc: "el catálogo supera las 16 páginas reservadas pagando sus páginas extra con el allocator del heap (patrón overflow de SPEC-0071), sin techo de 64 KiB práctico" }
  - { id: FR-0086-02, desc: "las páginas del catálogo no colisionan con las del heap: allocate del heap nunca devuelve una página del catálogo ni al revés" }
  - { id: FR-0086-03, desc: "el formato existente (catálogo ≤ 16 páginas, SPEC-0031) sigue cargando sin migración: compatible hacia atrás" }
nf:
  - { id: NF-0086-01, desc: "sin dependencias nuevas; el techo residual (si lo hay) es de páginas u32 y su error es actionable, no un panic" }
  - { id: NF-0086-02, desc: "el bump `next_page` y la persistencia siguen siendo el único mecanismo (KISS: sin free-list de catálogo en esta spec)" }
  - { id: NF-0086-03, desc: "docstrings ES-UTF8; sin unwrap en producción; errores WHAT+WHY+WHERE" }
acceptance_criteria:
  - id: AC-0086-01
    given: una base donde se crean suficientes tablas para que el catálogo supere 16 páginas
    when: se persiste, se cierra y se reabre
    then: todas las tablas y columnas se recargan legibles (payload completo)
    test: test_ac_0086_01_catalog_grows_beyond_reserved_pages
  - id: AC-0086-02
    given: un catálogo ampliado conviviendo con un heap con filas
    when: se insertan filas tras el crecimiento del catálogo
    then: las filas viven en páginas de heap y ninguna página del catálogo se pisa (integridad verificada al releer ambas)
    test: test_ac_0086_02_heap_and_catalog_pages_never_collide
  - id: AC-0086-03
    given: un catálogo en el formato antiguo (≤ 16 páginas)
    when: se abre con el código nuevo
    then: carga sin migración y sin error (compatibilidad hacia atrás)
    test: test_ac_0086_03_legacy_catalog_within_reserved_pages_loads
exit_criteria:
  - cargo test -p ruscadb --test catalog_scale
  - cargo test -p ruscadb   (suite completa, sin regresión)
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - volver al check `pages > CATALOG_RESERVED_PAGES` de error (SPEC-0031)
sandbox:
  - cargo test -p ruscadb --test catalog_scale
---

# SPEC-0086 — Catálogo sin techo de 64 KiB (Ola 2)

## Contexto

Deuda §MVP (línea 79): el catálogo está **limitado a la región reservada de
16 páginas = 64 KiB** (`CATALOG_RESERVED_PAGES = 16` en `catalog.rs`); al
superarlo `Catalog::save` devuelve error «el catálogo ocupa N páginas y
supera la región reservada de 16 (reduce tablas/columnas)». Cualquier esquema
razonable con ~cientos de tablas/columnas lo alcanza.

Mecanismo reutilizable ya en el repo: **SPEC-0071 (`heap_overflow_pages.md`)**
añadió páginas de overflow para el heap. El catálogo aplica el mismo patrón:
pago de páginas extra con el allocator `next_page` compartido, sin nuevo
fichero ni formato de página distinto.

## Superficie

- `crates/ruscadb/src/catalog.rs` (único fichero de código de la spec):
  persistencia con payload paginado más allá de `CATALOG_RESERVED_PAGES`
  (carga de páginas extra, punteros/orden según el patrón overflow existente),
  `next_page` coherente con el heap, quitar el techo de 16 con un techo
  residual alto y actionable.
- Test: `crates/ruscadb/tests/catalog_scale.rs`.
- Reutiliza leyendo `heap.rs`/`specs/heap_overflow_pages.md` como patrón; si
  necesita tocar `heap.rs`, es conflicto con SPEC-0085 — reportarlo.

## Criterios de aceptación

- **AC-0086-01** — catálogo > 16 páginas persiste y recarga completo.
- **AC-0086-02** — sin colisión catálogo/heap (ambos releídos intactos).
- **AC-0086-03** — formato ≤ 16 páginas sigue cargando (compat).

## Trazabilidad

Tests `test_ac_0086_<nn>_*` con marcador `@spec AC-0086-NN`; verificado por
`cargo xtask trace`/`contract`.
