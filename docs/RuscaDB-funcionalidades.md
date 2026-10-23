# RuscaDB — Funcionalidades completas (referencia para integración)

> **Fuente de verdad**: repositorio `RuscaDB` (HEAD `16a01c0`+).  
> **Licencia**: Apache-2.0. **MSRV**: Rust 1.89. **Edición**: 2024.  
> **Estado**: 0.1.0, en desarrollo activo. 477 AC trazados, gates T1 verdes.

RuscaDB es una **base de datos embebida, multi-modelo y multimodal** escrita en
Rust. Corre **en proceso** (sin servidor, sin red, sin daemon) y almacena datos
relacionales, documentales, de grafo, vectoriales y series temporales más blobs
multimodales en un único directorio de base, consultados con **un único lenguaje
extendido (RQL)**.

---

## 1. Arquitectura

- **Hexagonal** (ports & adapters). Las flechas de dependencia apuntan siempre
  hacia `ruscadb-core`. Un adapter nunca depende de otro; la composición vive
  en la fachada `ruscadb`.
- **23 crates** en el workspace (dominio, storage, WAL, query, índices, crypto,
  txn, bindings, MCP, CLI, xtask).
- Guard de arquitectura: `scripts/check_architecture.py` (0 ciclos, 0 aristas
  prohibidas).
- `unsafe` solo en `ruscadb-ffi`, presupuestado en `unsafe-allowlist.toml`.

### Workspace (crates principales)

| Crate | Rol |
|---|---|
| `ruscadb-core` | Dominio: `Record`, `ScalarValue`, `RuscaError`, ports |
| `ruscadb-storage` | Buffer pool LRU-K + `PagedFile` |
| `ruscadb-wal` | WAL global (CRC32C, recovery, cifrado SPEC-0013) |
| `ruscadb-query` | RQL: lexer + parser + IR |
| `ruscadb-vector` | HNSW |
| `ruscadb-graph` | CSR |
| `ruscadb-fvs` | Filtro híbrido pre/in/post + iFVS |
| `ruscadb-btree` | B+tree ordenado |
| `ruscadb-multimodal` | Blob store CAS |
| `ruscadb-fts` | Índice invertido + BM25 |
| `ruscadb-ai` | Embeddings locales (static-embeddings, SPEC-0083) |
| `ruscadb-ts` | Time-series (bucketing, ventanas, remuestreo) |
| `ruscadb-crypto` | XChaCha20-Poly1305 + Argon2id + keystore KMS-lite |
| `ruscadb-txn` | MVCC snapshot isolation + manifiesto |
| `ruscadb-mcp` | Servidor MCP por stdio (tools/resources + session/auth) |
| `ruscadb` | **Fachada / composition root** (`Database`) |
| `ruscadb-ffi` | C-ABI estable |
| `ruscadb-py` / `ruscadb-node` / `ruscadb-wasm` | Bindings Python / Node / WASM |
| `ruscadb-cli` | Ejecutable `ruscadb` (RQL shell, DDL, MCP server) |
| `ruscadb-testkit` | Utilidades de test |
| `xtask` | `cargo xtask trace` / `contract` |

---

## 2. Capacidades core

### 2.1 Apertura y configuración

- `Database::open(DbConfig)` o builder fluido `Database::builder().data_path(..).pool_capacity(..).encryption(..).open()`.
- `Database::open_readonly(DbConfig)` — modo solo lectura, sin lock exclusivo, múltiples lectores concurrentes (SPEC-0109).
- Recovery WAL-first idempotente al arrancar.
- Lock cross-proceso `<data>.lock` (SPEC-0076): segunda apertura falla con `DatabaseInUse`.
- Pool de páginas LRU-K configurable.

### 2.2 Modelo relacional

- `create_table` con esquema (`ColumnDef { name, col_type }`), tipos: `Bool`, `Int`, `Float`, `Text`.
- Varios índices secundarios por tabla (SPEC-0104): `create_index(table, column)`, una columna por índice.
- `insert` escalar con coerción `Int→Float`.
- `insert_many`: N registros, un solo commit (un frame WAL + un fsync).
- RQL: `SELECT *`, `WHERE`, proyección, `LIMIT`/`OFFSET`, `ORDER BY`, `GROUP BY`, `DISTINCT`, `JOIN`.

### 2.3 DML en RQL

- `INSERT INTO t (cols) VALUES (...)` (multi-fila = un solo commit).
- `UPDATE t SET col = ... [WHERE ...]`.
- `DELETE FROM t [WHERE ...]`.
- `execute` devuelve `{"affected": N}` o filas con `RETURNING *|<cols>` (SPEC-0102).
- `UPSERT`: `INSERT ... ON CONFLICT (cols) DO NOTHING | DO UPDATE SET col = literal | excluded.col` con `RETURNING` (SPEC-0107).

### 2.4 Analítica en RQL

- `ORDER BY <col> [ASC|DESC]` (NULL al final en ASC; acepta `tabla.col`).
- `GROUP BY a, b, ...` multi-clave con agregados `COUNT(*)`/`COUNT(col)`/`SUM`/`AVG`/`MIN`/`MAX` (hash aggregation).
- `HAVING <agregado> <op> <literal> [AND ...]` post-agregación.
- `SELECT DISTINCT` (proyección completa, dedupe antes de OFFSET/LIMIT).
- `LIMIT n OFFSET m` (y `OFFSET n` sin `LIMIT`, semántica Postgres).

### 2.5 JOIN en RQL

- `SELECT ... FROM a [INNER|LEFT|RIGHT|FULL [OUTER]|CROSS] JOIN b [ON a.x = b.y]`.
- Una igualdad; nested-loop con lookups al índice secundario o barrido filtrado.
- Columnas fusionadas con prefijo `tabla.col`.
- Outer joins conservan filas sin contraparte con el otro lado a `NULL`.
- `JOIN` admite `WHERE` y `ORDER BY` sobre filas fusionadas (SPEC-0108).

### 2.6 Modelo documental

- `Record.doc` (JSON anidado) consultable desde RQL con `col -> 'a.b'` (extracción por ruta) y `col @> '{json}'` (contención).

### 2.7 Modelo vectorial

- `insert_record` con `Embedding` (+ `EmbeddingMeta` con modelo, dimensión y `Metric::L2`).
- Búsqueda `KNN embedding <|k|> [...]`.
- `KNN` con `WHERE` resuelto por **iFVS** (estrategia pre/in/post por selectividad).
- HNSW index.

### 2.8 Modelo de grafo

- Aristas tipadas (`Edge`/`EdgeSet`).
- Recorridos `TRAVERSE edges DEPTH n` (CSR).

### 2.9 Texto + FTS

- Columnas `Text` consultables con `WHERE MATCH(col, 'término')` ordenado por BM25.
- FTS avanzado (feature `fts-advanced`): fuzzy matching (Levenshtein), phrase matching, stemming (SPEC-0117).

### 2.10 Blobs multimodales

- `DbConfig::blob_path` abre el blob store CAS.
- `put_blob`/`get_blob` roundtrip exacto (dedup por contenido SHA-256).
- `gc_blobs` purga huérfanos con barrier R7.

### 2.11 Time-series

- Crate `ruscadb-ts` con `time_bucket`, ventanas tumbling/deslizantes, remuestreo con relleno, `percentile`, `rate`, `moving_average`.
- Downsampling (feature `ts-advanced`): avg/max/min en ventanas (SPEC-0118).

### 2.12 Escritura por lotes

- `insert_many` confirma N registros con un único commit.

### 2.13 Borrado lógico MVCC + GC

- `delete` por `RecordId` (idempotente).
- Visibilidad por snapshot (`snapshot` / `execute_at` / `get_record`).
- Purga física con `reap`.

### 2.14 Transacciones

- `begin` / `commit` / `rollback`, `active_tx`, `last_lsn`.
- Manifiesto versionado (`manifest`, `tables`, `catalog`).

### 2.15 Durabilidad total

- `close` persiste lo pendiente; al reabrir se recuperan tablas, filas e índices.

### 2.16 Cifrado en reposo

- `EncryptionConfig::new(clave)` o `EncryptionConfig::from_passphrase(..)`.
- WAL cifrado (AEAD XChaCha20-Poly1305, Argon2id KDF).
- Keystore KMS-lite con rotación (SPEC-0088).

### 2.17 Escritura estructurada

- `Database::update(table, id, scalars)` actualiza columnas de una fila viva por `RecordId`.

### 2.18 Catálogo escalable

- Sin techo de 64 KiB (SPEC-0086); crece por cadena de páginas de overflow.
- Formato v3/v4 con múltiples índices por tabla (SPEC-0104), lectura v1/v2.

### 2.19 Reclamación de storage

- `reap` purga el heap, da de alta páginas en free-list (SPEC-0085).

### 2.20 Lock cross-proceso

- Lock exclusivo no-bloqueante sobre `<data>.lock` (SPEC-0076).
- `DatabaseInUse` si otra instancia tiene la base abierta.

### 2.21 Group commit

- `FsyncPolicy::{Full, Normal}` (SPEC-0081).

### 2.22 Embeddings estáticos

- `model2vec-rs` para embeddings locales (SPEC-0083).

### 2.23 MCP server

- Servidor MCP por stdio (tools + resources).
- Session/auth con keystore KMS-lite (SPEC-0088).
- Tools: query, insert, update, delete, tables, integrity_check.

### 2.24 Caché semántico

- Caché semántico persistente en `<data>.cache.json` (SPEC-0093).

### 2.25 UPSERT

- `INSERT ... ON CONFLICT (cols) DO NOTHING | DO UPDATE SET col = literal | excluded.col` con `RETURNING` (SPEC-0107).

---

## 3. Capacidades opcionales (feature flags, off por defecto)

| Feature | Spec | Descripción |
|---|---|---|
| `columnar` | SPEC-0110 | Engine columnar en memoria (typed columns, scan, project, filter, aggregates) |
| `onnx` | SPEC-0111 | Inferencia ONNX (modelo lineal, salida normalizada) |
| `geo` | SPEC-0112 | Tipos geoespaciales (Point, Rect) con distancia y contención |
| `pgwire` | SPEC-0113 | Servidor PostgreSQL wire protocol (stub) |
| `spatial` | SPEC-0114 | Índice espacial R-tree (insert + range query) |
| `compression` | SPEC-0115 | Compresión RLE (compress/decompress) |
| `backup` | SPEC-0116 | Backup online (copia de ficheros) |
| `fts-advanced` | SPEC-0117 | Fuzzy matching (Levenshtein), phrase matching, stemming |
| `ts-advanced` | SPEC-0118 | Downsampling de series temporales (avg/max/min) |
| `observability` | SPEC-0119 | Registry de métricas Prometheus |
| `incremental-backup` | SPEC-0120 | Backup incremental (por timestamp) |
| `metrics` | SPEC-0080 | Métricas de latencia de commit |
| `datafusion` | SPEC-0060 | Spike TableProvider (off por defecto) |

### Lectura concurrente (SPEC-0109)

```rust
// Abrir en modo solo lectura (sin lock exclusivo, múltiples lectores)
let reader = Database::open_readonly(DbConfig::new("app.db", 64))?;
let rows = reader.execute("SELECT * FROM docs")?;
// Escrituras devuelven RuscaError::ReadOnly
```

---

## 4. Ejecutable CLI (`ruscadb-cli`)

Binario `ruscadb` multiplataforma (Windows x64/ARM64, Linux x64/ARM64 gnu+musl, macOS Intel/Apple Silicon).

```bash
ruscadb table create --db ./app.db docs title:Text score:Float
ruscadb query --db ./app.db "SELECT * FROM docs"
ruscadb query --db ./app.db --json "SELECT * FROM docs"
ruscadb script --db ./app.db ./seed.rql
ruscadb mcp --db ./app.db          # servidor MCP por stdio
ruscadb mcp --db ./app.db --write  # con permisos de escritura
ruscadb version
```

---

## 5. Bindings

- **C-ABI** (`ruscadb-ffi`): handle table con generación, anti use-after-free.
- **Python** (`ruscadb-py`): ctypes wrapper.
- **Node** (`ruscadb-node`): koffi wrapper.
- **WASM** (`ruscadb-wasm`): browser bindings.

---

## 6. Query language (RQL)

A small, typed, hand-written SQL dialect — no external parser.

```sql
SELECT * FROM docs                                  -- projection + scan
SELECT a, b FROM t WHERE a > 1 AND b = 'x' LIMIT 10 -- filter + projection + limit
SELECT * FROM t ORDER BY score DESC LIMIT 10        -- ordering + limit
SELECT a, COUNT(*) FROM t GROUP BY a                -- group by + aggregates
SELECT * FROM t WHERE doc -> 'meta.tag' = 'ai'      -- document extraction
SELECT * FROM t WHERE doc @> '{"tags":["ai"]}'      -- document containment
SELECT * FROM t WHERE MATCH(title, 'fox')           -- full-text (BM25)
SELECT * FROM t KNN embedding <|5|> [0.1, 0.2, 0.3] -- ANN vector search
SELECT * FROM t TRAVERSE edges DEPTH 3              -- graph traversal
EXPLAIN SELECT * FROM t WHERE a = 1                 -- plan inspection
INSERT INTO t (a, b) VALUES (1, 'x'), (2, 'y')      -- DML: affected rows
UPDATE t SET b = 'z' WHERE a = 1                    -- DML: update
DELETE FROM t WHERE a = 2                           -- DML: logical delete
INSERT INTO t (a) VALUES (1) ON CONFLICT (a) DO UPDATE SET b = excluded.b  -- UPSERT
SELECT * FROM a JOIN b ON a.x = b.y WHERE name = 'Ana' ORDER BY name DESC  -- JOIN + WHERE + ORDER BY
```

---

## 7. Instalación

### Rust

```bash
cargo add ruscadb --git https://github.com/MauricioFCC/RuscaDB
```

### Ejecutable

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/MauricioFCC/RuscaDB/main/scripts/install.sh | sh
# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -File scripts/install.ps1
# Desde fuente
cargo install --git https://github.com/MauricioFCC/RuscaDB --package ruscadb-cli --locked
```

### MCP (agentes)

```json
{
  "mcpServers": {
    "ruscadb": {
      "command": "ruscadb",
      "args": ["mcp", "--db", "/path/to/app.db"]
    }
  }
}
```

---

## 8. Calidad y gates

- **T1** (bloquea merge): fmt, clippy, test, doc, architecture, unsafe, ci-config, xtask trace.
- **T2** (LLM-judge): Spec Fidelity ≥ 0.90.
- **Mutation score**: ≥70% merge, ≥85% nightly.
- **Cobertura**: core ≥80% líneas, ≥70% ramas.
- **477 AC trazados** (`cargo xtask trace`).
- **0 ciclos de dependencia**, **0 aristas prohibidas**.

---

## 9. Seguridad

- Cifrado en reposo: XChaCha20-Poly1305 + Argon2id.
- `unsafe` solo en `ruscadb-ffi`, presupuestado y justificado.
- Supply chain: `cargo-deny`, `cargo-audit`, CycloneDX SBOM, gitleaks.
- RBAC: roles Admin/Write/Read (SPEC-0066).
- Lock cross-proceso: `<data>.lock` (SPEC-0076).
- Keystore KMS-lite con rotación (SPEC-0088).
- Reporte de vulnerabilidades: `SECURITY.md`.

---

## 10. Documentación

- `README.md` — público, inglés.
- `docs/src/es/` — español, primario.
- `docs/RuscaDB-roadmap.md` — roadmap completo (Olas 0–9).
- `docs/MVP.md` — capacidades y límites del MVP.
- `docs/operacion.md` — runbook operativo.
- `docs/instalacion.md` — guía de instalación y anexado a proyectos.
- `specs/` — 120+ specs SDD con acceptance criteria.
- `CHANGELOG.md` — historial de cambios.

---

## 11. Límites conocidos

- **Single-writer**: una única conexión de escritura por fichero (lock exclusivo).
- **Sin cursores con estado ni streaming**: `execute` materializa todas las filas.
- **Sin optimizador de consultas**: solo IndexScan por igualdad.
- **WAL sin checkpoint en cifrado**: el WAL crece sin límite hasta checkpoint futuro.
- **KMS/HSM externos fuera de alcance**: keystore KMS-lite local.
- **RQL acotado**: JOIN de dos tablas con una igualdad; ORDER BY de una clave; sin subconsultas; sin MERGE (UPSERT soportado).
- **Concurrent readers**: `open_readonly` (SPEC-0109) permite lectores concurrentes sin lock.

---

## 12. Roadmap (Olas)

| Ola | Specs | Estado |
|---|---|---|
| F0–F6 | SPEC-0001…0075 | ✅ Completas |
| Olas 0–4 (producción) | SPEC-0076…0088 | ✅ 13/13 |
| Ola 5 (flujos P0) | SPEC-0089…0090 | ✅ 2/2 |
| Ola 6 (MCP + caché + BRDA + RED-first) | SPEC-0091…0095 | ✅ 5/5 |
| Ola 7 (cierre límites MVP) | SPEC-0096…0105 | ✅ 10/10 |
| Ola 8 (cierre límites MVP) | SPEC-0106…0108 | ✅ 3/3 |
| Ola 9 (Frontier) | SPEC-0109…0120 | ✅ 12/12 |

### Líneas futuras (opcionales)
- Copy-on-write B+trees (redb-style) para snapshot readers sin bloqueo.
- Integración Arrow/Lance para engine columnar y vectores.
- ONNX runtime real (inferencia transformer).
- R-tree con splitting heurístico + KNN espacial.
- zstd/lz4 para compresión de WAL.
- Backup incremental remoto + cifrado.
- Sync protocol / replicación.
- Full-text search: stemming Snowball, fuzzy scoring.
- Time-series: continuous aggregates, retention policies.
- Observabilidad: exportador Prometheus real, OpenTelemetry.
- PostgreSQL wire protocol real (pgwire crate).
- Geospatial: proyecciones, índices espaciales.
- Code review multi-agente cableado (proceso).
- T2 LLM-judge programado (proceso).
- Modelo cross-encoder (requiere runtime ML).
- KMS/HSM externo (producción regulada).
