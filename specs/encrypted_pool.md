---
id: SPEC-0106
feature: encrypted_pool
status: implemented
owner: storage-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-wal]
  files: [crates/ruscadb/src/database.rs, crates/ruscadb-wal/src/lib.rs, crates/ruscadb/tests/encrypted_pool.rs, crates/ruscadb/src/encryption.rs]
  out_of_scope: [modo claro (sin cambios), checkpoint/truncado del WAL en cifrado (el crecimiento del WAL queda documentado como limite), rotacion de claves, KMS/HSM externo, blobs CAS, modo mixto pre-cifrado (paginas .data en claro: se conserva como fallback de lectura), cifrado de paginas en .data]
fr:
  - { id: FR-0106-01, desc: "en modo cifrado, open no precarga el pool: construye un indice overlay page_id -> (lsn, offset) recorriendo los commit records del WAL y fija last_lsn/checkpoint_lsn con el ultimo commit aplicado" }
  - { id: FR-0106-02, desc: "commit() en modo cifrado, tras wal.sync(), marca limpias las paginas confirmadas y actualiza el overlay con el LSN y el offset del commit: las paginas pasan a ser victimas validas de LRU-K" }
  - { id: FR-0106-03, desc: "read_page en modo cifrado sirve desde el overlay cuando el pool falla: lee el commit record en su offset, extrae la pagina, la inserta limpia en el pool y la devuelve; sin overlay la lectura cae a .data y si tampoco existe responde PageOutOfRange" }
  - { id: FR-0106-04, desc: "rollback en modo cifrado conserva la semantica de ultimo estado confirmado sin replay(): descarta solo las sucias y las confirmadas se sirven limpias o se materializan desde el overlay" }
  - { id: FR-0106-05, desc: "el modo claro no cambia: open sigue aplicando replay() sobre .data y publish() sigue marcando limpio tras publicar" }
nf:
  - { id: NF-0106-01, desc: "la lectura desde el overlay es O(1) respecto al tamano del WAL: seek al offset del commit record, sin barrido lineal por fallo de pool" }
  - { id: NF-0106-02, desc: "pool acotado: con un conjunto de trabajo confirmado mayor que la capacidad, open, commit y lecturas completas no fallan con BufferPoolFull; el unico modo de observarlo es dirty sin confirmar sin victimas (misma semantica que modo claro)" }
  - { id: NF-0106-03, desc: "en modo cifrado sigue sin escribirse .data fuera de commit: el overlay y el pool son derivados del WAL, no fuentes" }
acceptance_criteria:
  - id: AC-0106-01
    given: una base cifrada con mas paginas de heap confirmadas que la capacidad del pool
    when: se abre y se recorre una consulta completa que toca todas las paginas
    then: la apertura no falla con BufferPoolFull y la consulta devuelve todas las filas con sus valores exactos
    test: test_ac_0106_01_encrypted_open_with_working_set_above_pool
  - id: AC-0106-02
    given: una base cifrada abierta con pool_capacity 2 y diez paginas de heap insertadas cada una con su commit
    when: se vuelve a leer cada pagina (agotando y desalojando marcos)
    then: cada lectura devuelve el valor exacto de su fila, sin datos rancios ni errores
    test: test_ac_0106_02_encrypted_reads_after_eviction_are_fresh
  - id: AC-0106-03
    given: una base cifrada recien abierta
    when: se inserta una fila y se confirma
    then: el buffer pool no conserva marcas sucias de esa pagina (dirty_pages vacio): la pagina confirmada es desalojable
    test: test_ac_0106_03_encrypted_commit_marks_pages_clean
  - id: AC-0106-04
    given: una base cifrada abierta
    when: se confirma una pagina nueva y otra ya existente
    then: el overlay registra ambas con el LSN del commit y un offset que permite leer el record sin recorrer el WAL
    test: test_ac_0106_04_encrypted_overlay_tracks_committed_pages
  - id: AC-0106-05
    given: una base con paginas escritas en claro antes de activar el cifrado (WAL nuevo vacio), mas suficientes paginas confirmadas en cifrado al reabrir como para superar la capacidad del pool
    when: se reabre con clave y se recorre una consulta completa
    then: la apertura no falla y la consulta sirve el contenido pre-cifrado exacto desde .data junto con el post-cifrado desde el overlay
    test: test_ac_0106_05_encrypted_reads_fall_back_to_data_file
  - id: AC-0106-06
    given: una base cifrada cuyas paginas confirmadas fueron desalojadas del pool, con una transaccion en vuelo que modifico una pagina
    when: se llama rollback()
    then: el ultimo estado confirmado se sirve completo tras el descarte (todas las paginas materializables desde el overlay) y los ficheros quedan intactos
    test: test_ac_0106_06_encrypted_rollback_after_eviction
  - id: AC-0106-07
    given: una base cifrada con pool_capacity 1 en la que se escribe y confirma la pagina A y luego una transaccion escribe sin confirmar la pagina B
    when: una escritura posterior a una tercera pagina C no encuentra marco victima (B esta sucia y A quedo limpia al confirmarse)
    then: la escritura de B nunca agoto el pool y la de C falla en voz alta con BufferPoolFull: solo el dirty sin confirmado queda sin victimas (limite restante identico al modo claro)
    test: test_ac_0106_07_encrypted_pool_full_only_for_uncommitted_dirty
exit_criteria:
  - cargo test -p ruscadb --all-features --test encrypted_pool
  - cargo test -p ruscadb --all-features --lib database
  - cargo test -p ruscadb-wal --all-features
  - cargo xtask red specs/encrypted_pool.md (evidencia test-first)
  - cargo fmt --all -- --check
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo test --workspace --all-features
  - cargo doc --workspace --no-deps --all-features (RUSTDOCFLAGS=-D warnings)
  - python scripts/check_architecture.py
  - python scripts/check_core_unsafe.py
  - cargo xtask trace / contract verdes
  - cargo mutants -p ruscadb --in-diff MS_diff >= 70%
rollback:
  - revertir database.rs, ruscadb-wal/src/lib.rs y tests al commit previo; el
    limite del pool vuelve a documentarse en docs/MVP.md y encryption.rs
sandbox:
  - cargo test -p ruscadb --all-features --test encrypted_pool
---

# SPEC-0106 — Pool acotado en modo cifrado (overlay WAL con recuperación perezosa)

## Contexto

En modo cifrado `publish()` no escribe `.data` (SPEC-0013: el WAL es la única
fuente de verdad), así que **ninguna página confirmada es desalojable**: el pool
solo acepta marcos que se puedan descartar sin pérdida y, al no marcarse
limpios, el conjunto de trabajo vive fijo en RAM. Hoy el efecto es doble:

- `Database::open` llama a `replay()`, que vuelca **todas** las páginas
  confirmadas del WAL al pool como sucias: una base cifrada con más páginas
  que `pool_capacity` **no puede ni abrirse** (BufferPoolFull en la
  precarga).
- En régimen, cada commit deja sus páginas sucias para siempre: el pool se
  llena con el working set y toda escritura posterior falla con
  `BufferPoolFull`.

Estado del límite en `docs/MVP.md`: "Pool dimensionado en cifrado: las páginas
cifradas permanecen `dirty` en el pool (nunca se desalojan); un pool lleno
falla con `BufferPoolFull`".

La observación que habilita la feature es la misma que en SPEC-0105: el WAL
contiene la **imagen completa y confirmada** de cada página en su commit
record. Por eso desalojar deja de ser peligroso: la página puede
**reconstruirse desde el WAL** en el momento de la lectura. Es el patrón de
lector *lazy* del modo WAL de SQLite (wal-index) aplicado al pool.

## Superficie

- `crates/ruscadb/src/database.rs` — overlay en `Database` (`page_id →
  (lsn, offset)`), `open` (construcción del overlay en vez de precarga),
  `commit` (marca limpia + actualización), `read_page` (materialización desde
  el WAL), `rollback` (sin `replay()`), `page_exists` (considera el overlay).
- `crates/ruscadb-wal/src/lib.rs` — API de localización: el escaneo de
  records expone el offset de fichero de cada commit y la lectura dirigida de
  un record en su offset (NF-0106-01).
- `crates/ruscadb/tests/encrypted_pool.rs` — AC-0106-01, 02, 05, 06 (API
  pública).
- `crates/ruscadb/src/database.rs` (unidad) — AC-0106-03, 04, 07.
- `crates/ruscadb/src/encryption.rs` — el límite documentado se retira del
  módulo y `docs/MVP.md` pasa de "pool nunca desaloja" al límite restante
  honesto (WAL que crece sin checkpoint en cifrado, fuera de alcance aquí).

Queda fuera: modo claro (sin cambios), checkpoint/truncado del WAL en cifrado,
rotación de claves, KMS/HSM, blobs, el modo mixto pre-cifrado (se conserva
como *fallback* de lectura) y cifrado de las páginas de `.data`.

## Estado del arte (FRS, verificado web 2026-10-17)

El diseño sigue el patrón consensuado por la vanguardia del almacenamiento
embebido:

- **SQLite WAL / wal-index** (`sqlite.org/wal.html`, `sqlite.org/walformat.html`,
  Fly.io "SQLite Internals: WAL"): el lector resuelve cada página con
  *pool/reader-cache → frames del WAL → fichero de base*, y el *wal-index*
  (página de hashes `aPgno/aHash` + `FindFrame(P, M)`) existe justo para no
  barrer el WAL en cada miss. Nuestro overlay `page_id → (lsn, offset)` cumple
  el mismo contrato: *FindFrame* con offset directo, resolución O(1) sin
  escaneo (NF-0106-01).
- **SQLCipher (Zetetic design)**: las páginas del WAL se cifran por página con
  la misma clave de la base y con MAC por página — igual que nuestro commit
  record cifrado — y su documentación de soporte confirma el patrón de
  fallar *lazily* en lectura en vez de precargar estado cifrado en RAM.
- **RocksDB `encryption_at_rest` / TiKV**: cifran por fichero con *data keys*
  y las claves se resuelven al leer (key indirection), no al abrir; refuerza
  que la recuperación diferida es el modelo operativo estándar y que el
  checkpoint/reescritura con clave es trabajo de *key rotation* (fuera de
  alcance, ya declarado).
- **Coste del sin-checkpoint** (lobste.rs "Ones to Zeroes", HN): un lector
  vivo continuo impide el checkpoint y el WAL crece sin límite — exactamente
  el límite restante que documentamos en `docs/MVP.md` tras retirar el del
  pool. Lo honesto y frontera es acotar el pool por reconstrucción y dejar el
  checkpoint como trabajo futuro explícito, no ocultarlo precargando.

## Notas de diseño

- **Overlay como índice de reconstrucción.** `HashMap<PageId, (Lsn, u64)>`
  apunta al commit record que contiene la última imagen confirmada de la
  página. Se construye en `open` con una sola pasada por el WAL (misma
  lectura que hoy hace `replay`, reteniendo la última aparición de cada
  página) y se actualiza en `commit` con el record recién appendeado. El
  offset convierte cada miss en un *seek* + decodificación de un solo record
  (NF-0106-01); extraer la página del payload es buscar su `PageId` en el
  vector de `decode_commit`.
- **`open` deja de precargar.** En cifrado, `open` no llama a la rama de
  precarga de `replay()`: construye el overlay y fija `last_lsn` y
  `checkpoint_lsn` con el último commit, como hoy. `rebuild_indexes` y
  `rebuild_primary_index` leen el heap con marcos **limpios**, que sí son
  desalojables: una base mayor que el pool abre y barre completa
  (AC-0106-01).
- **`commit` marca limpio (la llave del desalojo).** Tras `wal.sync()`, el
  commit ya es recuperable desde el WAL, así que `publish` en cifrado pasa de
  no-op a `mark_clean` + overlay. LRUK del pool no cambia: las páginas
  confirmadas entran en el conjunto de víctimas (AC-0106-03) y el miss
  posterior se resuelve desde el overlay (AC-0106-02), nunca desde un
  `.data` desactualizado.
- **`read_page` con tres vías.** Pool → overlay (materializa limpia en el
  pool) → `.data` (páginas pre-cifrado, AC-0106-05) → `PageOutOfRange`.
  `page_exists` debe considerar el overlay: en cifrado `.data` no crece, así
  que una página existente solo por overlay devolvería falso en el guard previo
  al escaneo.
- **`rollback` sin replay.** Con las confirmadas marcadas limpias, el
  descarte de sucias devuelve el pool al último estado confirmado y las
  páginas desalojadas se materializan al leer; la rama de replay de SPEC-0105
  queda redundante y se retira manteniendo su semántica byte-idéntica
  (AC-0106-06). La rama de `replay()` solo sobrevive para el modo claro en
  `open`.
- **Límite restante honesto.** Sin víctimas solo queda el dirty sin
  confirmar (AC-0106-07), idéntico al modo claro; el crecimiento del WAL sin
  checkpoint queda fuera de alcance y se documenta en `docs/MVP.md` como el
  límite que sustituye al retirado.
- **Por qué no cifrar `.data` aquí.** Requeriría un formato de página con
  nonce+tag dentro del slot fijo de 4 KiB (migración de formato) y abriría
  checkpoint en cifrado; el overlay cierra el límite del pool con la
  semántica ya probada por SPEC-0105 sin tocar el formato en disco.
