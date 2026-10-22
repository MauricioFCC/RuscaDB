# Instalacion y comandos para anexar RuscaDB a tus proyectos

RuscaDB es una libreria embebida: no hay servidor ni red. Se anexa al proyecto
como dependencia (Rust) o se instala el ejecutable `ruscadb` (CLI + servidor MCP).

## 1. Rust (crate `ruscadb`)

Anadir desde GitHub (un solo comando):

```bash
cargo add ruscadb --git https://github.com/MauricioFCC/RuscaDB
```

Anadir desde una copia local (desarrollo):

```bash
cargo add ruscadb --path ../RuscaDB/crates/ruscadb
```

Con la feature opcional de metricas (SPEC-0080):

```bash
cargo add ruscadb --git https://github.com/MauricioFCC/RuscaDB --features metrics
```

`Cargo.toml` resultante:

```toml
[dependencies]
ruscadb = { git = "https://github.com/MauricioFCC/RuscaDB" }
```

Uso minimo:

```rust
use ruscadb::{Database, DbConfig};

let mut db = Database::open(DbConfig::new("app.db", 256))?;
let rows = db.execute("SELECT * FROM docs")?;
```

## 2. Ejecutable `ruscadb` (CLI + MCP)

### Linux y macOS

```bash
curl -fsSL https://raw.githubusercontent.com/MauricioFCC/RuscaDB/main/scripts/install.sh | sh
```

### Windows (PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install.ps1
```

### Compilar desde fuente

```bash
cargo install --git https://github.com/MauricioFCC/RuscaDB --package ruscadb-cli --locked
```

Comandos disponibles:

```bash
ruscadb table create --db ./app.db docs title:Text score:Float
ruscadb query --db ./app.db "SELECT * FROM docs"
ruscadb query --db ./app.db --json "SELECT * FROM docs"
ruscadb script --db ./app.db ./seed.rql
ruscadb mcp --db ./app.db          # servidor MCP (JSON-RPC 2.0) por stdio
ruscadb version
```

## 3. Servidor MCP para agentes

`ruscadb mcp` expone el protocolo MCP por stdio. Configuracion para clientes
MCP (Claude Code, OpenCode, etc.):

```json
{
  "mcpServers": {
    "ruscadb": {
      "command": "ruscadb",
      "args": ["mcp", "--db", "/ruta/al/app.db"]
    }
  }
}
```

Herramientas expuestas: consulta RQL, listado de tablas, `integrity_check` y
(escritura) `insert`/`update`/`delete` con `--write`.

## 4. Python (binding por ctypes)

```bash
pip install "ruscadb @ git+https://github.com/MauricioFCC/RuscaDB#subdirectory=packaging/pypi"
```

El binding carga la libreria C-ABI compartida (`ruscadb-ffi`); compila el
compartido con:

```bash
cargo build --release -p ruscadb-ffi
```

## 5. Node.js (binding por koffi)

```bash
npm install github:MauricioFCC/RuscaDB#packaging/npm
```

## 6. Plataformas soportadas por los ejecutables

| Plataforma | Objetivo Rust | Artefacto |
| --- | --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc` | `ruscadb-x86_64-pc-windows-msvc.exe` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | `ruscadb-aarch64-pc-windows-msvc.exe` |
| Linux x64 (glibc) | `x86_64-unknown-linux-gnu` | `ruscadb-x86_64-unknown-linux-gnu` |
| Linux ARM64 (glibc) | `aarch64-unknown-linux-gnu` | `ruscadb-aarch64-unknown-linux-gnu` |
| Linux x64 (static) | `x86_64-unknown-linux-musl` | `ruscadb-x86_64-unknown-linux-musl` |
| Linux ARM64 (static) | `aarch64-unknown-linux-musl` | `ruscadb-aarch64-unknown-linux-musl` |
| macOS Intel | `x86_64-apple-darwin` | `ruscadb-x86_64-apple-darwin` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `ruscadb-aarch64-apple-darwin` |

Cada release publica el `.tar.gz`, el checksum `.sha256` y el agregado
`SHA256SUMS`.

## 7. Verificacion local (gate T1)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
python scripts/check_architecture.py
cargo xtask trace
```
