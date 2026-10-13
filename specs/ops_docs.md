---
id: SPEC-0084
feature: ops_docs
status: implemented
owner: docs-team
appetite_days: 1
boundaries:
  crates: [ruscadb]
  files: [SECURITY.md, docs/operacion.md, README.md, crates/ruscadb/tests/ops_docs.rs]
  out_of_scope: [runbooks de CI/CD (SWARMIND), SLOs de runtime (SPEC-0082), traducción al inglés de docs de operación]
fr:
  - { id: FR-0084-01, desc: "docs/operacion.md existe con runbook: abrir base (lock DatabaseInUse), migración (SchemaVersionTooNew/MigrationFailed), backup en frío, rotación de claves y troubleshooting" }
  - { id: FR-0084-02, desc: "SECURITY.md solo afirma mecanismos realmente presentes en el repo (se audita cada afirmación: BLAKE3, cargo-vet, gitleaks…)" }
  - { id: FR-0084-03, desc: "README.md enlaza docs/operacion.md desde la sección de operación/soporte" }
nf:
  - { id: NF-0084-01, desc: "docs en español-UTF8; comandos copiables y verificables contra el estado real del repo" }
  - { id: NF-0084-02, desc: "los tests leen los docs desde disco (no strings inline) para que el gate detecte drift" }
  - { id: NF-0084-03, desc: "todo enlace roto dentro de los docs cuenta como fallo del test" }
acceptance_criteria:
  - id: AC-0084-01
    given: docs/operacion.md
    when: se lee desde disco
    then: existe y contiene las secciones mínimas (abrir/lock, migración, backup, claves, troubleshooting)
    test: test_ac_0084_01_ops_runbook_sections
  - id: AC-0084-02
    given: SECURITY.md
    when: se contrasta con el contenido real del repo
    then: no afirma mecanismos inexistentes (ej. BLAKE3 o cargo-vet si no están en el workspace)
    test: test_ac_0084_02_security_no_unfounded_claims
  - id: AC-0084-03
    given: README.md
    when: se inspeccionan sus enlaces
    then: enlaza docs/operacion.md y SECURITY.md con rutas que resuelven a ficheros existentes
    test: test_ac_0084_03_readme_links_ops_docs
exit_criteria:
  - cargo test -p ruscadb --test ops_docs
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar docs/operacion.md, ops_docs.rs y los enlaces (vuelve a existir el hueco de runbook)
sandbox:
  - cargo test -p ruscadb --test ops_docs
---

# SPEC-0084 — Documentación de operación y SECURITY alineado (Ola 1)

## Contexto

Auditoría *production readiness* 2026-10-12 (riesgos 7 y 9): no hay runbook
de operación (backup, migración, lock, claves) y `SECURITY.md` afirma
mecanismos que **no existen** (BLAKE3 para integridad, `cargo-vet`, según la
revisión del texto) — una política de seguridad que miente es peor que una
ausente.

## Superficie

- `docs/operacion.md` (nuevo): runbook en español con los flujos reales
  implementados en Ola 0 (`DatabaseInUse`, `SchemaVersionTooNew`,
  `MigrationFailed`) más backup en frío y rotación de claves (SPEC crypto).
- `SECURITY.md`: auditoría de afirmaciones — cada mecanismo citado se
  contrasta con el workspace (existe `deny.toml`, job SBOM, cargo-audit,
  gitleaks…); se corrige el texto a la realidad (no se implementan tools
  nuevas aquí).
- `README.md`: enlaces a `docs/operacion.md` y `SECURITY.md`.
- Test: `crates/ruscadb/tests/ops_docs.rs` — lectura de los tres docs desde
  disco con aserciones de secciones/enlaces.

## Criterios de aceptación

- **AC-0084-01** — runbook con secciones mínimas verificadas.
- **AC-0084-02** — `SECURITY.md` sin afirmaciones falsas.
- **AC-0084-03** — README enlaza ambos documentos con rutas que existen.

## Trazabilidad

Tests `test_ac_0084_<nn>_*` con marcador `@spec AC-0084-NN`; verificado por
`cargo xtask trace`/`contract`.
