# Recomendaciones de integración Swarmind → RuscaDB (2026-11-06)

> Documento para el equipo de RuscaDB. Resume qué consume Swarmind, qué
> encontró al integrarlo y qué cambios de upstream desbloquean más capacidad.
> El cliente Python vive en `SWARMIND/harness/memory_rag/ruscadb.py`.

## 1. Estado de integración (verificado)

Swarmind es cliente C-ABI de RuscaDB (ctypes, sin reimplementar el motor):

| Capacidad | Estado en Swarmind | Nota |
|---|---|---|
| SQL/RQL, CRUD, transacciones | usado | vía `ruscadb_execute` y records |
| KNN exacto (HNSW) | usado | `search` |
| KNN binario RaBitQ (`search_bq`) | usado | `search_binary` + tuner de oversample |
| **KNN cuantizado** (`search_quantized`) | **usado** | DLL reconstruida (2026-11-06) |
| **KNN filtrado ACORN** (`search_filtered`) | **usado** | requiere `RecordId`, no id lógico |
| FTS BM25 + híbrida + grafo | usado | `full_text_search`, `hybrid_search`, `graph_traverse` |
| Blobs `put/get/attach/gc` | usado | ingesta adjunta PDF/imagen ≤8 MiB |
| Backup/integridad/LSN/explain/time-travel | usado | `!db backup/health/explain/time-travel` |
| Async (writer actor/batcher) | expuesto | métodos async; sin caller de producción aún |
| Prepared nativo | **bloqueado** | el crate lo tiene; el C-ABI **no lo exporta** |
| 3VL / RCOL1 / cifrado | no usado | DLL vieja; evaluar tras rebuild/estabilización |

Evidencia del probe en Swarmind: **22 features, 19 disponibles**; faltan
`ruscadb_prepare`, `ruscadb_execute_prepared`, `ruscadb_search_filtered`,
`ruscadb_search_quantized` (las dos últimas ya existen en HEAD y se cablearon
tras rebuild).

## 2. Benchmarks de referencia (sintético, 64×16, top-5)

| Estrategia | recall@5 | p50 ms |
|---|---|---|
| exact | 1.000 | ~0.0 |
| binary oversample=1 | 0.680 | 0.32 |
| binary oversample=4 | 1.000 | 0.53 |
| quantized | 1.000 | 0.27 |
| filtered (ACORN) | 1.000 | 1.12 |

Conclusión para el cliente: no bajar a 1 bit sin oversample/rescore; `filtered`
paga latencia por el filtro pero mantiene recall. Script:
`SWARMIND/scripts/bench_ruscadb_search.py`.

## 3. Recomendaciones prioritarias (upstream)

### R1 — Exportar prepared statements en el C-ABI (alto impacto)

El crate `ruscadb` ya ofrece `prepare`/`execute_prepared`, pero
`crates/ruscadb-ffi/src/lib.rs` no exporta símbolos. Propuesta:

```c
int ruscadb_prepare(ruscadb_handle *h, const char *sql,
                    char *out_id, size_t id_len);
int ruscadb_execute_prepared(ruscadb_handle *h, const char *prepared_id,
                             const char *params_json,
                             char *out_json, size_t out_len);
```

- `params_json` como array/objeto JSON (mismo contrato que `execute`).
- Error con `RuscaError::code()` para distinguir `parse`, `validation` y
  `timeout` (hoy el cliente solo ve código ABI + mensaje).
- Test de contrato: preparar una query con parámetros, ejecutarla dos veces y
  verificar que no se re-parsea (métrica/plan estable).

### R2 — Documentar el tipo de id de `search_filtered`

`search_filtered` recibe `allowed_json` con **`RecordId`** (ULID), no el id
lógico UUID que usa la capa documental. Swarmind resuelve `logical_id → RecordId`
antes de llamar, pero un cliente nuevo tropezará. Recomendación: documentarlo en
`docs/RuscaDB-funcionalidades.md` y devolver en el error “id inválido” el tipo
esperado.

### R3 — Endpoint de versión/capacidades

Añadir `ruscadb_version()` (semver) o `ruscadb_capabilities_json()` para que los
clientes no tengan que probar `getattr` símbolo a símbolo. Swarmind ya tiene
`!db capabilities`, pero la detección es heurística.

### R4 — Blobs: tamaño máximo y streaming

Swarmind adjunta ≤8 MiB para no inflar la base. Si el motor expone un límite
duro, documentarlo; si no, añadir `ruscadb_put_blob_stream` o un flag de
compresión. También conviene un `blob_usage` para que el cliente decida GC.

### R5 — Estadísticas de índice para búsqueda adaptativa

Para elegir `exact|binary|quantized|filtered` sin benchmark local, el motor
podría exponer: número de vectores, si hay índice cuantizado, `dim`, y último
recall medido. Swarmind hoy decide con `scripts/bench_ruscadb_search.py`.

### R6 — MCP server: contrato de escritura

Swarmind registra `ruscadb mcp` (deshabilitado) como plugin MCP. Recomendación:
documentar en el repo el conjunto de tools y el requisito `--write` (hoy en
`docs/RuscaDB-funcionalidades.md`) y si `mcp` acepta `--db` relativo.

### R7 — 3VL / RCOL1 / cifrado

No se cablean hasta que el ABI/estabilidad esté cerrada. Recomendación: declarar
en el README qué símbolos acompañan a cada SPEC y desde qué versión.

## 4. Contrato que Swarmind ejecuta sobre la DLL

```bash
python -c "from harness.memory_rag.ruscadb import capabilities; print(capabilities())"
python -m harness.reverse_engineering ...   # no aplica
pytest harness/tests/test_ruscadb_store.py harness/tests/test_binary_search.py \
       harness/tests/test_async_ruscadb.py -q
python scripts/bench_ruscadb_search.py --n 64 --dim 16 --queries 5
```

Cualquier rebuild de `ruscadb-ffi` debe pasar esos tests y `!db capabilities`
debe reportar los 4 símbolos nuevos.

## 5. Checklist para el release de RuscaDB

- [ ] `ruscadb_search_filtered`/`search_quantized` documentados con ejemplo Python.
- [ ] `prepare`/`execute_prepared` exportados en el C-ABI + test de contrato.
- [ ] `ruscadb_version()` o capabilities JSON.
- [ ] Límite/streaming de blobs documentado.
- [ ] `ruscadb mcp --db` documentado para clientes MCP.
- [ ] Benchmarks de recall/latencia publicados (no solo tests).
