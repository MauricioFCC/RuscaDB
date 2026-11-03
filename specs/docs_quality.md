---
id: SPEC-0199
feature: docs_quality
status: implemented
maturity: real
owner: docs-team
appetite_days: 2
boundaries:
  crates: [xtask]
  files:
    - docs/README.md
    - scripts/check_docs.py
    - crates/xtask/tests/docs_gate.rs
    - .github/workflows/ci.yml
    - AGENTS.md
  out_of_scope: [sitio estático mdBook/MkDocs, traducción a otros idiomas, spellcheck]
fr:
  - { id: FR-0199-01, desc: "indice de documentacion con roles Diataxis y fuentes canonicas sin duplicar estado" }
  - { id: FR-0199-02, desc: "gate determinista de enlaces, anclas, rutas personales, fences e indice completo" }
  - { id: FR-0199-03, desc: "el gate corre en CI y en el Gate T1 local" }
nf:
  - { id: NF-0199-01, desc: "solo stdlib de Python; O(ficheros + enlaces) por corrida" }
  - { id: NF-0199-02, desc: "errores accionables ruta:linea; exit 0/1/2 sin verdes silenciosos" }
acceptance_criteria:
  - id: AC-0199-01
    given: la documentación versionada del repositorio
    when: se ejecuta scripts/check_docs.py
    then: enlaces, rutas, fences e índice pasan con exit 0
    test: test_ac_0199_01_docs_gate_repo_clean
  - id: AC-0199-02
    given: un fixture con enlace roto, ancla rota, ruta personal y fence desbalanceado
    when: se ejecuta el gate
    then: falla con exit 1 y nombra cada incidencia
    test: test_ac_0199_02_docs_gate_rejects_issues
  - id: AC-0199-03
    given: el workflow de CI y AGENTS.md
    when: se revisa el cableado del gate
    then: ambos invocan scripts/check_docs.py
    test: test_ac_0199_03_docs_gate_wired
exit_criteria:
  - python scripts/check_docs.py
  - cargo test -p xtask --test docs_gate
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
  - python scripts/check_ci_config.py
rollback:
  - eliminar el script y el job; no toca runtime ni formato persistido
sandbox:
  - python scripts/check_docs.py --json
---
# SPEC-0199 — Calidad y arquitectura de la documentación

## Contexto

La documentación creció por ciclos (28 ficheros, roadmap de >1000 líneas) y
acumuló: duplicación de estado (roadmap/backlog/madurez), documentos de chat
pre-proyecto tratados como fuente de verdad, conteos volátiles desactualizados,
enlaces escasos (13) y rutas de máquina en ejemplos. No había un gate que
detectara enlaces rotos ni documentos no enlazados.

## Diseño

1. **Índice canónico** `docs/README.md` con roles Diátaxis (tutorial, how-to,
   reference, explanation) y tabla de fuentes únicas.
2. **Archivo histórico** `docs/archive/`: documentos de diseño superados, con
   banner que apunta a las fuentes vigentes.
3. **Gate** `scripts/check_docs.py`: enlaces relativos, rutas personales,
   fences balanceados, un H1 por documento e índice completo.
4. **Cableado** en el job `architecture` de CI y en el Gate T1 de `AGENTS.md`.
5. **Regla anti-drift**: los conteos viven en `specs-index`, madurez y
   gap-register; README/roadmap enlazan en vez de copiar.

## Complejidad

```text
gate: O(F + L)   (F ficheros, L enlaces)
memoria: O(incidencias)
```

## TDD adversarial

- Fixture temporal con enlace roto + ruta personal + fence impar.
- Repo real como oráculo de "documentación limpia".
- Test de cableado CI/AGENTS (falla si se elimina el gate).

## Riesgos

- Falsos positivos de rutas: el gate solo marca `C:\Users\<x>` y `/home/<x>/`.
- Los documentos archivados quedan fuera del chequeo de enlaces por diseño.

## Estado de implementación (2026-11-03)

- `docs/README.md`, `docs/archive/` y `scripts/check_docs.py` creados.
- Roadmap con TOC y estado canónico; README sin conteos volátiles.
- Tests AC-0199-01..03 en `crates/xtask/tests/docs_gate.rs`; gate en CI y T1.
