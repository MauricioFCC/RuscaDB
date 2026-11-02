# Frontier research — Aristas con payload y `ts_unix` (2026-11-02)

> Tarea explícita: extender `RELATE` para que la arista sea **idempotente** y
> lleve **payload JSON** + **`ts_unix`** (mezcla de las alternativas
> planteadas). Este documento es el paper trail previo a la spec; no hay código
> todavía.

## 1. Estado del arte (fuentes leídas)

| Fuente | Aporte | URL |
|---|---|---|
| Neo4j Cypher `MERGE` | `MERGE` es idempotente: si el patrón exacto existe no crea nada; `ON CREATE`/`ON MATCH` separan el efecto. Las propiedades de relación son de primera clase. | https://neo4j.com/docs/cypher-manual/current/clauses/merge/ |
| Apache AGE `CREATE` | Aristas con etiqueta y propiedades (`CREATE (a)-[e:knows {since: 2020}]->(b)`); el modelo property-graph guarda el payload en la arista. | https://age.apache.org/age-manual/master/clauses/create.html |
| Apache Flink — *Timely Stream Processing* | Distingue **event time** (cuándo ocurrió) de **processing time** (cuándo se ingirió). Un `ts_unix` de arista debe declarar cuál es. | https://nightlies.apache.org/flink/flink-docs-stable/docs/concepts/time/ |
| JSON Schema 2020-12 | Validación estructural de payloads JSON; vocabulario estable para objetos y límites. | https://json-schema.org/specification |
| IETF Idempotency-Key (draft-08) | Semántica de reintentos idempotentes: misma clave ⇒ mismo efecto; conflicto detectable si el cuerpo difiere. | https://datatracker.ietf.org/doc/draft-ietf-httpapi-idempotency-key-header/ |
| SurrealDB (research local) | `RELATE` + `content` en la arista; paths `->label->`; el grafo se modela como tabla de aristas. | `docs/research/surrealdb-2026-11-02.md` |

> SurrealDB online no respondió en esta sesión; se usa el research local ya
> verificado (`[UNVERIFIED]` para cualquier detalle no cubierto allí).

## 2. Hipótesis comparadas

Criterios: **A** exactitud (payload/ts fieles), **L** latencia, **E**
escalabilidad, **R** robustez (transacción/recovery/idempotencia), **P**
reproducibilidad (tests deterministas).

| # | Hipótesis | A | L | E | R | P | Veredicto |
|---|---|---|---|---|---|---|---|
| H1 | Extender `Edge { label, node }` con `payload` + `ts_unix` | Alta | Alta | Alta | Media (rompe el formato postcard y obliga a migrar todas las aristas) | Media | Descartada: cambio de formato incompatible. |
| H2 | Store separado `EdgeMetaStore` keyed by `(from,label,to)`, B-tree + WAL + página `REDG1` | Alta | `O(log E)` | Alta | Alta (MVCC/WAL, recovery idempotente) | Alta (`AT` explícito) | **Elegida**. |
| H3 | Sidecar JSONL `<data>.edges.jsonl` | Alta | `O(1)` append | Media (crece sin índice) | Baja (no transaccional; recovery manual) | Media | Descartada: rompe la atomicidad con el WAL. |
| H4 | Tabla oculta `_edges` sobre el heap existente | Alta | `O(log E)` | Alta | Media (no hay índice único compuesto; `ON CONFLICT` no lo cubre) | Media | Descartada por falta de unicidad compuesta. |

## 3. Gap analysis

- `Record.edges` solo guarda `{ label, node }`; no hay payload ni timestamp.
- `RELATE` actual es idempotente por `(label,node)` pero **no** distingue
  payloads distintos ni detecta conflictos.
- No existe una política de conflicto (`DO NOTHING`/`DO UPDATE`) ni un
  `ts_unix` inmutable.
- El formato postcard de `Edge` está en producción; cambiarlo obliga a
  migración y a reescribir todos los registros.
- **Gap**: metadatos de arista transaccionales, idempotentes y consultables sin
  tocar el formato de `Edge`.

## 4. Decisión de diseño

1. **Store separado** `EdgeMetaStore` (H2): `BTreeMap<EdgeKey, EdgeMeta>` con
   `EdgeKey = (from, label, to)` y `EdgeMeta = { payload: Value, ts_unix: i64 }`.
2. **`ts_unix` = event time** en segundos Unix UTC; por defecto `now` y
   opcional `AT <unix>` para backfill. Inmutable tras crear la arista.
3. **Payload** = objeto JSON (RFC 8259) de hasta 64 KiB, sin claves reservadas
   (`from`, `to`, `label`, `ts_unix`). Validación estructural, no schema libre.
4. **Idempotencia** por `(from,label,to)`:
   - sin `SET`/`AT` o con los mismos valores ⇒ `affected = 0`, preserva todo;
   - con valores distintos ⇒ `EDGE_CONFLICT` accionable;
   - `ON CONFLICT DO NOTHING` fuerza no-op; `ON CONFLICT DO UPDATE SET payload`
     actualiza solo el payload (nunca `ts_unix`).
5. **Lectura** `->label{payload, ts_unix}->tabla` devuelve un `Text` JSON con
   los metadatos (o `null` para aristas legacy).
6. **Sin tocar `Edge`**: test golden del postcard actual; las aristas siguen
   siendo `{label,node}` en el registro.

## 5. Complejidad

```text
RELATE (crear/actualizar): O(log E + P)   E = aristas, P = bytes del payload
lookup por clave:         O(log E)
recovery:                 O(E)            (replay de frames REDG1)
lectura de path:          O(V + E + k·log E)  k = vecinos proyectados
```

## 6. Lo no aplicado y por qué

- Historial bi-temporal de aristas (múltiples versiones por `(from,label,to)`):
  fuera de alcance; esta spec guarda el estado actual.
- RDF-star / anotaciones de triple: el modelo es property-graph, no RDF.
- `LIVE SELECT` sobre cambios de arista: sin servidor.
- Payload tipado con JSON Schema completo: se valida estructura y límites; un
  schema por etiqueta queda como evolución.
