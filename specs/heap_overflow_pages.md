---
id: SPEC-0074
feature: heap_overflow_pages
status: implemented
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb, ruscadb-storage]
  out_of_scope: [compresion de filas, blobs externos para docs, cambio de PAGE_SIZE]
fr:
  - { id: FR-0074-01, desc: "una fila mayor que MAX_ROW_BYTES se parte en una cadena de paginas de overflow enlazadas; heap_read la reconstruye" }
  - { id: FR-0074-02, desc: "update/remove liberan la cadena de overflow previa; el WAL/replay y el scan siguen siendo correctos" }
nf:
  - { id: NF-0074-01, desc: "cero regresion: las filas que caben en una pagina se almacenan y leen igual (formato intacto)" }
  - { id: NF-0074-02, desc: "sin panics; filas corruptas -> error accionable; sin dependencias nuevas" }
acceptance_criteria:
  - id: AC-0074-01
    given: un record cuyo payload supera MAX_ROW_BYTES (una pagina de overflow)
    when: se inserta y se lee por id
    then: round-trip integro (doc completo) y el scan lo devuelve
    test: test_ac_0074_01_single_overflow_page_roundtrip
  - id: AC-0074-02
    given: un record de ~12 KB (tres paginas de overflow)
    when: se inserta y se lee
    then: round-trip integro byte a byte
    test: test_ac_0074_02_multi_page_overflow_roundtrip
  - id: AC-0074-03
    given: un record con overflow
    when: se actualiza a un valor que cabe en una pagina
    then: el nuevo valor se lee bien y la cadena de overflow queda liberada (reutilizable)
    test: test_ac_0074_03_update_releases_overflow
  - id: AC-0074-04
    given: un record con overflow
    when: se borra (delete) y se escanea la tabla
    then: la fila no aparece y la cadena queda liberada
    test: test_ac_0074_04_delete_releases_overflow
  - id: AC-0074-05
    given: un record con overflow insertado y confirmado
    when: se cierra y reabre la base
    then: el record se lee integro (replay/WAL correcto)
    test: test_ac_0074_05_overflow_survives_reopen
exit_criteria:
  - cargo test -p ruscadb --test heap_overflow -- test_ac_0074
  - cargo test -p ruscadb --test executor_integration --test mvcc_integration --test operational_hardening
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - volver al rechazo InvalidConfig para filas > MAX_ROW_BYTES (eliminar el camino de overflow)
sandbox:
  - cargo test -p ruscadb --test heap_overflow
---

# SPEC-0074 — Overflow pages del heap (filas > página)

## Contexto

Línea futura «overflow pages del heap». Hoy `serialize_row` rechaza filas
> `MAX_ROW_BYTES` (4090 B) con `InvalidConfig` — hallazgo de la retrospectiva 3
(el fixture de SPEC-0064 lo esquivó). Técnica clásica (SQLite overflow pages) y
vigente en 2026: la fila base guarda un puntero y la cadena de páginas de
desbordamiento almacena el resto.

## Diseño

- Página base: slot normal con una fila "stub" de cabecera
  `[OVERFLOW_MAGIC u32][total_len u64][first_page u64]` cuando el payload no
  cabe; las páginas de overflow se enlazan `[next u64][chunk]`.
- `heap_insert`: si `bytes.len() > MAX_ROW_BYTES` → crear cadena y escribir el
  stub; `heap_read`: detectar magic → recorrer la cadena y concatenar.
- `heap_update`: liberar la cadena previa y reescribir (o volver a fila normal).
- `heap_remove`: liberar la cadena.
- El registro de páginas sucias en el WAL ya es genérico (páginas), así que el
  replay sigue siendo correcto (AC-0074-05).

## Criterios de aceptación

- **AC-0074-01/02** — round-trip de 1 y 3 páginas de overflow.
- **AC-0074-03/04** — liberación en update/delete.
- **AC-0074-05** — persistencia/replay.

## Trazabilidad

Tests `test_ac_0074_<nn>_*`; verificado por `cargo xtask trace`/`contract`.