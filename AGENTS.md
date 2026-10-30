# AGENTS.md — RuscaDB

Base de datos **embebida, multi-modelo y multimodal** en Rust (SQLite para IA,
grafos y multimedia). Sin servidor, sin red. Fuente de verdad de diseño:
`docs/RuscaDB-roadmap.md`. Metodología: **SDD/SDAD + TDD adversarial + mutation
testing** (`specs/<feature>.md` antes del código).

## Mapa del repositorio

- `crates/ruscadb-core/` — dominio: `Record`, `ScalarValue`, `RuscaError`, `RecordId`.
- `crates/ruscadb-storage/` — buffer pool LRU-K + `PagedFile`.
- `crates/ruscadb-wal/` — WAL global (frames CRC32C, recovery, group commit, cifrado SPEC-0013).
- `crates/ruscadb-query/` — RQL: lexer + parser + IR (`SELECT`/`WHERE`/`KNN`/`TRAVERSE`/`EXPLAIN`/`MATCH`).
- `crates/ruscadb-vector/` — HNSW. `crates/ruscadb-graph/` — CSR.
- `crates/ruscadb-fvs/` — filtro híbrido pre/in/post + iFVS. `crates/ruscadb-btree/` — B+tree ordenado.
- `crates/ruscadb-multimodal/` — blob store CAS. `crates/ruscadb-fts/` — índice invertido + BM25.
- `crates/ruscadb-ai/` — embeddings locales (+ `static-embeddings`, SPEC-0083).
- `crates/ruscadb-ts/` — time-series (bucketing, ventanas, remuestreo).
- `crates/ruscadb-crypto/` — XChaCha20-Poly1305 + Argon2id + keystore KMS-lite (SPEC-0088).
- `crates/ruscadb-txn/` — MVCC snapshot isolation + manifiesto.
- `crates/ruscadb-mcp/` — servidor MCP por stdio (tools/resources + `session/auth`).
- `crates/ruscadb/` — **fachada / composition root** (cablea todo, `Database`) + `examples/` ejecutables (SPEC-0087).
- `crates/ruscadb-ffi|py|node|wasm/` — bindings (C-ABI + wrappers). `crates/ruscadb-testkit/` — utilidades de test.
- `crates/xtask/` — `cargo xtask trace` / `contract`.
- `specs/` — specs SDD. `scripts/` — guards y utilidades.

## Reglas de arquitectura (hexagonal)

- Las flechas de dependencia apuntan **siempre hacia `ruscadb-core`**.
- Un adapter **no** depende de otro adapter; la composición vive en la fachada.
- Guard obligatorio: `python scripts/check_architecture.py` (0 ciclos, 0 aristas prohibidas).
- `unsafe` solo en `ruscadb-ffi` (presupuesto en `unsafe-allowlist.toml`); todo
  bloque con `// SAFETY:`. `python scripts/check_core_unsafe.py`.

## Gate T1 (antes de commit)

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features   # RUSTDOCFLAGS=-D warnings
python scripts/check_architecture.py
python scripts/check_core_unsafe.py
python scripts/check_ci_config.py
python scripts/check_specs.py
cargo xtask trace
cargo xtask contract
```

Cada AC de una spec debe tener un test `test_ac_XXXX_NN_*` (lo exige
`cargo xtask trace`). Mutation score ≥ 70% para merge, ≥ 85% nightly.

## Aislamiento anti-contaminación (OBLIGATORIO)

Ver `.opencode/config/rust-isolation.md`. Resumen:

- **Nunca** compartas `CARGO_TARGET_DIR` entre agentes (el plugin
  `.opencode/plugin/cargo-isolation.js` lo aísla por sesión; manual:
  `pwsh scripts/isolated-cargo.ps1 <args>`).
- Mutación **sin** `--in-place`; si lo usas, `cargo clean -p <crate>` antes y después.
- `cargo clean -p <crate>` antes de verificar si sospechas contaminación.
- En trabajo paralelo: `cargo fmt -p <crate>` (nunca `--all`) y **un crate por agente**.

## Convenciones

- Código en inglés; docstrings/README/commits en español.
- Conventional Commits: `type(scope): descripción`.
- Docstrings ES con `Args/Returns/Raises`; sin `unwrap/expect` en producción.
- `#![forbid(unsafe_code)]` salvo `ruscadb-ffi`.
- `README.md` es **mantenido a mano** (público, inglés): **no** lo regeneres con
  `SWARMIND/scripts/deploy_all.py`; en este proyecto ejecútalo con `--sync-only`.
- Información **local-only** (no se pushea): ADRs en `docs/adr/` y el mirror
  `.opencode/` (salvo `.opencode/config/` y `.opencode/plugin/`). Al copiar el
  proyecto a otro disco, inclúyelos aparte si quieres preservarlos.
