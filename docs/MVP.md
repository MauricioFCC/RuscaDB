# RuscaDB — Producto Mínimo Viable (MVP)

> SPEC-0031 · Suite de aceptación: `crates/ruscadb/tests/mvp.rs`
> (`test_ac_0031_01_mvp_happy_path` … `test_ac_0031_04_mvp_doc_exists`).

RuscaDB es una base de datos **embebida, multi-modelo y multimodal**: un único
`Record` físico sirve los cinco modelos (relacional, documento, grafo, vector,
time-series) más blobs multimodales, sin servidor y sin red. El MVP es el flujo
mínimo que demuestra esa propuesta de valor con la **API pública estable** de la
fachada `ruscadb` (composition root): abrir → crear tabla → insertar (fila +
lote) → indexar → consultar → borrar → transaccionar → reabrir, en claro y
cifrado.

## Capacidades

- **Apertura flexible**: `Database::open(DbConfig)` o el builder fluido
  (`Database::builder().data_path(..).pool_capacity(..).encryption(..).open()`),
  con recovery WAL-first idempotente al arrancar.
- **Modelo relacional**: `create_table` con esquema (`title: Text`,
  `score: Float`, …), `insert` escalar con coerción `Int→Float`, varios índices
  secundarios (`create_index`, una columna por índice, SPEC-0104) y RQL
  (`SELECT *`, `WHERE`, proyección, `LIMIT`).
- **DML en RQL**: `INSERT INTO t (cols) VALUES (...)` (multi-fila = un solo
  commit vía `insert_many`), `UPDATE t SET col = ... [WHERE ...]` y
  `DELETE FROM t [WHERE ...]`; `execute` devuelve `{"affected": N}`, o las
  **filas afectadas** con `RETURNING *|<cols>` (SPEC-0102).
- **Analítica en RQL**: `ORDER BY <col> [ASC|DESC]` (NULL al final en ASC;
  acepta `tabla.col`) y `GROUP BY a, b, ...` multi-clave con agregados
  `COUNT(*)`/`COUNT(col)`/`SUM`/`AVG`/`MIN`/`MAX` (hash aggregation; sin
  `GROUP BY` = una sola fila global) más `HAVING <agregado> <op> <literal>
  [AND ...]` post-agregación.
- **JOIN en RQL**: `SELECT ... FROM a [INNER|LEFT|RIGHT|FULL [OUTER]|CROSS] JOIN
  b [ON a.x = b.y]` (una igualdad; nested-loop con lookups al índice secundario
  o barrido filtrado; columnas fusionadas con prefijo `tabla.col`). Los outer
  joins conservan las filas sin contraparte con el otro lado a `NULL`
  (SPEC-0052/SPEC-0099/SPEC-0103).
- **Modelo documental**: `Record.doc` (JSON anidado) consultable desde RQL con
  `col -> 'a.b'` (extracción por ruta) y `col @> '{json}'` (contención).
- **Modelo vectorial**: `insert_record` con `Embedding` (+ `EmbeddingMeta` con
  modelo, dimensión y `Metric::L2`), búsqueda `KNN embedding <|k|> [...]` y
  `KNN` con `WHERE` resuelto por **iFVS** (estrategia pre/in/post por
  selectividad, SPEC-0047/0048).
- **Modelo de grafo**: aristas tipadas (`Edge`/`EdgeSet`) y recorridos
  `TRAVERSE edges DEPTH n`.
- **Texto + FTS**: columnas `Text` consultables con `WHERE MATCH(col, 'término')`
  ordenado por BM25.
- **Blobs multimodales integrados**: `DbConfig::blob_path` abre el blob store
  CAS; `put_blob`/`get_blob` hacen roundtrip exacto (dedup por contenido) y
  `gc_blobs` purga huérfanos con el barrier R7 (solo sin transacción activa).
- **Time-series**: crate `ruscadb-ts` con `time_bucket`, ventanas tumbling/
  deslizantes, remuestreo con relleno, `percentile`, `rate` y `moving_average`
  sobre `ScalarValue::TimestampMillis`.
- **Escritura por lotes**: `insert_many` confirma N registros con un único
  commit (un frame WAL + un `fsync`).
- **Borrado lógico MVCC + GC**: `delete` por `RecordId` (idempotente), visibilidad
  por snapshot (`snapshot` / `execute_at` / `get_record`) y purga física con
  `reap`.
- **Transacciones**: `begin` / `commit` / `rollback`, `active_tx`, `last_lsn` y
  manifiesto versionado (`manifest`, `tables`, `catalog`).
- **Durabilidad total**: `close` persiste lo pendiente; al reabrir se recuperan
  tablas, filas e índices (secundario, HNSW, CSR, FTS e índice primario).
- **Paginación y dedupe**: `LIMIT n OFFSET m` (y `OFFSET n` sin `LIMIT`,
  semántica Postgres) y `SELECT DISTINCT` (proyección completa, dedupe antes de
  `OFFSET`/`LIMIT`), aplicados **después** de `ORDER BY` en la ruta de escaneo,
  la de agregación y el `JOIN` (SPEC-0096/SPEC-0097).
- **Cifrado en reposo**: `EncryptionConfig::new(clave)` o
  `EncryptionConfig::from_passphrase(..)`; el WAL se cifra (AEAD) y en disco no
  queda ningún claro verificable.
- **Concurrencia y durabilidad**: lock exclusivo cross-proceso `<data>.lock`
  (SPEC-0076) — una segunda apertura devuelve `DatabaseInUse`; group commit con
  `FsyncPolicy::{Full, Normal}` (SPEC-0081), donde `Normal` coalesce fsyncs sin
  confirmar nada no durable.
- **Escritura estructurada**: `Database::update(table, id, scalars)` (SPEC-0091)
  actualiza columnas de una fila viva por `RecordId` con la misma ruta de
  índices que el `UPDATE ... WHERE` en RQL.
- **Catálogo escalable**: sin techo de 64 KiB (SPEC-0086); el catálogo crece por
  una cadena de páginas de overflow (techo residual `u32`). Su formato persiste
  la lista de índices por tabla (v3/v4, SPEC-0104) y sigue leyendo las bases
  v1/v2 de un solo índice.
- **Reclamación de storage**: `reap` purga el heap, da de alta las páginas en la
  free-list y retira las entradas de los índices derivados (CSR/FTS) (SPEC-0085).
- **Caché semántico persistente**: `close` guarda las claves KNN en
  `<data>.cache.json` y `open` las restaura *best-effort* (SPEC-0093); una
  mutación invalida el caché y retira el sidecar.
- **Keystore KMS-lite**: keystore con rotación de claves para las sesiones MCP
  (SPEC-0088), sin KMS/HSM externo.

## Límites

- **Single-writer**: una única conexión de escritura por fichero, garantizada
  por un lock exclusivo cross-proceso `<data>.lock` (SPEC-0076): otra apertura
  (proceso o misma app) falla con `DatabaseInUse`. Sin modo servidor.
- **Sin cursores con estado ni streaming**: `execute` materializa todas las
  filas del resultado en memoria; la paginación es por `LIMIT`/`OFFSET`
  (SPEC-0096), no *keyset* ni cursor de servidor.
- **Sin optimizador de consultas**: el plan solo elige `IndexScan` por igualdad
  (la primera columna indexada del `WHERE`); sin costos, stats ni join reordering.
- **Compactación de índices derivados**: `reap` reconstruye HNSW/CSR/FTS desde
  las filas vivas (SPEC-0101), de modo que las entradas de filas purgadas
  desaparecen (sin tombstones residuales).
- **Rollback no soportado en cifrado**: en modo cifrado las páginas viven en el
  WAL y nunca se publican a `.data`; `rollback` devuelve `InvalidConfig` (cerrar
  y reabrir para volver al último estado confirmado).
- **Pool dimensionado en cifrado**: las páginas cifradas permanecen `dirty` en
  el pool (nunca se desalojan); un pool lleno falla con `BufferPoolFull`.
- **KMS/HSM externos fuera de alcance**: el keystore KMS-lite local soporta
  rotación de claves (SPEC-0088), pero no hay KMS/HSM ni custodia externa. El
  catálogo ya no tiene el techo de 64 KiB (SPEC-0086); su techo residual es
  `u32`.
- **RQL acotado**: `JOIN` solo de dos tablas con una igualdad (`INNER`, `LEFT`,
  `RIGHT`, `FULL` o `CROSS`, sin no-equi ni multi-condición) y sin subconsultas;
  `ORDER BY` de una sola clave (sin `NULLS FIRST/LAST`); DML sin
  `UPSERT`/`MERGE`; el orden de `MATCH` lo fija BM25 y el de
  `KNN` la distancia. Los operadores `->`/`@>` no usan índice (scan por fila).

## API estable

Superficie cubierta por la suite del MVP (todo lo demás son internals):

- Apertura: `Database`, `DbConfig`, `Database::builder()`.
- Escritura: `create_table`, `insert`, `insert_record`, `insert_many`, `update`,
  `delete`, `create_index`, `commit`, `rollback`, `close`, `put_blob`.
- Lectura: `execute` (incluye DML, `ORDER BY`, `GROUP BY` y operadores de
  documento), `execute_at`, `tables`, `catalog`, `get_record`, `reap`,
  `get_blob`, `gc_blobs`.
- Transacciones: `begin`, `snapshot`, `active_tx`, `manifest`, `last_lsn`.
- Tipos: `Record`, `RecordId`, `ScalarMap`, `ScalarValue`, `ColumnDef`,
  `ColumnType`, `Edge`, `EdgeSet`, `Embedding`, `EmbeddingMeta`, `Metric`,
  `BlobPointer`, `EncryptionConfig`, `Snapshot`, `Row`.
