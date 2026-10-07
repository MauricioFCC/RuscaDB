---
id: SPEC-0019
feature: mvcc_integration
status: accepted
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-txn, ruscadb-wal, ruscadb-core]
  out_of_scope: [GC/reaper, SSI, MVCC en grafos/fts, cifrado del manifiesto]
fr:
  - { id: FR-0019-01, desc: "la fachada abre/crea el manifiesto (schema_version, epoch, checkpoint_lsn) y lo valida" }
  - { id: FR-0019-02, desc: "cada commit incrementa el epoch y persiste el manifiesto de forma atomica" }
  - { id: FR-0019-03, desc: "Database::begin/commit exponen transacciones MVCC con TxId monotono" }
  - { id: FR-0019-04, desc: "las lecturas filtran por snapshot (insert_record estampa created_tx)" }
  - { id: FR-0019-05, desc: "checkpoint_lsn del manifiesto refleja el ultimo LSN confirmado" }
nf:
  - { id: NF-0019-01, desc: "sin dirty reads; snapshot_isolation observable en execute_at" }
  - { id: NF-0019-02, desc: "un manifiesto corrupto o con schema_version desconocida es error accionable (sin panics)" }
acceptance_criteria:
  - id: AC-0019-01
    given: una base nueva
    when: se abre, se escribe/commit y se reabre
    then: el manifiesto existe, el epoch subio y persiste entre aperturas
    test: test_ac_0019_01_manifest_epoch_persists
  - id: AC-0019-02
    given: un fichero de manifiesto con JSON invalido o schema_version futura
    when: se abre la base
    then: devuelve error accionable sin panics
    test: test_ac_0019_02_corrupt_or_future_manifest_is_error
  - id: AC-0019-03
    given: dos transacciones
    when: la primera inserta y no confirma y la segunda toma un snapshot
    then: el registro de la primera NO es visible (sin dirty reads)
    test: test_ac_0019_03_in_flight_tx_not_visible
  - id: AC-0019-04
    given: un snapshot anterior a un insert
    when: se inserta y confirma y se consulta con el snapshot viejo y con uno nuevo
    then: el snapshot viejo no ve el registro y el nuevo si
    test: test_ac_0019_04_snapshot_visibility_end_to_end
  - id: AC-0019-05
    given: una secuencia de commits
    when: se reabre la base
    then: checkpoint_lsn coincide con el ultimo LSN confirmado y el epoch es coherente
    test: test_ac_0019_05_checkpoint_lsn_tracks_commits
exit_criteria:
  - cargo test -p ruscadb -- test_ac_0019
  - cargo mutants -p ruscadb mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - revertir la fachada al commit previo (sin manifiesto/MVCC)
sandbox:
  - cargo test -p ruscadb
---

# SPEC-0019 — Integración de MVCC y manifiesto en el composition root

## Contexto

Integra el crate `ruscadb-txn` (SPEC-0018) en la fachada `ruscadb`:

- **Manifiesto** (`MANIFEST` JSON, ADR-010): `Database::open` carga el manifiesto
  junto a la ruta de datos (`<data>.manifest.json`); si no existe, lo crea; si
  `schema_version` es desconocida → error. Cada `commit` con páginas sucias
  incrementa `epoch`, fija `checkpoint_lsn = lsn` y lo persiste de forma atómica.
  Accesor `Database::manifest() -> &Manifest`.
- **MVCC**: `Database` posee un `TxnManager`. `Database::begin() -> TxId` abre una
  transacción; `Database::commit()` confirma (WAL + manifiesto + tx). `insert_record`
  estampa `record.meta.created_tx` con la transacción activa (o una auto-commit) y
  `meta.lsn`. `Database::snapshot() -> Snapshot`.
- **Visibilidad**: el executor filtra filas por `snapshot.is_visible(Version
  { created_tx, deleted_tx })`. `Database::execute_at(&str, &Snapshot)` permite
  consultar "as of" un snapshot (para snapshot isolation observable); `execute`
  usa el snapshot más reciente.

## Criterios de aceptación

- **AC-0019-01/02** — manifiesto creado/validado/persistido; corrupto → error.
- **AC-0019-03/04** — sin dirty reads; visibilidad por snapshot end-to-end.
- **AC-0019-05** — `checkpoint_lsn` sigue a los commits.

## Trazabilidad

Tests `test_ac_0019_<nn>_*` en `ruscadb`; verificado por `cargo xtask trace`.
