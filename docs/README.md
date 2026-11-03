# Documentación de RuscaDB — mapa e índice

> **Regla de oro:** cada dato de estado tiene **una** fuente canónica. El resto
> de documentos enlaza, no copia. Los documentos históricos viven en
> [`archive/`](archive/) y no son fuente de verdad.

## 1. Empieza aquí según tu necesidad (Diátaxis)

| Necesidad | Documento |
|---|---|
| Aprender y ejecutar el primer ejemplo | [`../README.md`](../README.md) · [`api_guide.md`](api_guide.md) |
| Instalar CLI, bindings o servidor MCP | [`instalacion.md`](instalacion.md) |
| Operar una base (backup, locks, claves) | [`operacion.md`](operacion.md) |
| Consultar formatos físicos en disco | [`formats.md`](formats.md) |
| Ver capacidades y límites del producto | [`RuscaDB-funcionalidades.md`](RuscaDB-funcionalidades.md) · [`MVP.md`](MVP.md) |
| Entender el diseño y el historial de olas | [`RuscaDB-roadmap.md`](RuscaDB-roadmap.md) |
| Revisar madurez por área y gaps | [`RuscaDB-maturity.md`](RuscaDB-maturity.md) · [`RuscaDB-gap-register.md`](RuscaDB-gap-register.md) |
| Contribuir: gates, tests y convenciones | [`../AGENTS.md`](../AGENTS.md) · [`verification.md`](verification.md) |
| Ver el estado de cada spec y sus ACs | [`specs-index.md`](specs-index.md) · [`../specs/`](../specs/) |
| Identidad, naming y dominio | [`RuscaDB-naming.md`](RuscaDB-naming.md) |
| Mutantes supervivientes conocidos | [`mutation-survivors.md`](mutation-survivors.md) |

## 2. Fuentes canónicas (no duplicar)

| Dato | Fuente | Generación |
|---|---|---|
| Estado y madurez por spec | [`specs-index.md`](specs-index.md) | `cargo xtask gen-specs-index` |
| Trazabilidad AC → test | `specs/*.md` + `cargo xtask trace` | manual |
| Madurez por área (L0–L4) | [`RuscaDB-maturity.md`](RuscaDB-maturity.md) | manual, con evidencia |
| Gaps/stubs por spec | [`RuscaDB-gap-register.md`](RuscaDB-gap-register.md) | manual |
| Priorización del ciclo | [`RuscaDB-backlog.md`](RuscaDB-backlog.md) | manual |
| Gates y evidencia de cierre | [`verification.md`](verification.md) | manual |
| Cambios notables | [`../CHANGELOG.md`](../CHANGELOG.md) | Keep a Changelog |
| Diseño e historial de olas | [`RuscaDB-roadmap.md`](RuscaDB-roadmap.md) | manual |

El roadmap **no** repite conteos: enlaza a `specs-index`, madurez y gap-register.
Si un número aparece en dos sitios, es un bug de documentación.

## 3. Investigación y decisiones

- [`research/`](research/): paper trail por ciclo (`frontier-<fecha>-<tema>.md`),
  con URLs leídas o `[UNVERIFIED]`. Últimos: `frontier-2026-11-03-docs.md` y
  `frontier-2026-11-03-hash-join.md`.
- [`adr/`](adr/): decisiones arquitectónicas (ADR; local-only, no se publican).
- [`archive/`](archive/): documentos históricos (pre-proyecto, chats de diseño,
  investigación superada). No se actualizan.

## 4. Convenciones de escritura

- Español para documentación; inglés para código, identificadores y commits
  convencionales (`type(scope): descripción`).
- Enlaces **relativos** entre documentos; sin rutas absolutas de máquina.
- Encabezados con una sola `#` de título; secciones `##`/`###`.
- Tablas para comparativas; bloques `text` para salidas de gates.
- Fechas ISO (`AAAA-MM-DD`); no usar “hoy”/“ayer”.
- No pegar conteos volátiles (tests, ACs) fuera de las fuentes canónicas.
- Marcar lo no verificado como `[UNVERIFIED]`; no presentar supuestos como hechos.
- Si un documento necesita citar una ruta de ejemplo, añade
  `<!-- docs-gate:allow -->` en esa línea (excepción explícita y auditable).

## 5. Validación (docs-as-code)

```bash
python scripts/check_docs.py            # enlaces, rutas personales, fences, índice
python scripts/check_docs.py --json     # salida para CI
```

El gate corre en el job `architecture` de CI y en el Gate T1 de `AGENTS.md`.
Comprueba: enlaces relativos y anclas existentes, ausencia de rutas absolutas de
usuario, bloques de código balanceados y que todo documento de primer nivel esté
enlazado desde este índice.

## 6. Cómo añadir documentación

1. Elige el cuadrante Diátaxis y evita duplicar una fuente canónica.
2. Añade el documento a la tabla de §1 (el gate falla si no está enlazado).
3. Actualiza `CHANGELOG.md` si el cambio es notable.
4. Ejecuta `python scripts/check_docs.py` y los gates T1 de `AGENTS.md`.
