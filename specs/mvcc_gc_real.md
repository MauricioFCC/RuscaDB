---
id: SPEC-0169
feature: mvcc_gc_real
status: implemented
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-txn]
  files: [crates/ruscadb-txn/src/mvcc.rs, crates/ruscadb/src/rcu.rs]
  out_of_scope: [distributed GC, background thread]
fr:
  - { id: FR-0169-01, desc: "epoch-based reclamation real con atomics" }
  - { id: FR-0169-02, desc: "GC de versiones obsoletas por low_watermark" }
  - { id: FR-0169-03, desc: "reclaim de paginas CoW tras sincronizacion" }
  - { id: FR-0169-04, desc: "metricas de versiones vivas/muertas" }
nf:
  - { id: NF-0169-01, desc: "cero dependencias nuevas" }
  - { id: NF-0169-02, desc: "lectores no bloquean al escritor" }
acceptance_criteria:
  - id: AC-0169-01
    given: un escritor confirma y el lector sale del epoch
    when: se ejecuta GC
    then: las versiones obsoletas se eliminan
    test: test_ac_0169_01_gc
  - id: AC-0169-02
    given: un lector activo en epoch anterior
    when: el escritor intenta reclaim
    then: las paginas no se liberan hasta que el lector salga
    test: test_ac_0169_02_reader_blocks
  - id: AC-0169-03
    given: metricas habilitadas
    when: se consulta
    then: se reportan versiones vivas y muertas
    test: test_ac_0169_03_metrics
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-txn --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0169 — MVCC GC + epoch-based reclamation real

## Contexto

RuscaDB tiene MVCC con `gc()`/`dead_versions()` y un stub RCU (SPEC-0146).
Esta spec implementa reclaim real con atomics y sincronizacion.

## Superficie

- `mvcc.rs`: `EpochGuard`, `RcuDomain` con `AtomicU64`, `synchronize()` con
  espera a quiescent state.
- `rcu.rs`: implementacion real (reemplaza stub).

## Estado del arte

- **Linux kernel RCU**: readers no bloquean; writers sincronizan.
- **redb**: epoch-based reclamation para paginas CoW.
- **HyPer/Umbra**: MVCC con GC por watermark + version chains.
- **RuscaDB actual**: stub (SPEC-0146) + `gc()` lineal.

## Diseño propuesto

1. **Epoch**: `AtomicU64`; lector captura epoch al entrar (guard).
2. **Writer**: incrementa epoch global; espera a que todos los guards del epoch
   anterior se liberen (quiescent state) antes de reclaim.
3. **Reclaim**: libera versiones/paginas con epoch < low_watermark.

## Riesgos

- Memory ordering: usar `SeqCst` inicialmente; optimizar con Acquire/Release.

## ADR requerido

`docs/adr/ADR-0169-mvcc-gc-rcu.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\mvcc_gc_real.md