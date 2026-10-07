# Bindings de RuscaDB (F5)

Wrappers delgados sobre el **C-ABI estable** que expone `ruscadb-ffi`
(SPEC-0010). No reimplementan lógica: todo el trabajo ocurre en el motor Rust.

```
crates/ruscadb-ffi/   C-ABI (fn extern "C": open/read_page/write_page/commit/close/last_error)
bindings/python/      wrapper ctypes
bindings/node/        wrapper koffi
crates/ruscadb-py/    placeholder documentado (sin PyO3)
crates/ruscadb-node/  placeholder documentado (sin napi-rs)
```

## Compilar la biblioteca

```bash
cargo build -p ruscadb-ffi
```

El artefacto queda en `target/debug/` (o `target/release/` con
`cargo build --release -p ruscadb-ffi`):

- Windows: `ruscadb_ffi.dll`
- Linux: `libruscadb_ffi.so`
- macOS: `libruscadb_ffi.dylib`

## Python (ctypes)

No requiere dependencias externas.

```python
import sys

sys.path.insert(1, "bindings/python")
import ruscadb  # carga target/{debug,release} o RUSCADB_FFI_LIB

handle = ruscadb.open("mi.db", pool_capacity=8)
page = bytearray(ruscadb.PAGE_SIZE)
page[0] = 42
ruscadb.write_page(handle, 3, bytes(page))
ruscadb.commit(handle)
ruscadb.close(handle)

handle = ruscadb.open("mi.db", pool_capacity=8)
print(ruscadb.read_page(handle, 3)[0])  # 42
ruscadb.close(handle)
```

Ruta explícita de la biblioteca:

```bash
set RUSCADB_FFI_LIB=C:\ruta\a\ruscadb_ffi.dll   # Windows
export RUSCADB_FFI_LIB=/ruta/a/libruscadb_ffi.so # Linux/macOS
```

## Node.js (koffi)

Instala la única dependencia del wrapper:

```bash
npm install koffi
```

```js
const ruscadb = require("./bindings/node/ruscadb.js");

const handle = ruscadb.open("mi.db", 8);
const page = Buffer.alloc(ruscadb.PAGE_SIZE);
page[0] = 42;
ruscadb.writePage(handle, 3, page);
ruscadb.commit(handle);
ruscadb.close(handle);

const reopened = ruscadb.open("mi.db", 8);
console.log(ruscadb.readPage(reopened, 3)[0]); // 42
ruscadb.close(reopened);
```

## Contrato del C-ABI

| Función | Firma resumida |
|---|---|
| `ruscadb_open` | `(data_path, pool_capacity, out_handle) -> c_int` |
| `ruscadb_read_page` | `(handle, page_id, out_buf, buf_len) -> c_int` |
| `ruscadb_write_page` | `(handle, page_id, in_buf, buf_len) -> c_int` |
| `ruscadb_commit` | `(handle) -> c_int` |
| `ruscadb_close` | `(handle) -> c_int` |
| `ruscadb_last_error` | `(out_buf, buf_len) -> usize` |

Códigos de retorno: `0=OK`, `1=puntero nulo`, `2=handle inválido`,
`3=error de dominio/E-S`, `4=panic capturado`.

El ABI usa un registro global de handles vivos (anti use-after-free/doble-free)
y captura panics en la frontera (`catch_unwind`); ningún panic cruza el ABI.
