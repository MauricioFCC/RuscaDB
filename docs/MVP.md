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
  `score: Float`, …), `insert` escalar con coerción `Int→Float`, índice
  secundario (`create_index`, una columna por tabla) y RQL
  (`SELECT *`, `WHERE`, proyección, `LIMIT`).
- **DML en RQL**: `INSERT INTO t (cols) VALUES (...)` (multi-fila = un solo
  commit vía `insert_many`), `UPDATE t SET col = ... [WHERE ...]` y
  `DELETE FROM t [WHERE ...]`; `execute` devuelve `{"affected": N}`.
- **Analítica en RQL**: `ORDER BY <col> [ASC|DESC]` (NULL al final en ASC) y
  `GROUP BY <col>` con agregados `COUNT(*)`/`COUNT(col)`/`SUM`/`AVG`/`MIN`/`MAX`
  (hash aggregation; sin `GROUP BY` = una sola fila global).
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
- **Cifrado en reposo**: `EncryptionConfig::new(clave)` o
  `EncryptionConfig::from_passphrase(..)`; el WAL se cifra (AEAD) y en disco no
  queda ningún claro verificable.

## Límites

- **Single-writer**: una única conexión de escritura por fichero; sin modo
  servidor ni acceso concurrente multi-proceso.
- **Sin streaming ni paginación por cursor**: `execute` materializa todas las
  filas del resultado en memoria.
- **Un índice secundario por tabla** (una columna); sin optimizador de consultas
  más allá de `IndexScan` por igualdad.
- **Sin GC en grafo/FTS**: `reap` purga el heap; los índices derivados
  (HNSW/CSR/invertido) excluyen las filas borradas vía tombstones desde el
  borrado lógico, pero no se compactan.
- **Rollback no soportado en cifrado**: en modo cifrado las páginas viven en el
  WAL y nunca se publican a `.data`; `rollback` devuelve `InvalidConfig` (cerrar
  y reabrir para volver al último estado confirmado).
- **Pool dimensionado en cifrado**: las páginas cifradas permanecen `dirty` en
  el pool (nunca se desalojan); un pool lleno falla con `BufferPoolFull`.
- **Sin rotación de claves ni KMS/HSM**; el catálogo está limitado a la región
  reservada (16 páginas = 64 KiB).
- **RQL acotado**: sin `JOIN` ni subconsultas; `ORDER BY` de una sola clave
  (sin `NULLS FIRST/LAST`); `GROUP BY` por columna, sin `HAVING` ni `DISTINCT`;
  DML sin `UPSERT`/`MERGE`/`RETURNING`; el orden de `MATCH` lo fija BM25 y el de
  `KNN` la distancia. Los operadores `->`/`@>` no usan índice (scan por fila).

## API estable

Superficie cubierta por la suite del MVP (todo lo demás son internals):

- Apertura: `Database`, `DbConfig`, `Database::builder()`.
- Escritura: `create_table`, `insert`, `insert_record`, `insert_many`, `delete`,
  `create_index`, `commit`, `rollback`, `close`, `put_blob`.
- Lectura: `execute` (incluye DML, `ORDER BY`, `GROUP BY` y operadores de
  documento), `execute_at`, `tables`, `catalog`, `get_record`, `reap`,
  `get_blob`, `gc_blobs`.
- Transacciones: `begin`, `snapshot`, `active_tx`, `manifest`, `last_lsn`.
- Tipos: `Record`, `RecordId`, `ScalarMap`, `ScalarValue`, `ColumnDef`,
  `ColumnType`, `Edge`, `EdgeSet`, `Embedding`, `EmbeddingMeta`, `Metric`,
  `BlobPointer`, `EncryptionConfig`, `Snapshot`, `Row`.
