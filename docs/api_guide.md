# Guía de API de RuscaDB

> Guía para terceros (SPEC-0087). RuscaDB es una base de datos
> **embebida, multi-modelo y multimodal**: sin servidor, sin red; los datos
> viven en un único directorio. Esta guía muestra la fachada `ruscadb` con
> ejemplos ejecutables; la referencia completa está en el rustdoc
> (`cargo doc --open`) y la verificación en [verification.md](verification.md).

## Ejecutar los ejemplos

Los tres ejemplos crean su base en un directorio temporal y la borran al
salir; no tocan tu directorio de trabajo:

```bash
cargo run --example quickstart        # examples/quickstart.rs
cargo run --example vector_search     # examples/vector_search.rs
cargo run --example graph_traverse    # examples/graph_traverse.rs
```

En CI se ejecutan de verdad (no solo compilan) vía
`crates/ruscadb/tests/examples_smoke.rs`.

## Abrir una base

```rust
use ruscadb::{Database, DbConfig};

let mut db = Database::open(DbConfig::new("mis_datos.db", 64))?;
```

- `Database::open(DbConfig)` — abre (o crea) la base en `path` con un
  buffer pool de `pool_capacity` páginas.
- `ruscadb::DatabaseBuilder` (`Database::builder()`) — caminos
  avanzados: cifrado en reposo (`encryption`), política de fsync del WAL
  (`fsync_policy`, ver `FsyncPolicy`) y rutas de blobs (`blob_path`).
- `db.close()` — cierra con flush; **siempre** cierra antes de borrar el
  directorio (en Windows los ficheros abiertos bloquean el borrado).

## Tablas y DML (RQL)

Las columnas se declaran con `create_table` + `ColumnDef`/`ColumnType`:

```rust
use ruscadb::{ColumnDef, ColumnType};

db.create_table(
    "personas",
    vec![
        ColumnDef { name: "id".into(), col_type: ColumnType::Int },
        ColumnDef { name: "nombre".into(), col_type: ColumnType::Text },
    ],
)?;
```

El DML/DQL va por `Database::execute` con **RQL** (sintaxis SQL-like):

```rust
db.execute("INSERT INTO personas (id, nombre) VALUES (1, 'Ana')")?;
let rows = db.execute("SELECT * FROM personas WHERE id = 1")?;
db.execute("UPDATE personas SET nombre = 'Mar' WHERE id = 1")?;
db.execute("DELETE FROM personas WHERE id = 1")?;
```

Ver el flujo completo en `examples/quickstart.rs`.

## Búsqueda vectorial y full-text

Los vectores viven en el registro (`Record.vector`), no en una columna:

```rust
use ruscadb::{Embedding, EmbeddingMeta, Metric, Record, RecordId, RecordMeta, EdgeSet};

db.insert_record("docs", record)?;   // registro con vector: Some(embedding)
db.execute("SELECT * FROM docs KNN embedding <|2|> [0.0, 0.0]")?;
db.execute("SELECT * FROM docs WHERE MATCH(titulo, 'gato')")?;
```

- `KNN embedding <|k|> [..]` — vecinos más cercanos (índice HNSW,
  ordenados por distancia).
- `MATCH(columna, 'término')` — full-text con BM25; puede combinarse con
  `WHERE` y en la API híbrida (`HybridHit`/`HybridStage`).

Ver `examples/vector_search.rs`.

## Grafos

Las aristas salen del registro (`Record.edges`):

```rust
use ruscadb::{Edge, EdgeSet, RecordId};

let edge = Edge { label: "link".into(), node: objetivo };
// Record { edges: EdgeSet { out: vec![edge], incoming: vec![] }, .. }
db.execute("SELECT * FROM nodes TRAVERSE edges DEPTH 2")?;
```

`TRAVERSE` recorre las aristas desde la raíz y `DEPTH` acota la
profundidad; los nodos inalcanzables quedan fuera. Ver
`examples/graph_traverse.rs`.

## Seguridad y operación

- Cifrado en reposo (`EncryptionConfig` + Argon2id/XChaCha20-Poly1305),
  RBAC por sesión (`Role`) y auditoría: ver `SECURITY.md` y
  `docs/operacion.md`.
- El servidor MCP (solo lectura por defecto) exige `session/auth` con
  token de sesión (SPEC-0088); ver `SECURITY.md`.

## Siguientes pasos

- `docs/operacion.md` — backup, restauración y upgrades.
- `docs/verification.md` — gates de calidad que dan fe de este código.
- Tests de integración en `crates/ruscadb/tests/` — ejemplos reales de
  cada capacidad (KNN, TRAVERSE, MATCH, MVCC, WAL).
