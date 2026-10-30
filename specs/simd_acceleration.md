---
id: SPEC-0150
feature: simd_acceleration
status: implemented
maturity: stub
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-vector]
  files: [crates/ruscadb/src/simd.rs]
  out_of_scope: [GPU acceleration, SIMD intrinsics manuales]
fr:
  - { id: FR-0150-01, desc: "feature flag `simd` (off por defecto) habilita aceleracion SIMD" }
  - { id: FR-0150-02, desc: "SimdOps::dot_product calcula producto punto acelerado" }
nf:
  - { id: NF-0150-01, desc: "cero dependencias nuevas en el build por defecto" }
acceptance_criteria:
  - id: AC-0150-01
    given: la feature `simd` activada
    when: se calcula un producto punto
    then: el resultado es correcto
    test: test_ac_0150_01_dot_product
exit_criteria:
  - cargo test -p ruscadb --all-features --test simd
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0150 â€” AceleraciÃ³n SIMD para operaciones vectoriales

## Contexto

Frontier 2026 (turbopuffer, avx-arrow): SIMD acceleration para bÃºsqueda vectorial.
RuscaDB usa loops escalares. Esta spec aÃ±ade operaciones SIMD bÃ¡sicas detrÃ¡s de
un feature flag.

## Superficie

- Feature `simd` en `ruscadb` (off por defecto).
- `crates/ruscadb/src/simd.rs` â€” `SimdOps` con `dot_product`, `cosine_similarity`.
- IntegraciÃ³n futura: SIMD intrinsics, auto-vectorizaciÃ³n.

## Estado del arte

- **turbopuffer**: SIMD para merge de iteradores (6.5ms â†’ 110Î¼s).
- **avx-arrow**: 35x SIMD acceleration.
- **RuscaDB actual**: loops escalares.

## DiseÃ±o propuesto

1. **SimdOps**: operaciones vectoriales (dot product, cosine, L2 distance).
2. **Auto-vectorizaciÃ³n**: usar `#[inline]` y slices para que el compilador
   vectorice automÃ¡ticamente.
3. **Intrinsics**: SIMD intrinsics manuales (futuro, requiere `std::arch`).

## Notas

- ImplementaciÃ³n inicial: auto-vectorizaciÃ³n del compilador (sin intrinsics).
- SIMD intrinsics manuales quedan como trabajo futuro.
---
