# Frontera: DDL en RQL (2026-11-05)

> Cierre del gap "DROP por API solamente". Tras SPEC-0205/0207 existían
> `drop_index`/`drop_table`, pero no eran accesibles desde SQL/RQL.

## 1. Research SOTA — fuentes leídas

| Fuente | URL | Aporte |
|---|---|---|
| PostgreSQL 18 — `DROP INDEX` | https://www.postgresql.org/docs/current/sql-dropindex.html | sintaxis, `IF EXISTS`, `CASCADE`/`RESTRICT` |
| PostgreSQL 18 — `DROP TABLE` | https://www.postgresql.org/docs/current/droptable.html | eliminación de tabla y dependencias |
| SQL:2023 DDL | https://www.iso.org/standard/76583.html | **[UNVERIFIED]** norma de pago, no leída |

## 2. Hipótesis comparadas

| # | Estrategia | Cobertura SQL | Coste | Riesgo |
|---|---|---|---|---|
| H1 | Solo API Rust | baja | bajo | bajo |
| H2 | DROP en RQL reutilizando APIs (elegida) | alta | bajo | bajo |
| H3 | Nombres globales de índice + `IF EXISTS`/`CASCADE` | máxima | medio | alto (dependencias) |

**Decisión:** H2 con `DROP INDEX tabla.columna` (los índices no tienen nombre
global en RuscaDB) y `DROP TABLE tabla`. Sin `CASCADE`; páginas no reclamadas.

## 3. Diseño

```text
DROP INDEX tabla.columna -> drop_index(tabla, columna)
DROP TABLE tabla         -> drop_table(tabla)
```

Complejidad O(|SQL|) de parseo + la del drop (SPEC-0205/0207). Idempotente:
un segundo DROP no falla.

## 4. Gap analysis

| Gap | Acción |
|---|---|
| DDL solo por API | sentencias RQL |
| AST/WASM sin DDL | variantes y `kind=drop_*` |
| Errores de sintaxis | mensajes con INDEX/TABLE/punto |

## 5. No aplicado

- **`IF EXISTS`/`CASCADE`**: dependencias entre tablas/aristas; ola futura.
- **Nombres globales de índice**: el catálogo indexa por `tabla.columna`.
- **Reclamación de páginas**: asignador bump; compactación futura.
