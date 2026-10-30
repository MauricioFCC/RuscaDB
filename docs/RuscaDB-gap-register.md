# RuscaDB — Registro de gaps y madurez (2026-10-29)

> Inventario del estado real del repositorio tras cerrar la Ola 14. Cada spec de
> frontera (SPEC-0161..0180) se clasifica por **madurez** para evitar el
> anti-patron "implemented de nombre".
>
> Convencion:
>
> - **real**: implementacion funcional con tests de propiedad/round-trip, sin
>   dependencias externas, lista para integrarse.
> - **stub**: API cableada con logica simplificada.
> - **placeholder**: no ofrece seguridad/funcionalidad real; feature off por
>   defecto y spec en `proposed`.
>
> Gates verificados: clippy, fmt, trace (568 AC), contract, architecture,
> unsafe, CI config — todos verdes.

## 1. Inventario del repositorio

| Componente | Cantidad |
|---|---|
| Crates del workspace | 23 |
| Specs SDD (`specs/*.md`) | 176 (175 `implemented`, 1 `proposed`) |
| AC trazados (`cargo xtask trace`) | 568 |
| Tests (archivos `.rs` en tests/) | 143 |
| LOC Rust (aprox.) | 61 000 |
| Docs en `docs/` | 14 archivos (+1 ADR local) |

## 2. Madurez por spec (Ola 14, SPEC-0161..0180)

| Spec | Modulo | Madurez | Evidencia |
|---|---|---|---|
| SPEC-0161 | `RqlExprEngine` | **real** | AST + evaluador con logica trivaluada SQL (AND/OR/NOT, IN, BETWEEN, IS NULL, COALESCE, CASE, aritmetica), profundidad acotada |
| SPEC-0162 | `RqlSubquery` | **real** | escalar con error ante >1 fila, EXISTS, IN trivaluado, limite defensivo |
| SPEC-0163 | `RqlCte` | **real** | punto fijo con UNION/UNION ALL, dedup, deteccion de ciclo, limite de iteraciones |
| SPEC-0164 | `RqlSetOps` | **real** | semantica de bolsa SQL (`min`/`max(0, n_a-n_b)`), orden preservado |
| SPEC-0165 | `RqlWindowFuncs` | **real** | `ROW_NUMBER`, `RANK` (huecos), `DENSE_RANK`, `LAG`/`LEAD`, marcos `ROWS` con NULL |
| SPEC-0166 | `PreparedStmtReal` | **real** | validacion `$n`/`?`, tipos, literales escapados, clave de cache FNV-1a |
| SPEC-0167 | `PageChecksum` | **real** | CRC-32C Castagnoli, vector RFC 3720 `0xE3069283`, sello/verificacion, streaming |
| SPEC-0168 | `SegmentedWal` | **real** | LSN monotono, rotacion antes de escribir, checkpoint, replay, `segment_of` |
| SPEC-0169 | `MvccGcReal` | **real** | horizonte = min(snapshots activos), solo versiones borradas reclamables, metricas |
| SPEC-0170 | `SsiTracker` | **real** | grafo rw, ciclo DFS, estructura peligrosa, abort del pivote, retire |
| SPEC-0171 | `RabitqQuantizer` | **real** | 1 bit/dim, estimador `sqrt(2/(pi*d))`, recall@10 >= 0.90 medido |
| SPEC-0172 | `AcornSearch` | **real** | traversal best-first de subgrafo de predicado, presupuesto acotado, politica por selectividad |
| SPEC-0173 | `PersistentHnsw` | **real** | formato `RHNSW1` versionado con checksum FNV-1a, validacion de cotas, rechazo de truncados/bit-flips |
| SPEC-0174 | `LearnedIndex` | **real** | PGM-greedy con cota epsilon, lookup exacto en distribuciones adversariales |
| SPEC-0175 | `AlpCompressor` | **real** | ALP decimal + FOR + bit-packing + excepciones, exacto bit a bit, fallback crudo |
| SPEC-0176 | `FastLanes` | **real** | bit-packing LSB-first exacto 0..=32, buffer truncado seguro |
| SPEC-0177 | `BlockMaxWand` | **real** | DAAT con poda por bloque; `search == brute_force` para k=1..25 |
| SPEC-0178 | `GorillaTs` | **real** | XOR con ventanas, exacto para NaN/-0.0/inf, sensor estable <2 B/valor |
| SPEC-0179 | `CrdtSyncReal` | **real** | LWW + reloj vectorial, semirreticulo verificado |
| SPEC-0180 | `MlKemStub` | **placeholder** | solo API; feature off por defecto; bloqueado por dependencia auditada (ADR-0180) |

**Resumen**: 19 reales, 0 stubs, 1 placeholder con decision documentada.

## 3. Gaps cerrados en esta ola

1. **RQL completo** (SPEC-0161..0165): primitivas con semantica SQL real
   (3VL, bolsa, marcos, recursividad) en lugar de helpers booleanos.
2. **Prepared statements** (SPEC-0166): validacion de marcadores, tipos y
   escapado de literales; texto de consulta inmutable.
3. **WAL segmentado** (SPEC-0168): rotacion antes de escribir (el ultimo
   registro queda en el segmento nuevo) y checkpoint que preserva el replay.
4. **MVCC GC** (SPEC-0169): regla correcta (`deleted < horizonte`) en vez de
   borrar versiones vivas.
5. **ACORN** (SPEC-0172): traversal real de subgrafo de predicado.
6. **HNSW persistente** (SPEC-0173): formato binario con checksum y validacion.
7. **Block-Max WAND** (SPEC-0177): poda por bloque verificada contra fuerza bruta.

## 4. Gap restante y decision (SPEC-0180)

**ML-KEM (FIPS 203) no se implementa a mano.** Una implementacion casera de
criptografia post-cuantica seria insegura y no pasaria los test vectors de NIST
ACVP. Decision: mantener el placeholder, `status: proposed`, `maturity:
placeholder`, feature `post-quantum` **off por defecto**, y abrir la integracion
cuando se adopte un crate auditado (RustCrypto `ml-kem`). Ver
`docs/adr/ADR-0180-post-quantum-deferred.md`.

## 5. Cableado a adapters (2026-10-29)

Los modulos ya no son solo re-exports: la fachada (composition root) los usa en
sus rutas reales.

| Modulo | Adapter cableado | Punto de uso | Evidencia |
|---|---|---|---|
| `PageChecksum` | page manager | `Database::page_fingerprint()` + `integrity_check` | `page_fingerprint_is_stable` (integracion) |
| `MvccGcReal` | txn / MVCC | `collect_obsolete()` en `Database::reap` | tests de `reap`/GC existentes |
| `CrdtSyncReal` | sync | `SyncProtocol::merge` / `checkpoint` | `merge_converges` + `test_ac_0138_01` |
| `RabitqQuantizer` | HNSW / vector | `Database::search_vectors_quantized` (2 etapas + rerank) | `quantized_search_matches_exact_l2` |
| `GorillaTs` | time-series | `Database::ts_compress` / `ts_decompress` | `ts_gorilla_compression_roundtrip` |
| `PreparedStmtReal` | query | validacion de marcadores/tipos y escapado | `prepared_real` (8 tests) |
| `AlpCompressor` + `FastLanes` | columnar | `columnar::ColumnChunk` + `ColumnarTable::encode_int_column/encode_float_column` | `int_column_roundtrip`, `float_column_roundtrip`, `table_columns_compress` |
| `LearnedIndex` | planner | `CostEstimator::learned_range` + `Database::estimate_distinct_range` | `learned_range_matches_exact_count`, `estimate_distinct_range_uses_learned_index` |
| `AcornSearch` | HNSW | `HnswIndex::neighbors/entry_point` + `TableIndexes::acorn_search` + `Database::search_vectors_filtered` | `acorn_search_respects_allowed` |
| `BlockMaxWand` | FTS (`ruscadb-fts`) | `InvertedIndex::search_wand` (posting lists por termino) | `search_wand_matches_search`, `search_wand_skips_tombstones` |
| `PersistentHnsw` | snapshot vectorial | `TableIndexes::vector_snapshot` + `Database::save_vector_snapshot/load_vector_snapshot` | `vector_snapshot_roundtrip` |
| `SsiTracker` | txn (`ruscadb-txn`) | `TxnManager::enable_ssi/record_read` + abort en `commit` | `ssi_aborts_dangerous_structure_on_commit`, `ssi_tracker_records_edges` |

### Notas de arquitectura

`SsiTracker` y `BlockMaxWand` se **movieron** a `ruscadb-txn` y `ruscadb-fts`
respectivamente (un adapter no depende de la fachada); la fachada los
re-exporta para no romper la API ni la trazabilidad de AC. `PersistentHnsw`,
`AcornSearch`, `LearnedIndex`, `RabitQ`, `Gorilla`, `ALP` y `FastLanes` siguen
en la fachada porque son politicas del composition root sobre adapters ya
existentes (page manager, HNSW, planner, TS, columnar).

### Pendiente (no bloqueante)

- Cablear `AlpCompressor` al almacenamiento columnar **en disco** cuando exista
  el formato fisico de columna (hoy el adaptador es en memoria).
- Exponer `search_vectors_filtered`/`search_vectors_quantized` en el lenguaje
  RQL (hoy son API de la fachada y del FFI pendiente).

## 6. Mutation testing (modulos nuevos)

Comando por modulo (filtra los tests del modulo para que cada mutante sea
barato):

```text
cargo mutants -p ruscadb --file crates/ruscadb/src/<modulo>.rs \
  --timeout 120 -j 4 -- --lib <modulo>::tests
```

Resultado medido en `segmented_wal` (run parcial interrumpido por presupuesto):
**12 capturados / 14 generados = 85,7%**. Los 2 supervivientes estaban en
`SegmentedWal::append` (`&&`→`||` y `>=`→`<`); se anadieron
`append_does_not_rotate_below_target` y `append_rotates_at_exact_boundary`, que
los matan por construccion.

El resto de modulos nuevos anade tests de rama en este ciclo (NULL/3VL, float
no finito, horizonte MVCC, listas disjuntas/vacias, mascara corta, magic
incorrecta, serializacion determinista). El gate T2/T3
(`cargo mutants --in-diff` / `--workspace`) sigue siendo el juez final: el
rebuild por mutante (~85 s) excede el presupuesto de una sesion interactiva.

## 7. Siguiente paso recomendado

Completar el cableado pendiente moviendo los modulos a su crate dueno, y
ejecutar el gate de mutacion completo en nightly (objetivo >= 85%).

## 8. Verificacion de gates (2026-10-29)

- `cargo fmt --all -- --check`: OK.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: OK.
- `cargo test -p ruscadb --all-features`: OK (211 lib tests + integracion).
- `cargo xtask trace`: 568 AC trazados.
- `cargo xtask contract`: OK.
- `scripts/check_architecture.py`: 0 ciclos, 0 aristas prohibidas.
- `scripts/check_core_unsafe.py`: OK (presupuesto 76).
- `scripts/check_ci_config.py`: OK.
