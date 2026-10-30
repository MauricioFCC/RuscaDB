---
id: SPEC-0136
feature: lance_format
status: implemented
maturity: stub
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-multimodal]
  files: [crates/ruscadb/src/lance_format.rs]
  out_of_scope: [distributed, clustering, full Lance spec]
fr:
  - { id: FR-0136-01, desc: "feature flag `lance-format` (off por defecto) habilita integracion con Lance" }
  - { id: FR-0136-02, desc: "LanceTable soporta append de filas columnares y scan" }
  - { id: FR-0136-03, desc: "Blob V2: storage semantics inline/packed/dedicated/external" }
nf:
  - { id: NF-0136-01, desc: "cero dependencias nuevas en el build por defecto; lance crate opcional" }
acceptance_criteria:
  - id: AC-0136-01
    given: la feature `lance-format` activada
    when: se crea una LanceTable y se anaden filas
    then: el scan devuelve las filas en orden
    test: test_ac_0136_01_lance_append_scan
exit_criteria:
  - cargo test -p ruscadb --all-features --test lance_format
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0136 â€” IntegraciÃ³n con Lance format (multimodal columnar)

## Contexto

Frontier 2026 (LanceDB, Lance format v2.2): formato columnar para datos multimodales
con Blob V2 (inline/packed/dedicated/external), 50%+ compresiÃ³n, lazy reads.
RuscaDB tiene blob store CAS y columnar en memoria. Esta spec aÃ±ade integraciÃ³n
con Lance detrÃ¡s de un feature flag.

## Superficie

- Feature `lance-format` en `ruscadb` (off por defecto).
- Dependencia opcional `lance = "0.42"` (o `lance-core`).
- `crates/ruscadb/src/lance_format.rs` â€” `LanceTable` con append + scan.
- IntegraciÃ³n futura: parquet export, vector search, full-text search.

## Estado del arte

- **Lance** (lance.org): formato columnar para multimodal AI, Blob V2, IVF-HNSW,
  full-text search, feature engineering, ACID transactions, time travel.
- **LanceDB**: embedded, no service required, stores embeddings/metadata/indexes
  together in a single table.
- **RuscaDB actual**: blob store CAS (SHA-256, dedup + refcount), columnar en
  memoria (SPEC-0110), Arrow columnar (SPEC-0124). No hay formato Lance.

## DiseÃ±o propuesto

1. **LanceTable**: wrapper sobre lance crate con append + scan.
2. **Blob V2**: storage semantics (inline/packed/dedicated/external).
3. **Vector search**: IVF-HNSW sobre columnas de embeddings.
4. **Full-text search**: integraciÃ³n con Ã­ndice invertido.
5. **Time travel**: versioning por commit.

## Criterios de aceptaciÃ³n

- **AC-0136-01** â€” append + scan: las filas se devuelven en orden.

## Notas

- ImplementaciÃ³n inicial: stub sin dependencia de lance crate.
- IntegraciÃ³n real con lance queda como trabajo futuro.
- ADR requerido: `docs/adr/ADR-0136-lance-format.md`.
---
