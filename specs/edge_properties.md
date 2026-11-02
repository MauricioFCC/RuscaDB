---
id: SPEC-0193
feature: edge_properties
status: implemented
maturity: real
owner: query-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-query, ruscadb-txn]
  files:
    - crates/ruscadb-query/src/ast.rs
    - crates/ruscadb-query/src/lexer.rs
    - crates/ruscadb-query/src/parser.rs
    - crates/ruscadb/src/edge_meta.rs
    - crates/ruscadb/src/executor/graph_paths.rs
    - crates/ruscadb/src/table_api.rs
    - crates/ruscadb/src/catalog.rs
    - crates/ruscadb-txn/src/mvcc.rs
  out_of_scope: [historial bi-temporal de aristas, RDF-star, LIVE SELECT]
fr:
  - { id: FR-0193-01, desc: "RELATE a->label->b [SET {json}] [AT unix] crea la arista con payload y ts_unix" }
  - { id: FR-0193-02, desc: "idempotencia por (from,label,to) con ON CONFLICT DO NOTHING | DO UPDATE SET payload" }
  - { id: FR-0193-03, desc: "persistencia transaccional en EdgeMetaStore (pagina REDG1 + frames WAL)" }
  - { id: FR-0193-04, desc: "lectura ->label{payload, ts_unix}->tabla devuelve los metadatos de la arista" }
nf:
  - { id: NF-0193-01, desc: "no cambia el formato postcard de Edge {label,node}" }
  - { id: NF-0193-02, desc: "RELATE O(log E + P); lectura O(V + E + k log E)" }
  - { id: NF-0193-03, desc: "payload JSON objeto <= 64 KiB, sin claves reservadas; errores accionables" }
  - { id: NF-0193-04, desc: "sin dependencias nuevas; MVCC/WAL existentes; 0 secretos" }
acceptance_criteria:
  - id: AC-0193-01
    given: dos registros vivos y RELATE con SET {json}
    when: se ejecuta dos veces con el mismo payload
    then: la primera crea la arista (affected=1) y la segunda es no-op (affected=0) preservando payload y ts_unix
    test: test_ac_0193_01_relate_payload_idempotent
  - id: AC-0193-02
    given: una arista existente con payload/ts_unix
    when: se repite RELATE con payload distinto, con ON CONFLICT DO NOTHING y con DO UPDATE SET payload
    then: el default devuelve EDGE_CONFLICT, DO NOTHING no cambia nada y DO UPDATE cambia solo el payload (ts_unix inmutable)
    test: test_ac_0193_02_relate_conflict_and_update
  - id: AC-0193-03
    given: un payload no-objeto, con clave reservada o mayor de 64 KiB
    when: se ejecuta RELATE
    then: se rechaza con error accionable y no se escribe la arista
    test: test_ac_0193_03_payload_validation
  - id: AC-0193-04
    given: una arista con payload/ts_unix y otra legacy sin metadatos
    when: se proyecta ->label{payload, ts_unix}->tabla
    then: se devuelve un Text JSON con los metadatos (o null para la legacy)
    test: test_ac_0193_04_edge_read_projection
  - id: AC-0193-05
    given: una arista creada y la base cerrada
    when: se reabre y se reproduce el WAL
    then: el EdgeMetaStore conserva payload y ts_unix; el replay es idempotente
    test: test_ac_0193_05_persistence_recovery
  - id: AC-0193-06
    given: una transaccion con RELATE
    when: se hace rollback
    then: no queda arista ni metadatos (atomicidad MVCC)
    test: test_ac_0193_06_txn_rollback
  - id: AC-0193-07
    given: un Edge {label,node} serializado con postcard
    when: se compara con el golden actual
    then: los bytes no cambian (compatibilidad de formato)
    test: test_ac_0193_07_edge_format_unchanged
exit_criteria:
  - cargo test -p ruscadb --all-features
  - cargo test -p ruscadb-graph --all-features
  - cargo test -p ruscadb-query --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - eliminar la pagina REDG1 y sus frames WAL; Record.edges y el formato postcard no se tocan, por lo que las aristas existentes siguen siendo legibles
sandbox:
  - cargo test -p ruscadb --test edge_properties -- --ignored
  - cargo xtask gc --dry-run --json
---

# SPEC-0193 — Aristas con payload JSON y `ts_unix`

## Contexto

`RELATE` ya crea aristas idempotentes en `Record.edges { label, node }`, pero
no guarda **payload** ni **timestamp**. Los casos de uso (relaciones con
metadatos, auditoría temporal, sincronización) necesitan un payload JSON y un
`ts_unix` consultable sin romper el formato postcard de `Edge` que ya está en
producción. Esta spec define la mezcla: payload JSON + `ts_unix` automático o
explícito + política de conflicto, con persistencia transaccional separada.

## Superficie

### Gramática

```sql
RELATE person:a -> knows -> person:b
  [ SET { "since": 2020, "source": "import" } ]
  [ AT 1700000000 ]
  [ ON CONFLICT DO NOTHING | DO UPDATE SET payload = { ... } ]
```

- `SET` es un objeto JSON (no array, no escalar). Por defecto `{}`.
- `AT` es un entero de segundos Unix UTC (event time). Por defecto `now()`.
- `ON CONFLICT` solo aplica si la arista `(from,label,to)` ya existe.
- `ts_unix` es **inmutable**: `DO UPDATE` solo puede cambiar `payload`.

### Persistencia

Tabla interna `__ruscadb_edge_meta` (no visible en la API pública):

```text
edge_id = SHA-256(from_table, from_id, label, to_table, to_id)
columnas = edge_id TEXT (índice), from_table, from_id, label,
           to_table, to_id, payload TEXT (JSON), ts_unix INT
```

- Clave sintética única + índice B+tree ⇒ lookup/insert `O(log E)`.
- Se reutilizan **heap, índice, MVCC, WAL y recovery** existentes; no hay
  formato `REDG1` ni frame WAL nuevo.
- `Record.edges` **no** cambia: sigue siendo `{ label, node }`.
- En una transacción explícita, el catálogo se escribe con
  `Catalog::save_pages` (sin commit) y el commit lo decide el llamador.
- Rollback: el `TxnManager` marca la transacción como abortada y
  `Snapshot::is_visible` excluye sus versiones (SPEC-0193 AC-06).

### Lectura

`->label{payload, ts_unix}->tabla` devuelve un `Text` con JSON canónico:

```json
{"payload":{"since":2020},"ts_unix":1700000000}
```

Una arista legacy (sin `EdgeMeta`) devuelve
`{"payload":null,"ts_unix":null}`. El path sin llaves sigue devolviendo el
`RecordId` como hasta ahora.

## Política de idempotencia

| Estado | Entrada | Resultado |
|---|---|---|
| no existe | cualquier `SET`/`AT` válido | crea; `affected=1` |
| existe, mismos valores | sin `SET`/`AT` o idénticos | no-op; `affected=0` |
| existe, valores distintos | default | `EDGE_CONFLICT` accionable |
| existe, valores distintos | `DO NOTHING` | no-op; `affected=0` |
| existe | `DO UPDATE SET payload` | actualiza payload; `ts_unix` intacto; `affected=1` |
| existe | `DO UPDATE` + `AT` | error: `ts_unix` inmutable |

## Complejidad

```text
validar payload:       O(P)          P = bytes JSON (<= 64 KiB)
lookup/insert:         O(log E)      E = aristas (índice edge_id)
RELATE total:          O(log E + P)
recovery:              O(E)          (heap + WAL existentes)
path con metadatos:    O(V + E + k log E)
```

Antes: `RELATE` era `O(log n)` pero sin metadatos; la consulta de payload
requería una tabla externa o no existía. Después: `O(log E + P)` con
atomicidad MVCC/WAL y sin tocar el formato de `Edge`.

## Migración

- Bases existentes: no hay `__ruscadb_edge_meta`; las aristas legacy se leen
  con `payload=null`, `ts_unix=null`. No se reescribe `Record.edges`.
- `RELATE` sobre una arista legacy crea su fila de metadatos (el `Edge` ya
  existe); no duplica la arista.
- Rollback: borrar la tabla `__ruscadb_edge_meta`; el grafo sigue funcionando
  con `Record.edges` y el formato postcard intacto.

## Riesgos

- **Crecimiento del payload**: límite duro de 64 KiB y error accionable.
- **Reloj**: `AT` explícito permite tests deterministas; sin `AT` se usa el
  reloj del sistema (event time = processing time).
- **Concurrencia**: dos `RELATE` simultáneos sobre la misma clave; el WAL/MVCC
  decide el ganador y el perdedor aplica la política de conflicto.
- **Auditoría**: no hay historial bi-temporal; solo el estado actual.

## Estado de implementación (2026-11-02)

- `crates/ruscadb/tests/edge_properties.rs`: **7 tests activos** (sin
  `#[ignore]`) para AC-0193-01..07.
- Gramática: `SET {json}`, `AT <unix>`, `ON CONFLICT DO NOTHING` y
  `ON CONFLICT DO UPDATE SET payload = {json}`.
- Lectura: `->label{payload, ts_unix}->tabla` devuelve `Text` JSON.
- Persistencia: tabla interna + índice `edge_id`; `Catalog::save_pages` evita
  commits prematuros dentro de una transacción explícita.
- MVCC: `TxnManager` registra transacciones abortadas y `Snapshot::is_visible`
  las excluye (rollback observable).
- Golden postcard de `Edge`: bytes congelados en AC-0193-07.

## Referencias

- Neo4j, *MERGE*: https://neo4j.com/docs/cypher-manual/current/clauses/merge/
- Apache AGE, *CREATE*: https://age.apache.org/age-manual/master/clauses/create.html
- Apache Flink, *Timely Stream Processing*: https://nightlies.apache.org/flink/flink-docs-stable/docs/concepts/time/
- JSON Schema 2020-12: https://json-schema.org/specification
- IETF Idempotency-Key: https://datatracker.ietf.org/doc/draft-ietf-httpapi-idempotency-key-header/
- SurrealDB (research local): `docs/research/surrealdb-2026-11-02.md`
