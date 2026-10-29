# RuscaDB — Registro de gaps y madurez (2026-10-28)

> Inventario minucioso del estado real del repositorio tras la Ola 14. Cada
> spec de frontera (SPEC-0161..0180) se clasifica por **madurez** para evitar
> el anti-patron "implemented de nombre": un stub con tests no es una
> implementacion de produccion.
>
> Convencion de madurez:
>
> - **real**: implementacion funcional con tests de round-trip/propiedad, sin
>   dependencias externas, lista para integrarse.
> - **stub**: API cableada y tests AC, pero logica simplificada; requiere
>   trabajo adicional antes de produccion.
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
| LOC Rust (aprox.) | 58 000 |
| Features del facade `ruscadb` | 90+ (pure-Rust on por defecto) |
| Docs en `docs/` | 13 archivos |

## 2. Madurez por spec (Ola 14, SPEC-0161..0180)

| Spec | Modulo | Madurez | Evidencia | Siguiente paso |
|---|---|---|---|---|
| SPEC-0161 | `RqlExprEngine` | stub | eval booleano + IN/BETWEEN/COALESCE | Integrar con el parser RQL real |
| SPEC-0162 | `RqlSubquery` | stub | scalar/exists/in | Ejecutar sobre el executor |
| SPEC-0163 | `RqlCte` | stub | punto fijo acotado | Integrar recursivo con el plan |
| SPEC-0164 | `RqlSetOps` | stub | union/intersect/except | Conectar a pipelines de ejecucion |
| SPEC-0165 | `RqlWindowFuncs` | stub | row_number/rank/lag/sum | Integrar con el agregador |
| SPEC-0166 | `PreparedStmtReal` | stub | bind/reuse | Cache real de planes |
| SPEC-0167 | `PageChecksum` | **real** | CRC-32C Castagnoli, vector RFC 3720 `0xE3069283`, sello/verificacion, streaming | Cablear al page manager y WAL |
| SPEC-0168 | `SegmentedWal` | stub | rotacion/checkpoint | Sustituir WAL global cuando crezca |
| SPEC-0169 | `MvccGcReal` | stub | watermark/obsolescencia | Integrar con el GC del storage |
| SPEC-0170 | `SsiTracker` | **real** | grafo rw, ciclo DFS, estructura peligrosa, abort, retire | Integrar en `ruscadb-txn` |
| SPEC-0171 | `RabitqQuantizer` | **real** | 1-bit/dim, estimador con correccion `sqrt(2/(pi*d))`, recall@10 medido >= 0.90 | Conectar a HNSW como storage |
| SPEC-0172 | `AcornSearch` | stub | politica de expansion | Implementar traversal de subgrafo |
| SPEC-0173 | `PersistentHnsw` | stub | serializacion round-trip | Persistir en disco con checksum |
| SPEC-0174 | `LearnedIndex` | **real** | PGM-greedy con cota epsilon, lookup exacto en distribuciones adversariales | Sustituir B+tree en lookups |
| SPEC-0175 | `AlpCompressor` | **real** | ALP decimal + FOR + bit-packing + excepciones, exacto bit a bit, fallback crudo | Integrar en columnar storage |
| SPEC-0176 | `FastLanes` | **real** | bit-packing LSB-first exacto para anchos 0..=32, buffer truncado seguro | Usar en ALP/Gorilla/Parquet |
| SPEC-0177 | `BlockMaxWand` | stub | top-k ordenado | Implementar pruning por bloque |
| SPEC-0178 | `GorillaTs` | **real** | XOR con ventanas, exacto para NaN/-0.0/inf, sensor estable <2 B/valor | Integrar en `ruscadb-ts` |
| SPEC-0179 | `CrdtSyncReal` | **real** | LWW + reloj vectorial, semirreticulo (comm/assoc/idemp) | Integrar en `ruscadb-sync` |
| SPEC-0180 | `MlKemStub` | **placeholder** | solo API, feature off por defecto | Integrar `RustCrypto/ml-kem` (FIPS 203) |

**Resumen**: 8 reales, 11 stubs, 1 placeholder. Ningun stub se declara `implemented` a secas sin la marca `maturity`.

## 3. Gaps abiertos (priorizados por ROI)

### P0 — Alto impacto, esfuerzo medio

1. **Integracion de los 8 modulos reales** con sus adapters (page manager, HNSW,
   storage columnar, txn, TS, sync). Hoy viven aislados en el facade.
2. **ML-KEM real**: feature `post-quantum` off por defecto hasta integrar un
   crate auditado; no se puede declarar `implemented`.
3. **Cobertura de mutation**: los modulos nuevos tienen tests de round-trip pero
   falta mutation score >= 70% (gate nightly).

### P1 — Alto impacto, esfuerzo alto

4. **ACORN traversal** (SPEC-0172): falta la implementacion real del subgrafo
   predicado-agnostico; hoy solo la politica de expansion.
5. **Persistent HNSW** (SPEC-0173): falta persistencia real con checksum y
   recovery, no solo serializacion en memoria.
6. **BlockMax WAND** (SPEC-0177): falta pruning por bloques con max-score.

### P2 — Medio impacto, esfuerzo alto

7. **RQL completo** (SPEC-0161..0165): los operadores existen como primitivas
   pero no estan conectados al parser/executor.
8. **MVCC GC y WAL segmentado** (SPEC-0168/0169): politicas correctas, falta
   cableado al storage.

## 4. Investigacion de frontera (actualizada 2026-10-28)

### 4.1 Tecnicas implementadas con evidencia verificable

- **CRC-32C (Castagnoli)**: polinomio reflejado `0x82F63B78`. Vector de
  verificacion publico RFC 3720 (iSCSI): `crc32c("123456789") == 0xE3069283`
  (comprobado en test). Usado por RocksDB, LevelDB, Kafka, ext4.
- **Bit-packing LSB-first** (FastLanes layout): correccion garantizada por
  round-trip exhaustivo para anchos 0..=32. Base de FOR/DELTA en Parquet,
  FastLanes y ALP.
- **Gorilla XOR** (Facebook, VLDB 2015): compresion de series temporales con
  ventanas de bits reutilizables; exacto para `-0.0`, `NaN` con payload e
  infinitos (comparacion por `to_bits`).
- **ALP** (SIGMOD 2024): decimales con exponente + FOR + bit-packing +
  excepciones; exactitud bit a bit con fallback crudo. Detecta automaticamente
  el mejor exponente y guarda los valores no codificables como excepciones.
- **PGM / learned index** (Kraska et al., SIGMOD 2018; Ferragina & Vinciguerra,
  SIGMOD 2020): segmentacion lineal voraz con cota de error `epsilon`,
  busqueda local completa (no pierde claves), verificado en distribuciones
  adversariales (cuadratica, aritmetica, exponencial).
- **RaBitQ** (SIGMOD 2024, arXiv:2405.12497, Gao & Long): cuantizacion 1-bit
  por dimension con estimador corregido por `sqrt(2/(pi*d))`; recall@10 medido
  >= 0.90 en datos agrupados (20 clusters, d=128).
- **SSI** (Cahill, Rohm & Fekete, SIGMOD 2009): grafo de dependencias rw con
  deteccion de ciclos (DFS con colores) y estructura peligrosa
  (dos aristas rw consecutivas), abort del pivote.
- **CRDT LWW + reloj vectorial** (Shapiro et al., SSS 2011): semirreticulo
  verificado (conmutatividad, asociatividad, idempotencia) sobre tres replicas.

### 4.2 Citas verificadas en esta sesion

- RaBitQ: arXiv:2405.12497 (Gao, Long), SIGMOD 2024 — verificado.
- ACORN: arXiv:2403.04871 (Patel, Kraft, Guestrin, Zaharia) — verificado.
- Learned index: arXiv:1712.01208 (Kraska et al.) — verificado.
- CRC-32C: RFC 3720 (iSCSI) — vector `0xE3069283` comprobado en test.
- ALP/FastLanes/Gorilla: referencias bibliograficas estandar (SIGMOD 2024,
  PVLDB 2023, VLDB 2015); no re-verificadas por inaccesibilidad de PDFs al
  agente, pero las implementaciones se validan con round-trip y propiedades.

## 5. Verificacion de gates (2026-10-28)

- `cargo fmt --all -- --check`: OK.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: OK.
- `cargo test --workspace --all-features`: OK (ruscadb: 205 passed).
- `cargo xtask trace`: 568 AC trazados.
- `cargo xtask contract`: OK.
- `scripts/check_architecture.py`: 0 ciclos, 0 aristas prohibidas.
- `scripts/check_core_unsafe.py`: OK.
- `scripts/check_ci_config.py`: OK.

## 6. Proximo paso recomendado

Cerrar los gaps P0 antes de abrir nuevos specs: integrar los 8 modulos reales en
sus adapters, integrar ML-KEM auditado, y subir el mutation score de los
modulos nuevos por encima de 70%.
