<!-- hand-maintained README: do NOT regenerate with SWARMIND deploy_all.py;
     run it with `--sync-only` for this project. See AGENTS.md. -->

# RuscaDB

**Embedded, multi-model and multimodal database written in Rust** — *“SQLite for
AI, graphs and multimedia”*.

[![CI](https://github.com/MauricioFCC/RuscaDB/actions/workflows/ci.yml/badge.svg)](https://github.com/MauricioFCC/RuscaDB/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-informational.svg)](rust-toolchain.toml)

RuscaDB runs **in-process** (no server, no network, no daemon) and stores
relational, document, graph, vector and time-series data plus multimodal blobs in
a single database directory, queried through **one extended-SQL language (RQL)**.

> **Status: early development (`0.1.0`).** The engine is under active
> construction; public APIs may change. Design source of truth:
> [`docs/RuscaDB-roadmap.md`](docs/RuscaDB-roadmap.md).

## Highlights

- **Multi-model, one record.** A single physical `Record` projects onto five
  models (relational, document, graph, vector, time-series) plus multimodal
  (`blob` + `embedding`). One storage, many indexes.
- **Durability without a server.** A global write-ahead log (CRC32C frames,
  torn-write recovery) with **MVCC snapshot isolation** and a versioned,
  atomically-written manifest.
- **Search built in.** HNSW vector index with hybrid filtered search
  (pre/in/post by selectivity), CSR graph traversal, BM25 full-text search and an
  ordered B+tree.
- **Multimodal.** Content-addressed blob store (SHA-256, dedup + refcount) and
  local embeddings.
- **Secure by default where it counts.** Optional **XChaCha20-Poly1305**
  encryption at rest (WAL and blobs, Argon2id key derivation); `unsafe` is
  confined to the C-ABI crate and budgeted in `unsafe-allowlist.toml`.
- **Interoperable.** A stable C-ABI (`ruscadb-ffi`) with thin Python (ctypes) and
  Node (koffi) wrappers.

## Quick start

Add the facade crate to your project (path/git for now; not yet published):

```toml
[dependencies]
ruscadb = { git = "https://github.com/MauricioFCC/RuscaDB", package = "ruscadb" }
```

```rust
use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

fn main() -> Result<(), ruscadb::RuscaError> {
    // Open (or create) a database: a data file plus a derived WAL + manifest.
    let mut db = Database::open(DbConfig::new("app.db", 256))?;

    // Declare a table and insert a row (insert auto-commits).
    db.create_table(
        "docs",
        vec![
            ColumnDef { name: "title".into(), col_type: ColumnType::Text },
            ColumnDef { name: "score".into(), col_type: ColumnType::Float },
        ],
    )?;
    db.insert(
        "docs",
        ScalarMap::from([
            ("title".to_string(), ScalarValue::Text("the quick brown fox".into())),
            ("score".to_string(), ScalarValue::Float(0.9)),
        ]),
    )?;

    // Query with RQL.
    let rows = db.execute("SELECT title FROM docs WHERE MATCH(title, 'fox')")?;
    println!("{rows:?}");
    Ok(())
}
```

Vectors, graphs and full-text are queried through the same language:

```rust
// k-nearest neighbours over a record's `embedding` field
db.execute("SELECT * FROM items KNN embedding <|5|> [0.1, 0.2, 0.3]")?;
// graph traversal from a seed node
db.execute("SELECT * FROM nodes TRAVERSE edges DEPTH 2")?;
// inspect the query plan
db.execute("EXPLAIN SELECT * FROM docs WHERE score > 0.5")?;
```

## Query language (RQL)

A small, typed, hand-written SQL dialect (no external parser):

```sql
SELECT * FROM docs                                  -- projection + scan
SELECT a, b FROM t WHERE a > 1 AND b = 'x' LIMIT 10 -- filter + projection + limit
SELECT * FROM t WHERE MATCH(title, 'fox')           -- full-text (BM25)
SELECT * FROM t KNN embedding <|5|> [0.1, 0.2, 0.3] -- ANN vector search
SELECT * FROM t TRAVERSE edges DEPTH 3              -- graph traversal
EXPLAIN SELECT * FROM t WHERE a = 1                 -- plan inspection
```

The IR, parser and grammar live in `ruscadb-query`; the executor lives in the
facade and filters by MVCC snapshot visibility.

## Workspace

| Crate | Role |
|---|---|
| `ruscadb-core` | Domain: `Record`, `ScalarValue`, `RuscaError`, ports. |
| `ruscadb-storage` | LRU-K buffer pool + paged file. |
| `ruscadb-wal` | Global WAL (CRC32C, recovery, encryption). |
| `ruscadb-query` | RQL lexer, parser and IR. |
| `ruscadb-vector` | HNSW index. |
| `ruscadb-fvs` | Hybrid filtered vector search (pre/in/post). |
| `ruscadb-graph` | CSR adjacency + traversal. |
| `ruscadb-fts` | Inverted index + BM25. |
| `ruscadb-btree` | Ordered B+tree. |
| `ruscadb-txn` | MVCC snapshot isolation + versioned manifest. |
| `ruscadb-multimodal` | Content-addressed blob store. |
| `ruscadb-ai` | Local embeddings. |
| `ruscadb-crypto` | XChaCha20-Poly1305 + Argon2id. |
| `ruscadb-ffi` | Stable C-ABI. |
| `ruscadb-py` / `ruscadb-node` | Python / Node bindings. |
| `ruscadb-wasm` | WebAssembly (browser) bindings. |
| `ruscadb` | **Facade / composition root** (wires everything; `Database`). |
| `ruscadb-testkit` | Test utilities and oracles. |
| `xtask` | Developer tasks (`cargo xtask trace`). |

## Architecture

Hexagonal (ports & adapters). **Dependency arrows always point toward
`ruscadb-core`**; an adapter never depends on another adapter — composition lives
in the `ruscadb` facade. The guard `scripts/check_architecture.py` enforces
0 cycles and 0 forbidden edges.

## Security

- Optional encryption at rest (AEAD XChaCha20-Poly1305; Argon2id KDF) for the
  WAL and the blob store; see [`specs/encrypted_at_rest.md`](specs/encrypted_at_rest.md).
- `unsafe` is only allowed in `ruscadb-ffi`, with a per-crate budget and a
  `// SAFETY:` justification on every block. Every other crate is
  `#![forbid(unsafe_code)]`.
- Supply chain: `cargo-deny`, `cargo-audit`, CycloneDX SBOM and gitleaks.
- Report vulnerabilities per [`SECURITY.md`](SECURITY.md) — please do **not** open
  a public issue for security reports.

## Development

Methodology: **spec-first (SDD/SDAD) + adversarial TDD + mutation testing**. Every
feature has a `specs/<feature>.md` with acceptance criteria, and every AC maps to
a `test_ac_XXXX_NN_*` test enforced by `cargo xtask trace`.

Gate **T1** (must be green before commit):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
python scripts/check_architecture.py
python scripts/check_core_unsafe.py
python scripts/check_ci_config.py
cargo xtask trace
```

Mutation score ≥ 70% for merge, ≥ 85% nightly (see `.github/workflows/nightly.yml`).
When several agents build in parallel, isolate the Cargo target directory to avoid
stale-artifact contamination — see `.opencode/config/rust-isolation.md` and
`scripts/isolated-cargo.ps1`.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full workflow.

## License

Licensed under the Apache License, Version 2.0 — see [`LICENSE`](LICENSE).
