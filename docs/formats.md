# Formatos físicos de RuscaDB (2026-11-02)

> Documento de formato (P2.6). `migrate --check/--apply` queda como fase 2; hoy
> la detección es por `schema_version` del manifiesto.

## WAL (`<data>.wal`)

- Frames con `magic`, `kind`, `lsn`, `len`, payload y **CRC32C**.
- Group commit y segmentación (`segmented-wal`) rotan antes de escribir.
- Recuperación: se ignoran frames truncados al final; un CRC inválido aborta.
- El frame cifrado (SPEC-0013/0105) usa AEAD; el layout claro no cambia.

## Manifiesto (`MANIFEST.json`)

- Campos: `schema_version`, `epoch`, `checkpoint_lsn`, `last_committed_tx`,
  `free_list`.
- `schema_version` es la fuente de migración; un valor mayor que el soportado
  devuelve `SchemaVersionTooNew` (no es corrupción).

## RCOL1 (columnar, `*.rcol`)

```text
magic "RCOL1" | version u32 | rows u32 | columns u32
por columna: name_len u32 | name | encoding u8 | count u32 | payload_len u32 | payload
encoding 0 = Int (FOR + bit-packing), 1 = Float (ALP), 2 = Tagged (text/NULL/bool)
```

- Escritura atómica `tmp + rename`; valida cotas, UTF-8 y bytes sobrantes.
- `format_version` público: `columnar::RCOL1_VERSION` (1).

## RHNSW1 (HNSW persistente, `*.hnsw`)

```text
magic "RHNSW1" | version u32 | dim u32 | metric u8 | nodes u32 |
por nodo: id u64 | vector f32*dim | neighbors u32*
checksum FNV-1a u64
```

- Rechaza truncados/bit-flips; la carga valida el checksum antes de usar.

## RCRD1 (CRDT, `to_bytes`)

```text
magic "RCRD1" | actor | clock_count | (actor,counter)* | value_flag | stamp_flag
```

- Actor/strings con longitud `u32`; valida UTF-8 y bytes sobrantes.

## Pendiente

- `ruscadb migrate --check` (compara `schema_version` y lista el plan).
- `--apply` con backup previo y test de fichero v1 abierto en v2.
