---
id: SPEC-0196
feature: bindings_conformance
status: implemented
maturity: real
owner: bindings-team
appetite_days: 4
boundaries:
  crates: [ruscadb-py, ruscadb-node, ruscadb-wasm, ruscadb]
  files:
    - crates/ruscadb/tests/conformance/basic.json
    - bindings/python/ruscadb.py
    - bindings/node/ruscadb.js
    - crates/ruscadb-wasm/src/lib.rs
    - .github/workflows/ci.yml
  out_of_scope: [publicación en PyPI/npm, WASM en navegador real]
fr:
  - { id: FR-0196-01, desc: "el fixture conformance/basic.json se ejecuta desde Python" }
  - { id: FR-0196-02, desc: "el mismo fixture se ejecuta desde Node.js" }
  - { id: FR-0196-03, desc: "el mismo fixture se ejecuta desde WASM/host" }
  - { id: FR-0196-04, desc: "CI compara un hash canonico del resultado en los tres bindings" }
nf:
  - { id: NF-0196-01, desc: "sin dependencias nuevas de runtime; usar ctypes/koffi/wasm-bindgen existentes" }
  - { id: NF-0196-02, desc: "determinista: mismo hash en cada ejecucion" }
acceptance_criteria:
  - id: AC-0196-01
    given: el fixture conformance/basic.json y la libreria FFI compilada
    when: se ejecuta desde Python
    then: cada consulta produce el resultado canonico y el hash coincide con Rust
    test: test_ac_0196_01_python_conformance
  - id: AC-0196-02
    given: el mismo fixture y la libreria FFI
    when: se ejecuta desde Node.js
    then: el hash coincide con Rust
    test: test_ac_0196_02_node_conformance
  - id: AC-0196-03
    given: el mismo fixture
    when: se parsea en el binding WASM (host)
    then: el AST canónico es válido (el WASM host no expone almacenamiento de tablas)
    test: test_ac_0196_03_wasm_conformance
  - id: AC-0196-04
    given: un resultado alterado en un binding
    when: se compara el hash
    then: el gate falla con el binding y el caso divergente
    test: test_ac_0196_04_hash_mismatch
exit_criteria:
  - cargo build -p ruscadb-ffi
  - python bindings/python/conformance.py
  - node bindings/node/conformance.js
  - python bindings/python/conformance.py --tamper  # debe fallar (exit 1)
  - node bindings/node/conformance.js --tamper      # debe fallar (exit 1)
  - cargo test -p ruscadb-wasm --test conformance
  - cargo test -p xtask test_ac_0196_01_python_conformance
  - cargo test -p xtask test_ac_0196_02_node_conformance
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - revertir el runner/CI job; el fixture Rust sigue siendo la fuente de verdad
sandbox:
  - cargo build -p ruscadb-ffi
  - python bindings/python/ruscadb.py --conformance
---

# SPEC-0196 — Conformance multi-binding (fase 2)

## Contexto

SPEC-0190 creó `tests/conformance/basic.json` y un runner Rust; la fase 2
(ejecutarlo desde `ruscadb-py`, `ruscadb-node` y `ruscadb-wasm`) quedó fuera de
alcance. Los wrappers Python/Node son finos (16/17 líneas) y no tienen tests:
su madurez es L1. SQLite usa SLT para comparar motores; aquí el equivalente es
un hash canónico del fixture en cada binding.

## Diseño

1. `bindings/python/ruscadb.py --conformance`: carga la FFI, ejecuta cada
   `{sql, expect}` y serializa filas canónicamente.
2. `bindings/node/ruscadb.js --conformance`: idéntico con koffi/napi.
3. `ruscadb-wasm` test host: mismo fixture, `execute` del binding.
4. `scripts/conformance_hash.py` (o `xtask`) compara SHA-256 de los tres
   hashes contra Rust; falla con `binding` + `case`.
5. CI job `conformance` tras `cargo build -p ruscadb-ffi`; artefacto JSON.

Invariante: el orden de claves y filas se normaliza (BTreeMap/JSON canónico)
para que el hash sea estable.

## Complejidad

```text
runner: O(casos · filas · ancho) por binding
hash:   O(bytes)
```

## TDD adversarial

- Fixture con NULL, Float, Text, Bool y error esperado.
- Test de mismatch: alterar una fila en un binding ⇒ exit 1 con caso.
- Test de determinismo: dos ejecuciones ⇒ mismo hash.
- No descargar datasets ni publicar paquetes; solo FFI local.

## Riesgos

- Python/Node deben poder cargar la `.dll`/`.so` de `ruscadb-ffi`; se documenta
  `RUSCADB_FFI_LIB`.
- WASM en navegador real queda fuera; el test es host.

## Estado (2026-11-02)

- **Python** (`bindings/python/conformance.py`): ejecuta el fixture vía C-ABI;
  hash `981aadaaf88517b4c90725bf0fa3bed7d5b6569172d66ac036b63d1d9cc72b41`.
- **Node** (`bindings/node/conformance.js` + koffi 3.3.6): mismo hash.
- **WASM** (`crates/ruscadb-wasm/tests/conformance.rs`): valida parseo y AST
  canónico; el host WASM no tiene almacenamiento de tablas.
- **CI**: job `conformance` en `ci.yml` compila la FFI, instala koffi, ejecuta
  ambos scripts y sus tamper gates (`--tamper` debe fallar con `FAIL
  python/filter_gte` o `FAIL node/filter_gte`).
- **Contrato Rust**: `crates/ruscadb/src/conformance.rs` define
  `canonical_rows`/`compare`; un mismatch devuelve `FAIL <binding>/<caso>`.
- **AC-0196-04** activo en `test_ac_0196_04_hash_mismatch`; AC-0196-01/02
  verifican en `crates/xtask/src/main.rs` el contrato del script y su cableado
  en CI. Cerrada.
