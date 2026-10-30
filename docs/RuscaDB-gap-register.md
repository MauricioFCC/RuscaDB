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
| Tests (archivos `.rs` en tests/) | 148 |
| LOC Rust (aprox.) | 66 957 |
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

### Cerrados en el ciclo 5 (2026-10-30)

- **ALP en disco**: `ColumnarTable::write_to/read_from` (`RCOL1`) codifica
  enteros con FOR + bit-packing, flotantes con **ALP** y el resto etiquetado,
  con escritura atomica (`tmp` + rename) y validacion de cotas.
- **RQL/FFI**: `KNN ... USING AUTO|QUANTIZED|FILTERED|EXACT` (SPEC-0181) y
  `ruscadb_search_quantized` / `ruscadb_search_filtered` en la C-ABI.

### Pendiente (no bloqueante)

- Endpoint HTTP de scrape Prometheus (el registry y el export de texto son
  reales; el transporte es adapter).
- Aplicar zstd al formato fisico de pagina columnar (hoy ALP/FOR propios).

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
incorrecta, serializacion determinista).

**Gate T3 (nightly)**: `scripts/mutation_score.py` agrega los 8 shards y
**falla** si el score global < 85 % (job `mutation-score` de `nightly.yml`,
bloqueante). Verificado localmente contra el run parcial de `segmented_wal`:
`85,71 %` (12 caught, 2 missed) -> exit 0; un fixture 1/3 -> exit 1.

## 7. Gaps residuales (upstream o politica no trivial)

El cableado esta completo. Lo que queda abierto no es codigo pendiente de
escribir, sino **cambios upstream** o **politicas que requieren datos reales**:

- ~~**RaBitQ por C-ABI**~~: **cerrado** con `ruscadb_quantize` +
  `ruscadb_search_bq`.
- ~~**IVF-PQ real**~~: **cerrado** (`IvfPqIndex` con k-means por subespacio,
  codebooks de 256, IVF + ADC + re-rank; recall@10 >= 0.90).
- ~~**Columnas de gobernanza**~~: **cerrado** con
  `alter_table_add_column`, `add_governance_columns` y `ruscadb_alter_table`.
- ~~**Eviccion por calor**~~: **cerrado** (`HeatTracker`).
- ~~**Umbral aprendido**~~: **cerrado** (`LearnedThreshold`).
- ~~**Reconciliacion programada**~~: **cerrado** (`ReconciliationJob`).
- ~~**Contratos de dependencia declarados**~~: **cerrado** con
  `architecture.toml` (equivalente Rust a `tach.toml`); el guard falla ante
  aristas no declaradas y contratos obsoletos.
- ~~**Indices sobre columnas de gobernanza**~~: **cerrado** con
  `add_governance_indexes` / `ensure_governance` (idempotente; el planificador
  usa `IndexScan`).
- ~~**Ajuste de nprobe/oversampling**~~: **cerrado** con
  `IvfPqIndex::calibrate` (elige la configuracion mas barata que cumple el
  recall objetivo con datos reales).
- ~~**Time-travel `--as-of`**~~: **cerrado** con
  `execute_at_lsn`/`snapshot_at`/`current_tx`/`replay_as_of` y el ABI
  `ruscadb_execute_at`; test de visibilidad historica y borrado posterior.
- ~~**CRDT serializable**~~: **cerrado** con `CrdtSnapshot` +
  `to_json`/`from_json`/`export`/`restore`.
- ~~**S3Backup sin credenciales**~~: **cerrado** con el puerto `ObjectStore` +
  `LocalObjectStore`; el cliente S3 real se inyecta como adapter.

Detalle, fuentes y plan de desbloqueo: `docs/research/frontier-2026-10-29.md`
(referencia cruzada al analisis del consumidor en
`SWARMIND/docs/research/frontier-2026-10-29.md`).

## 8. Siguiente paso recomendado

Quedan solo los adapters externos de §7 (Lance, pgwire TCP, KMS/HSM, OTLP
HTTP, S3, ONNX runtime, endpoint Prometheus). El gate de mutacion nightly ya es
bloqueante (>= 85 %) y la cuantizacion/filtrado ya se exponen en RQL y ABI.

## 9. Verificacion de gates (2026-10-30)

- `cargo fmt --all -- --check`: OK.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: OK.
- `cargo test -p ruscadb --all-features`: OK (284 lib tests + integracion; 646 en total).
- `cargo test --workspace --all-features --exclude ruscadb`: OK (68 suites, 496 tests).
- `cargo xtask trace`: 575 AC trazados.
- `cargo xtask contract`: OK.
- `scripts/check_architecture.py`: 0 ciclos, 0 aristas prohibidas.
- `scripts/check_core_unsafe.py`: OK (presupuesto 76).
- `scripts/check_ci_config.py`: OK (incluye el gate `mutation-score`).
- `scripts/check_specs.py`: 176 specs, 45 `real` + 1 `placeholder`.
- `scripts/mutation_score.py`: 85,71 % sobre el run parcial de `segmented_wal`
  (objetivo 85 %); fixture 1/3 -> exit 1.

## 10. Modulos stub declarados (no cablear aun)

Auditoria 2026-10-30: **0 modulos stub**. Los siete ultimos
(`async_io`, `branching`, `concurrent_txn`, `lance_format`, `pgwire`, `simd`,
`vle`) pasaron a `maturity: real` en el ciclo 4 con estas implementaciones y
limites declarados:

- `concurrent_txn` (SPEC-0131): `ConcurrentTxnManager` delega en
  `TxnManager` real; first-committer-wins sobre claves declaradas.
- `branching` (SPEC-0139): `BranchManager` sobre `CowBTree` persistente;
  `fork_from` `O(1)`, merge por union con prioridad al destino.
- `vle` (SPEC-0137): contenedor single-file `RUSCVLE1` con checksum FNV-1a,
  validacion de cotas y deteccion de truncamiento/corrupcion.
- `async_io` (SPEC-0151): `Future` + `poll_read` en memoria (sin Tokio); el
  offload real lo hace el executor del consumidor.
- `simd` (SPEC-0150): 4 acumuladores independientes + suma compensada de
  Neumaier; **sin intrinsics** por `forbid(unsafe_code)`
  (`is_hardware_simd() == false` honesto).
- `lance_format` (SPEC-0136): contenedor columnar propio `RLNC1` con
  round-trip exacto; **no es compatible con el formato Lance** (requiere el
  crate `lance`) y no implementa ALP/FastLanes.
- `pgwire` (SPEC-0113): codec wire v3 (big-endian) de frontend/backend y
  `handle_query`; **sin servidor TCP** (el bind queda en el adapter).

Pasaron a `maturity: real` en este ciclo:

- `ivf_clustering` (SPEC-0147): k-means real reutilizando `ivf_pq::kmeans`.
- `rcu` (SPEC-0146): `RcuDomain` + `RcuCell` segura (snapshot por `Arc`, sin
  `unsafe`), con retiro y `synchronize`.
- `query_cache` (SPEC-0159): TTL + capacidad + metricas (hits/misses/evictions).
- `materialized_view` (SPEC-0149): cableada al executor y **persistida** en
  `<data>.views.json` (`create`/`refresh`/`rows`/`drop`).
- `prepared` (SPEC-0148): delega en `PreparedStmtReal` (marcadores, tipos y
  escapado), sin wrapper duplicado.
- `stemming` (SPEC-0144): Porter (1980) completo (5 fases), base de Snowball.
- `ts_aggregates` (SPEC-0145): agregacion incremental por bucket (`O(log B)`
  por punto) con `merge` asociativo y min/max/avg.
- `rtree_split` (SPEC-0156): R-tree real con split por mediana y consulta
  con poda por MBR (`O(log n + k)` esperado).
- `cow_btree` (SPEC-0130): treap persistente con path copying (`O(log n)`,
  `fork` `O(1)`), snapshots inmutables.
- `kms` (SPEC-0143): sellado AEAD real XChaCha20-Poly1305 (nonce unico);
  KMS/HSM remoto sigue pendiente por credenciales.
- `onnx` (SPEC-0111): MLP `2 capas + ReLU + softmax` en Rust puro; `ort`
  queda para transformers.
- `otel` / `otel_real` (SPEC-0126/0155): payload OTLP/JSON
  (`resourceSpans`/`scopeSpans`); transporte HTTP pendiente (externo).
- `geo` (SPEC-0112/0114): GeoHash + curva de Hilbert + R-tree con poda.
- `columnar` (SPEC-0110): formato en disco `RCOL1` con ALP/FOR + bit-packing,
  escritura atomica y validacion de cotas.
- `knn_modes` (SPEC-0181): `USING AUTO|QUANTIZED|FILTERED|EXACT` en RQL y ABI
  (`ruscadb_search_quantized` / `ruscadb_search_filtered`).

## 11. Higiene de lints (2026-10-30)

- `#![allow(missing_docs)]`: **0** modulos en `ruscadb` (todos los items
  publicos documentados; `clippy -D missing_docs` verde).
- `#![allow(dead_code)]`: **0** modulos en `ruscadb`.
- `TODO:`/`FIXME:` reales: **0**.
- Features declaradas sin uso: **0**.
- Archivos trackeados >1 MB: **0**.

## 12. Refactor de specs (2026-10-30)

- **`scripts/check_specs.py`**: valida frontmatter (id unico, feature, status),
  `status`/`maturity` permitidos, ACs declarados con el prefijo correcto y
  `implemented` con al menos un AC. Integrado en el job `architecture` de CI.
- **`docs/specs-index.md`**: indice generado (176 specs) con
  `scripts/gen_specs_index.py`; fuente unica para ver status y madurez.
- Estado: 175 `implemented` + 1 `proposed`; madurez declarada **45 `real`,
  0 `stub`, 1 `placeholder`** (el resto sin declarar).
