---
id: SPEC-0147
feature: ivf_clustering
status: implemented
maturity: real
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-vector]
  files: [crates/ruscadb/src/ivf_clustering.rs]
  out_of_scope: [GPU acceleration, distributed search]
fr:
  - { id: FR-0147-01, desc: "feature flag `ivf-clustering` (off por defecto) habilita IVF clustering" }
  - { id: FR-0147-02, desc: "IvfClusterer::cluster agrupa vectores en centroides" }
nf:
  - { id: NF-0147-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0147-01
    given: la feature `ivf-clustering` activada
    when: se clusterizan vectores
    then: el resultado tiene centroides y asignaciones
    test: test_ac_0147_01_ivf_cluster
exit_criteria:
  - cargo test -p ruscadb --all-features --test ivf_clustering
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0147 — IVF clustering para búsqueda vectorial

## Contexto

Frontier 2026 (LanceDB, FAISS): IVF (Inverted File) clustering agrupa vectores
en centroides para búsqueda eficiente. RuscaDB tiene HNSW (SPEC-0006) e IVF-PQ
stub (SPEC-0125). Esta spec añade IVF clustering detrás de un feature flag.

## Superficie

- Feature `ivf-clustering` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/ivf_clustering.rs` — `IvfClusterer` con `cluster`.
- Integración futura: IVF-HNSW, IVF-PQ con clustering real.

## Estado del arte

- **FAISS**: IVF con k-means clustering, nprobe para recall.
- **LanceDB**: IVF-PQ con clustering automático.
- **RuscaDB actual**: HNSW puro, IVF-PQ stub (sin clustering).

## Diseño propuesto

1. **IvfClusterer**: k-means clustering de vectores en centroides.
2. **Asignación**: cada vector se asigna al centroide más cercano.
3. **Búsqueda**: busca en los nprobe centroides más cercanos.

## Notas

- Implementación inicial: k-means simplificado (sin optimización).
- Clustering real (k-means++, mini-batch) queda como trabajo futuro.
---
