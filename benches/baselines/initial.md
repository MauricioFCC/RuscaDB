# Baseline inicial de benches (SPEC-0059)

Referencia de rendimiento para comparaciones entre ramas con `critcmp`. La
medición es **orientativa** (hardware de escritorio, 10 muestras, warm-up
0.5 s, medición 1 s): sirve como punto de partida para el gate T3 P95 ≤ +5 %.

Fecha de captura: 2026-10-10 · Perfil: `release` (workspace `opt-level`
por defecto) · Comando:

```powershell
cargo bench -p ruscadb-wal --bench wal_commit `
  -p ruscadb-vector --bench knn_latency `
  -p ruscadb --bench facade_smoke -- `
  --warm-up-time 0.5 --measurement-time 1 --sample-size 10
```

| Benchmark | p50 (mediana) | rango (p05–p95) |
|---|---|---|
| `facade/point_read_indexed` | 1.427 ms | 1.392 – 1.489 ms |
| `facade/blob_ingest_1mib` | 8.441 ms | 8.234 – 8.690 ms |
| `knn/hnsw_2k_dim128_k10` | 638.65 µs | 627.15 – 656.75 µs |
| `wal/group_commit_c64` | 1.420 ms | 1.387 – 1.448 ms |

Lectura: el ingest de blob 1 MiB rinde ≈ 124 MB/s en este hardware (la meta
aspiracional §6.4 es ≥ 500 MB/s; optimizar está fuera del alcance de
SPEC-0059 — solo medir). El point-read indexado de 10 k filas ronda 1.4 ms.

## Cómo regenerar / comparar entre ramas

```powershell
# 1) Guardar la baseline de tu rama con un nombre estable:
cargo bench --workspace --benches -- --save-baseline main

# 2) En la rama de features, comparar contra esa baseline:
cargo bench --workspace --benches -- --baseline main

# 3) diff textual para el PR (si hay critcmp instalado):
critcmp main
```

Los resultados crudos de criterion quedan en `target/criterion/` (no se
commitean). Esta tabla se actualiza al cambiar de hardware o de semilla de
datos; el gate P95 alert-only de nightly usa el smoke `-- --test`
(`AC-0059-01`), no esta tabla.
