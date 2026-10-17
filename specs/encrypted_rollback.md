---
id: SPEC-0105
feature: encrypted_rollback
status: implemented
owner: storage-team
appetite_days: 3
boundaries:
  crates: [ruscadb]
  files: [crates/ruscadb/src/database.rs, crates/ruscadb/tests/encrypted_rollback.rs, crates/ruscadb/src/encryption.rs, crates/ruscadb/src/batch.rs]
  out_of_scope: [rotacion de claves, KMS/HSM externo, savepoints, truncado/checkpoint del WAL, modo mixto pre-cifrado (.data con paginas en claro), blobs CAS, modo claro (sin cambios)]
fr:
  - { id: FR-0105-01, desc: "Database::rollback() en modo cifrado aborta la tx activa, vacia el buffer pool y restaura el ultimo estado confirmado desde el WAL (replay idempotente con la clave de apertura)" }
  - { id: FR-0105-02, desc: "las paginas creadas sin commit no sobreviven al rollback: no estan en el WAL y dejan de existir en el pool (page_exists false)" }
  - { id: FR-0105-03, desc: "el rollback cifrado no publica nada a disco: ni paginas nuevas en .data ni frames nuevos en el .wal (el WAL sigue siendo la unica fuente de verdad)" }
  - { id: FR-0105-04, desc: "los indices en memoria (primario y derivados) se reconstruyen desde el heap restaurado, sin entradas fantasma de lo abortado" }
nf:
  - { id: NF-0105-01, desc: "el costo del rollback cifrado es O(paginas confirmadas) en memoria + O(tamano del WAL) de lectura, sin escrituras de disco" }
  - { id: NF-0105-02, desc: "rollback sin cambios es un no-op idempotente: Ok repetido, estado y discos intactos" }
acceptance_criteria:
  - id: AC-0105-01
    given: una base cifrada con una fila confirmada y su pagina de heap modificada sin commit
    when: se llama rollback()
    then: la pagina vuelve byte a byte al estado confirmado, active_tx queda en None, la fila confirmada sigue visible y la base acepta nuevas escrituras
    test: test_ac_0105_01_encrypted_rollback_restores_confirmed_state
  - id: AC-0105-02
    given: una base cifrada con una pagina nueva escrita sin commit
    when: se llama rollback()
    then: read_page de esa pagina falla con PageOutOfRange (no esta en el WAL) y el SELECT sigue correcto
    test: test_ac_0105_02_encrypted_rollback_drops_uncommitted_pages
  - id: AC-0105-03
    given: una base cifrada abierta con .data y .wal en disco
    when: se llama rollback()
    then: ambos ficheros quedan byte-identicos y sin el marcador en claro
    test: test_ac_0105_03_encrypted_rollback_writes_nothing
  - id: AC-0105-04
    given: una base cifrada tras un rollback
    when: se cierra y se reabre con la clave, se inserta otra fila y se vuelve a reabrir
    then: las filas confirmadas sobreviven sin duplicados ni perdida y la base sigue operativa
    test: test_ac_0105_04_encrypted_rollback_resumes_consistently
  - id: AC-0105-05
    given: una base cifrada recien creada (sin commits) y otra con estado confirmado
    when: se llama rollback() dos veces seguidas
    then: ambas devuelven Ok, el estado y los ficheros quedan intactos y la base sigue operativa
    test: test_ac_0105_05_encrypted_rollback_is_idempotent_noop
  - id: AC-0105-06
    given: una transaccion en vuelo que dejo entradas fantasma en el indice primario y en el indice de texto derivado
    when: se llama rollback()
    then: get_record del id fantasma devuelve None y los conteos de indices vuelven al estado confirmado
    test: test_ac_0105_06_encrypted_rollback_rebuilds_indexes
exit_criteria:
  - cargo test -p ruscadb --all-features --test encrypted_rollback
  - cargo test -p ruscadb --all-features --lib encryption
  - cargo xtask red specs/encrypted_rollback.md (evidencia test-first)
  - cargo fmt --all -- --check
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo test --workspace --all-features
  - cargo doc --workspace --no-deps --all-features (RUSTDOCFLAGS=-D warnings)
  - python scripts/check_architecture.py
  - python scripts/check_core_unsafe.py
  - cargo xtask trace / contract verdes
  - cargo mutants -p ruscadb --in-diff MS_diff >= 70%
rollback:
  - revertir database.rs/encryption.rs/tests al commit previo (restaura el test
    de rechazo en cifrado que se retira de specs/batch_and_rollback.md, hoy
    superseded por esta spec)
sandbox:
  - cargo test -p ruscadb --all-features --test encrypted_rollback
---

# SPEC-0105 — Rollback en modo cifrado

## Contexto

En modo cifrado el WAL es la **única fuente de verdad**: `publish()` no escribe
nunca a `.data` (SPEC-0013), por lo que todas las páginas confirmadas viven
sucias en el buffer pool. `Database::rollback()` (SPEC-0027) solo sabía
descartar las páginas sucias y releerlas desde `.data`, operación imposible
aquí —el `.data` está vacío—, así que se rechazaba con `InvalidConfig` y la
única vía de vuelta al estado confirmado era cerrar y reabrir la base (criterio
hoy superseded por esta spec).

La observación que habilita la feature: el WAL solo recibe *commit records*
(`commit()` es el único punto que hace `wal.append`), así que **reaplicar el
WAL restaura exactamente el último estado confirmado**. Por eso el rollback
cifrado puede ser tan correcto como una reapertura, sin tocar disco.

Estado del límite en `docs/MVP.md`: "Rollback no soportado en cifrado".

## Superficie

- `crates/ruscadb/src/database.rs` — `Database::rollback()`: rama cifrada
  (aborta tx → `discard_dirty_pages()` → `replay()` → reconstruye índices).
- `crates/ruscadb/tests/encrypted_rollback.rs` — AC-0105-01..05 (API pública).
- `crates/ruscadb/src/encryption.rs` — AC-0105-06 (unidad: índices fantasma,
  requiere `pub(crate)`).
- `crates/ruscadb/src/batch.rs` — se elimina `test_ac_0027_05_*` (su AC deja
  de existir; K2 prohíbe marcadores huérfanos).

Queda fuera: rotación de claves, KMS/HSM externo, savepoints, truncado/checkpoint
del WAL, el modo mixto pre-cifrado (páginas `.data` escritas en claro antes de
activar el cifrado) y el modo claro (sin cambios).

## Notas de diseño

- **Rollback cifrado = replay, no descarte simple.** `discard_dirty_pages()`
  vacía el pool (en modo cifrado *todas* las marcas están sucias: `publish` no
  marca limpio y las lecturas no pueden cargar de un `.data` vacío); después
  `replay()` reaplica los commit records del WAL descifrando con la clave de
  apertura y vuelve a dejar las páginas en el pool. El orden importa: si se
  reconstruyera antes de repoblar el pool, `heap_scan` fallaría con
  `PageOutOfRange`.
- **Páginas creadas sin commit.** Un `heap_insert` sin commit puede asignar
  una página que no está en el WAL; el descarte la elimina y `replay` no la
  restaura, de modo que `page_exists` pasa a falso (AC-0105-02). Es el motivo
  de descartar *antes* de replaer, y no solo sobrescribir.
- **No se escribe disco.** `replay()` solo lee el WAL y escribe en el pool; ni
  `.data` ni `.wal` cambian (AC-0105-03). El rollback no toca `last_lsn` ni el
  manifiesto: no nace ningún commit.
- **Idempotencia.** Sin cambios, el rollback cifrado es un no-op: descartar un
  pool vacío y reaplicar el WAL deja el mismo estado (AC-0105-05), incluida una
  base recién creada sin commits (WAL vacío).
- **Supersession.** El criterio y el FR que declaraban el rechazo en cifrado
  quedan invalidados por el nuevo comportamiento; se retiran de SPEC-0027 con
  nota, junto a su test. Los ids retirados no se citan aquí (K3 los exigiría
  como test); la rastroabilidad queda en la nota de SPEC-0027.
- Alternativa descartada: añadir `BufferPool::discard_all()`. Añade API a otro
  crate sin ganancia de corrección hoy (en cifrado no existen marcas limpias)
  y, en el caso mixto pre-cifrado, descartaría páginas que el WAL no puede
  restaurar.
