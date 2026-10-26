---
id: SPEC-0167
feature: page_checksums
status: proposed
owner: storage-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core, ruscadb-storage]
  files: [crates/ruscadb-storage/src/page.rs, crates/ruscadb-storage/src/paged_file.rs]
  out_of_scope: [full doublewrite buffer, io_uring]
fr:
  - { id: FR-0167-01, desc: "checksum CRC32C por pagina en el header" }
  - { id: FR-0167-02, desc: "verificacion al leer; error RuscaError::CorruptPage" }
  - { id: FR-0167-03, desc: "deteccion de torn writes (partial page)" }
  - { id: FR-0167-04, desc: "repair opcional desde WAL (page redo)" }
nf:
  - { id: NF-0167-01, desc: "overhead < 2% en throughput de lectura" }
acceptance_criteria:
  - id: AC-0167-01
    given: una pagina con checksum valido
    when: se lee
    then: se devuelve sin error
  - id: AC-0167-02
    given: una pagina con checksum corrupto
    when: se lee
    then: se devuelve RuscaError::CorruptPage
  - id: AC-0167-03
    given: una escritura parcial (torn write)
    when: se detecta
    then: se repara desde WAL o se reporta error
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-storage --all-features
  - cargo xtask trace
  - cargo xtask contract
---

# SPEC-0167 — Checksums de pagina + proteccion contra torn writes

## Contexto

`Page` no tiene checksum; un torn write o bit flip corrompe silenciosamente
datos. PostgreSQL usa CRC por pagina + full page writes en WAL.

## Superficie

- `page.rs`: header con `checksum: u32` (CRC32C) + `lsn: u64` + `page_type`.
- `paged_file.rs`: calcular/verificar checksum en read/write; detectar torn
  write (magic + length + checksum).
- `RuscaError::CorruptPage { page_id, expected, actual }`.

## Estado del arte

- **PostgreSQL**: CRC-32C por bloque + full page images en WAL.
- **SQLite**: checksums en WAL frames (no en paginas de datos).
- **RocksDB**: CRC por bloque SST + recovery.
- **RuscaDB actual**: sin checksums en paginas.

## Diseño propuesto

1. **Header** (primeros 16 bytes): magic(4) + page_id(4) + lsn(8) + checksum(4).
2. **Escritura**: calcular CRC32C sobre el payload + header (sin checksum).
3. **Lectura**: recalcular y comparar; mismatch -> CorruptPage.
4. **Torn write**: si magic/length invalidos -> tratar como pagina no inicializada.

## Alternativas descartadas

- Doublewrite buffer completo: alto costo; se deja para futuro.

## ADR requerido

`docs/adr/ADR-0167-page-checksums.md`.
---\n","path":"C:\\Users\\USUARIO\\Documents\\DEV-SPACE\\RuscaDB\\specs\\page_checksums.md