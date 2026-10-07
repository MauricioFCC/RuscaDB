---
id: SPEC-0016
feature: ci_t3
status: accepted
owner: platform-team
appetite_days: 4
boundaries:
  crates: []
  out_of_scope: [codigo de producto, cambios en crates, DataFusion]
fr:
  - { id: FR-0016-01, desc: "workflow nightly T3 con mutation full (sharded) y umbral MS >= 85%" }
  - { id: FR-0016-02, desc: "job de fuzzing (cargo-fuzz) sobre query_parse y wal_recover" }
  - { id: FR-0016-03, desc: "job miri sobre crates sin unsafe (query, fts, core)" }
  - { id: FR-0016-04, desc: "config de cargo-mutants (.cargo/mutants.toml) con exclude/toolchain" }
  - { id: FR-0016-05, desc: "deny.toml revisado (advisories/licenses/bans/sources) y documentado" }
  - { id: FR-0016-06, desc: "documentar T1/T2/T3 y como correr cada gate localmente en docs/" }
nf:
  - { id: NF-0016-01, desc: "T1 sigue < 90 s; T3 es nightly/alert-only (no bloquea PR)" }
  - { id: NF-0016-02, desc: "YAML valido (parseable) y sin secretos" }
acceptance_criteria:
  - id: AC-0016-01
    given: el repo
    when: se valida la sintaxis del workflow nightly
    then: es YAML valido con jobs mutation/fuzz/miri
    test: test_ac_0016_01_nightly_workflow_is_valid
  - id: AC-0016-02
    given: la config de cargo-mutants
    when: se parsea
    then: define exclude y toolchain sin romper T1
    test: test_ac_0016_02_mutants_config_is_valid
  - id: AC-0016-03
    given: deny.toml
    when: se valida
    then: declara secciones advisories/licenses/bans/sources
    test: test_ac_0016_03_deny_config_has_required_sections
exit_criteria:
  - python scripts/check_ci_config.py (valida YAML + secciones)
  - T1 verde sin cambios de codigo de producto
rollback:
  - revertir los workflows y configs anadidos
sandbox:
  - python scripts/check_ci_config.py
---

# SPEC-0016 — Endurecimiento de CI (T3 nightly) y supply chain

## Contexto

El roadmap §7.1 define tres tiers de gates: **T1** determinista (<90 s, ya
implementado), **T2** LLM-judge y **T3** nightly (mutation full, fuzz,
crash-recovery, miri, cross-platform). Esta spec añade la infraestructura de
**T3** y consolida la postura de supply chain (§6.5) sin tocar código de
producto.

## Criterios de aceptación

- **AC-0016-01** — workflow nightly válido (mutation/fuzz/miri).
- **AC-0016-02** — `.cargo/mutants.toml` válido.
- **AC-0016-03** — `deny.toml` con las 4 secciones requeridas.

## Trazabilidad

Verificado por `scripts/check_ci_config.py` (nuevo) y `cargo xtask trace`.
