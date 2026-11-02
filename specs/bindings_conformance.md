---
id: SPEC-0196
feature: bindings_conformance
status: proposed
maturity: stub
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
    when: se ejecuta en el binding WASM (host)
    then: el hash coincide con Rust
    test: test_ac_0196_03_wasm_conformance
  - id: AC-0196-04
    given: un resultado alterado en un binding
    when: se compara el hash
    then: el gate falla con el binding y el caso divergente
    test: test_ac_0196_04_hash_mismatch
exit_criteria:
  - cargo test -p ruscadb --test spec_0190_conformance
  - python bindings/python/ruscadb.py --conformance
  - node bindings/node/ruscadb.js --conformance
  - cargo test -p ruscadb-wasm --features wasm
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
