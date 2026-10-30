---
id: SPEC-0168
feature: segmented_wal
status: implemented
maturity: real
owner: storage-team
appetite_days: 6
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-wal]
  files: [crates/ruscadb-wal/src/lib.rs]
  out_of_scope: [logical replication, streaming]
fr:
  - { id: FR-0168-01, desc: "rotacion de segmentos WAL por tamano (default 64MB)" }
  - { id: FR-0168-02, desc: "checkpoint avanza el low-watermark y borra segmentos obsoletos" }
  - { id: FR-0168-03, desc: "fsync batching (group commit) configurable" }
  - { id: FR-0168-04, desc: "compresion zstd por segmento (opcional)" }
nf:
  - { id: NF-0168-01, desc: "cero dependencias nuevas en build por defecto" }
acceptance_criteria:
  - id: AC-0168-01
    given: un WAL que supera el tamano de segmento
    when: se escribe
    then: se crea un nuevo segmento y se cierra el anterior
    test: test_ac_0168_01_rotate
  - id: AC-0168-02
    given: un checkpoint exitoso
    when: se ejecuta
    then: los segmentos anteriores al checkpoint se eliminan
    test: test_ac_0168_02_checkpoint
  - id: AC-0168-03
    given: group commit con N transacciones concurrentes
    when: se hace sync
    then: un solo fsync cubre todas
    test: test_ac_0168_03_group_commit
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-wal --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0168 — WAL segmentado + rotacion + group commit

## Contexto

RuscaDB usa un unico fichero WAL que crece sin limite y no rota. Produccion
requiere segmentacion, checkpoint y group commit para throughput.

## Superficie

- `Wal`: `open_segmented(dir, segment_size)`, `checkpoint()`, `segment_count()`.
- Segmentos nombrados `wal-000001.log`, etc.
- Recovery lee segmentos en orden.

## Estado del arte

- **PostgreSQL**: WAL segmentado (16MB por defecto) + checkpoint + archiving.
- **RocksDB**: MANIFEST + log files rotados.
- **RuscaDB actual**: un solo fichero WAL (Wal::open).

## Diseño propuesto

1. **Segmento**: fichero de tamano fijo; al llenarse se cierra y se crea uno nuevo.
2. **Checkpoint**: flush de paginas dirty + actualizar manifest.last_committed_tx;
   borrar segmentos anteriores.
3. **Group commit**: acumular transacciones en una ventana temporal y un solo
   fsync (configurable, default 1ms).

## Riesgos

- Recovery multi-segmento: ordenar por numero de segmento.

## ADR requerido

`docs/adr/ADR-0168-segmented-wal.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\segmented_wal.md
## Estado de implementacion (2026-10-29)

- Modulo: `crates/ruscadb/src/segmented_wal.rs` (`SegmentedWal`, `WalRecord`).
- Tests: `test_ac_0168_01..03` + replay y `segment_of`.
- Invariantes: LSN monotono; la rotacion ocurre antes de escribir (el ultimo
  registro queda en el segmento nuevo); el checkpoint conserva el replay vivo.
