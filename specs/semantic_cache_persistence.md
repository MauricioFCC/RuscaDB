---
id: SPEC-0093
feature: semantic_cache_persistence
status: implemented
owner: query-team
appetite_days: 2
boundaries:
  crates: [ruscadb]
  files: [crates/ruscadb/src/cache.rs, crates/ruscadb/src/database.rs, crates/ruscadb/tests/semantic_cache_persistence.rs]
  out_of_scope: [cifrado del sidecar (en bases cifradas no se persiste), compresion, expiracion/TTL del cache, compaction del fichero, sincronizacion entre procesos]
fr:
  - { id: FR-0093-01, desc: "Database::persist_semantic_cache escribe <data>.cache.json (version 1, escritura atomica tmp+rename) y Database::close la invoca tras el cierre limpio" }
  - { id: FR-0093-02, desc: "Database::open restaura el sidecar (best-effort); un fichero ausente, corrupto o con version desconocida se ignora sin fallar la apertura" }
  - { id: FR-0093-03, desc: "una mutacion invalida las entradas y retira el sidecar; en bases cifradas nunca se persiste ni se carga el cache en claro" }
nf:
  - { id: NF-0093-01, desc: "el sidecar no compromete la durabilidad: se escribe atomico y se ignora ante corrupcion; cero regresion si no hay entradas" }
  - { id: NF-0093-02, desc: "seguridad: base cifrada => sin cache en claro en disco, ni se confia en un sidecar plantado" }
acceptance_criteria:
  - id: AC-0093-01
    given: una base con filas vectoriales y una consulta KNN que calienta el cache
    when: se cierra la base (close) y se reabre
    then: las entradas se restauran y un vector cercano se sirve como hit desde la clave restaurada
    test: test_ac_0093_01_cache_persists_across_reopen
  - id: AC-0093-02
    given: un cache caliente persistido en `<data>.cache.json`
    when: se ejecuta una mutacion (insert)
    then: las entradas quedan a cero y el sidecar se retira (no resucitan filas obsoletas)
    test: test_ac_0093_02_mutation_invalidates_persisted_cache
  - id: AC-0093-03
    given: un sidecar con version desconocida y, despues, un sidecar corrupto
    when: se reabre la base en cada caso
    then: la apertura no falla y el cache queda vacio
    test: test_ac_0093_03_corrupt_or_unknown_version_is_ignored
  - id: AC-0093-04
    given: una base cifrada con cache caliente y, despues, un sidecar en claro plantado a mano
    when: se cierra y se reabre
    then: no se escribe sidecar y no se cargan entradas del fichero en claro
    test: test_ac_0093_04_encrypted_database_never_persists_plaintext_cache
exit_criteria:
  - cargo test -p ruscadb --test semantic_cache_persistence
  - cargo test -p ruscadb --all-features  (sin regresion)
  - cargo clippy -p ruscadb --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - retirar persist_semantic_cache/restore_semantic_cache, los helpers de fichero y la restauracion en open
sandbox:
  - cargo test -p ruscadb --test semantic_cache_persistence
---

# SPEC-0093 — Persistencia del caché semántico (`<data>.cache.json`)

## Contexto

Línea futura de producto (roadmap §Estado, «Líneas futuras»): la persistencia
del caché semántico (SPEC-0065) no existía; cada reapertura arrancaba con el
caché frío. La frontera de motores embebidos 2026 trata el caché como un
**derivado reconstruible** con sidecar versionado y escritura atómica, y con
una regla dura de seguridad: nunca persistir datos en claro de una base
cifrada.

Esta spec añade un sidecar `<data>.cache.json` (versión 1) que sobrevive a un
cierre limpio, se restaura de forma *best-effort* (nunca impide abrir), se
invalida (retira el fichero) ante cualquier mutación, y se omite por completo
en bases cifradas.

## Superficie

- `crates/ruscadb/src/cache.rs`: `Database::persist_semantic_cache` (público),
  `restore_semantic_cache` (interno, llamado por `open`), helpers
  `read_cache_file`/`write_cache_file`/`remove_cache_file` y la invalidación que
  retira el sidecar.
- `crates/ruscadb/src/database.rs`: campo `cache_path`, restauración en
  `Database::open` y persistencia en `Database::close`.
- Tests: `crates/ruscadb/tests/semantic_cache_persistence.rs`.

## Criterios de aceptación

- **AC-0093-01** — el caché sobrevive a `close`/reopen y sirve un hit.
- **AC-0093-02** — una mutación invalida y retira el sidecar.
- **AC-0093-03** — sidecar corrupto/versión desconocida se ignora.
- **AC-0093-04** — base cifrada: sin persistencia ni carga en claro.

## Trazabilidad

Tests `test_ac_0093_<nn>_*` con marcador `@spec AC-0093-NN`; verificado por
`cargo xtask trace`/`contract`.
