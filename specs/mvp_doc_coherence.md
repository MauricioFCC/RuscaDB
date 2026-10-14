---
id: SPEC-0095
feature: mvp_doc_coherence
status: implemented
owner: product-team
appetite_days: 1
boundaries:
  crates: [ruscadb]
  files: [docs/MVP.md, crates/ruscadb/tests/mvp_doc_coherence.rs]
  out_of_scope: [cambiar la API o el comportamiento del motor, docs/README publicos en ingles, roadmap]
fr:
  - { id: FR-0095-01, desc: "docs/MVP.md refleja las capacidades/limites vigentes tras las Olas 0-6 (lock cross-proceso, FsyncPolicy, update, catalogo sin techo, GC de derivados, keystore con rotacion, cache persistente)" }
  - { id: FR-0095-02, desc: "guard determinista que exige los conceptos clave del MVP en el doc y prohibe los limites ya cerrados, para evitar drift silencioso" }
nf:
  - { id: NF-0095-01, desc: "solo stdlib/lectura de fichero en el test; sin dependencias nuevas y sin tocar el motor" }
  - { id: NF-0095-02, desc: "el doc es publico y mantenido a mano; el guard falla ruidosamente si pierde una capacidad o reintroduce un limite superado" }
acceptance_criteria:
  - id: AC-0095-01
    given: docs/MVP.md
    when: el guard lo inspecciona
    then: menciona lock cross-proceso, FsyncPolicy/group commit, Database::update, catalogo sin techo (overflow), GC de derivados (reap) y keystore con rotacion
    test: test_ac_0095_01_mvp_doc_lists_current_capabilities
  - id: AC-0095-02
    given: docs/MVP.md
    when: el guard lo inspecciona
    then: no afirma los limites ya superados (techo de 64 KiB, ausencia de rotacion de claves, ausencia de GC de derivados)
    test: test_ac_0095_02_mvp_doc_drops_stale_limits
exit_criteria:
  - cargo test -p ruscadb --test mvp_doc_coherence
  - cargo test -p ruscadb --test mvp  (sin regresion)
  - cargo xtask trace / contract verdes
rollback:
  - revertir docs/MVP.md y retirar crates/ruscadb/tests/mvp_doc_coherence.rs
sandbox:
  - cargo test -p ruscadb --test mvp_doc_coherence
---

# SPEC-0095 — Coherencia de docs/MVP.md con el estado real

## Contexto

`docs/MVP.md` (SPEC-0031) define el producto mínimo viable y forma parte del
entregable de release, pero sus secciones «Capacidades» y «Límites» quedaron
desfasadas por specs posteriores: el catálogo ya no tiene el techo de 64 KiB
(SPEC-0086), existe GC de índices derivados desde `reap` (SPEC-0085), hay
keystore KMS-lite con rotación (SPEC-0088), lock cross-proceso (SPEC-0076),
group commit con `FsyncPolicy` (SPEC-0081), `Database::update` (SPEC-0091) y
caché semántico persistente (SPEC-0093). Un MVP con documentación que afirma
límites falsos no es apto para terceros.

Esta spec actualiza el doc y añade un **guard** que fija el contrato: el doc
debe citar las capacidades clave y no reintroducir límites superados.

## Superficie

- `docs/MVP.md`: capacidades/límites/API alineados con el estado real.
- `crates/ruscadb/tests/mvp_doc_coherence.rs`: guard de contenido (AC-0095-01/02).

## Criterios de aceptación

- **AC-0095-01** — el doc cita las capacidades vigentes clave.
- **AC-0095-02** — el doc no afirma los límites ya superados.

## Trazabilidad

Tests `test_ac_0095_<nn>_*` con marcador `@spec AC-0095-NN`; verificado por
`cargo xtask trace`/`contract`.
