---
id: SPEC-0031
feature: mvp_suite
status: accepted
owner: product-team
appetite_days: 5
boundaries:
  crates: [ruscadb]
  out_of_scope: [cambiar codigo de producto, nuevos crates, rendimiento]
fr:
  - { id: FR-0031-01, desc: "suite MVP end-to-end (open->tabla->insert->indices->query->delete->tx->reopen) con la API publica" }
  - { id: FR-0031-02, desc: "MVP cubre los 5 modelos + multimodal (vector, aristas, texto) en un solo flujo" }
  - { id: FR-0031-03, desc: "MVP cifrado: el mismo flujo con EncryptionConfig y sin claro en disco" }
  - { id: FR-0031-04, desc: "docs/MVP.md define el producto minimo viable (capacidades y limites)" }
nf:
  - { id: NF-0031-01, desc: "la suite usa SOLO la API publica estable (sin internals)" }
  - { id: NF-0031-02, desc: "determinista y rapida (<30s en total)" }
acceptance_criteria:
  - id: AC-0031-01
    given: una base nueva en claro
    when: se ejecuta el flujo MVP completo (crear->insertar->indexar->consultar->borrar->tx->reap->reabrir)
    then: todas las etapas producen el resultado esperado
    test: test_ac_0031_01_mvp_happy_path
  - id: AC-0031-02
    given: registros con vector, aristas y texto
    when: se consultan KNN/TRAVERSE/MATCH
    then: devuelven los resultados correctos (los 3 modelos en un flujo)
    test: test_ac_0031_02_mvp_multimodel
  - id: AC-0031-03
    given: una base cifrada
    when: se ejecuta el flujo MVP
    then: funciona igual y en disco no hay claro
    test: test_ac_0031_03_mvp_encrypted
  - id: AC-0031-04
    given: docs/MVP.md
    when: se lee
    then: define capacidades, limites y la API estable del MVP
    test: test_ac_0031_04_mvp_doc_exists
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0031
  - docs/MVP.md creado y coherente con la API
rollback:
  - eliminar tests/mvp.rs y docs/MVP.md
sandbox:
  - cargo test -p ruscadb -- test_ac_0031
---

# SPEC-0031 — Suite de aceptación del MVP + docs/MVP.md

## Contexto

Define y demuestra el **producto mínimo viable** de RuscaDB como un único flujo
ejecutable con la **API pública estable** (sin internals):

1. Abrir (builder) en claro y cifrado.
2. Crear tabla, insertar (escalares + vector + aristas + texto), lote.
3. Crear índices (secundario + primario automático).
4. Consultar `SELECT/WHERE/MATCH/KNN/TRAVERSE/LIMIT`.
5. Borrar, transaccionar (begin/commit/rollback), snapshot, reap.
6. Manifest/tables, cerrar y reabrir con durabilidad total.

`docs/MVP.md` documenta las capacidades, los límites conocidos y la API estable.

## Criterios de aceptación

- **AC-0031-01..03** — flujos en claro, multimodal y cifrado.
- **AC-0031-04** — documentación del MVP.

## Trazabilidad

Tests `test_ac_0031_<nn>_*` en `crates/ruscadb/tests/mvp.rs`; `cargo xtask trace`.
