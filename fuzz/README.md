# Fuzzing de RuscaDB (`cargo-fuzz`)

Fuzzing dirigido a las superficies de parsing y recovery (roadmap §6.2/§7.2).
Este directorio es un **workspace Cargo independiente** (tiene su propia tabla
`[workspace]` vacía) y **no se compila ni se ejecuta en CI T1**: libFuzzer solo
está disponible en el toolchain *nightly*.

## Targets

| Target | Superficie | Invariante |
|---|---|---|
| `query_parse` | `ruscadb_query::parse` (RQL) | nunca entra en pánico ante texto arbitrario |
| `wal_recover` | `ruscadb_wal::recover` (WAL) | nunca entra en pánico ante bytes arbitrarios (torn write / tampering) |

## Requisitos

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
```

## Ejecución

Desde este directorio (`fuzz/`):

```bash
# Fuzz continuo (Ctrl-C para parar). Los crashes se guardan en artifacts/.
cargo +nightly fuzz run query_parse
cargo +nightly fuzz run wal_recover

# Con corpus y timeout/workers explícitos.
cargo +nightly fuzz run query_parse -- -max_total_time=3600 -jobs=4

# Reproducir un crash concreto.
cargo +nightly fuzz run wal_recover artifacts/wal_recover/crash-<hash>
```

## Estructura

```
fuzz/
├── Cargo.toml            # workspace independiente (libfuzzer-sys + arbitrary)
├── fuzz_targets/
│   ├── query_parse.rs
│   └── wal_recover.rs
├── corpus/<target>/      # semillas versionables (opcional)
└── artifacts/<target>/   # crashes encontrados (ignorados por git)
```

`arbitrary` está declarado para futuros targets estructurados (p. ej. entradas
tipadas en lugar de `&[u8]`); libFuzzer ya reexporta el trait vía
`libfuzzer_sys::arbitrary`.

## Exit criteria (roadmap §6.5)

Fuzz sin crash durante 7 días antes del release. Cualquier crash se registra
como fallo (FAIL) y se convierte en un test de regresión en el crate dueño.
