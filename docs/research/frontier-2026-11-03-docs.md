# Frontera de documentación — auditoría y refactor (2026-11-03)

> Tarea explícita: revisar y refactorizar la documentación del repo (simplicidad,
> funcionalidad, fiabilidad, comparación 1:1, técnica y presentación), con ciclo
> `frontier-engineering`. Sin datos privados ni rutas personales.

## 1. Load (auditoría)

- 28 documentos Markdown; roadmap de >1000 líneas y verification de ~400.
- Solo **13 enlaces internos** en total: baja navegabilidad.
- Documentos de chat pre-proyecto (`RuscaDB-generalidades.md`,
  `Recomndaciones de inicio.md`, `Nombre y logo.md`) tratados como documentación
  vigente, sin H1 y con stack recomendado que no coincide con el implementado.
- Duplicación de estado: roadmap, backlog, madurez y gap-register repetían
  conteos y fases; el roadmap tenía "Fin de la hoja de ruta" a mitad del archivo.
- Conteos volátiles obsoletos en README (`22 crates`, `754 tests` frente a 23 y
  la suite actual).
- Sin índice de documentación ni gate de enlaces.

## 2. Research SOTA — fuentes leídas

| Fuente | URL | Aporte |
|---|---|---|
| Diátaxis | https://diataxis.fr/ | cuatro cuadrantes: tutorial, how-to, reference, explanation |
| Google developer docs style | https://developers.google.com/style | claridad, voz, consistencia, jerarquía de referencias |
| Keep a Changelog 1.1.0 | https://keepachangelog.com/en/1.1.0/ | changelog para humanos, no `git log` |
| Write the Docs — Docs as Code | https://www.writethedocs.org/guide/docs-as-code/ | docs en git, revisión y tests automatizados |
| ADR (adr.github.io) | https://adr.github.io/ | registrar decisión + rationale + trade-offs |

`websearch` no devolvió resultados en esta sesión; las URLs se leyeron con
`webfetch`.

## 3. Hipótesis comparadas

Criterios: simplicidad, funcionalidad (encontrar lo que se busca), fiabilidad
(anti-drift), coste de mantenimiento y presentación.

| # | Hipótesis | Simplicidad | Funcionalidad | Fiabilidad | Coste | Veredicto |
|---|---|---|---|---|---|---|
| H1 | Mantener docs actuales + TOC | Alta | Baja | Baja | Bajo | Rechazada |
| H2 | Diátaxis + índice canónico + archivo + gate | Media | Alta | Alta | Medio | **Elegida** |
| H3 | Sitio estático (mdBook/MkDocs) | Media | Alta | Media | Alto | Diferida: no hay necesidad de publicación |
| H4 | Monolito README | Alta | Baja | Baja | Bajo | Rechazada |

## 4. Gap analysis → acciones

| Gap | Acción |
|---|---|
| Sin mapa de documentación | `docs/README.md` con roles Diátaxis y fuentes canónicas |
| Docs pre-proyecto como verdad | mover a `docs/archive/` con banner histórico |
| Estado duplicado y "Fin" prematuro | roadmap con TOC, estado canónico y cierre al final; backlog/madurez enlazan |
| README con conteos obsoletos | quitar conteos volátiles y enlazar a `specs-index`/`verification` |
| Enlaces rotos sin detección | `scripts/check_docs.py` + tests AC-0199 |
| Rutas personales en docs | check `C:\Users\<x>` y `/home/<x>/` en el gate <!-- docs-gate:allow --> |

## 5. Diseño y complejidad

```text
gate: O(F + L)  (F ficheros, L enlaces); memoria O(incidencias)
mantenimiento: estado en 3 fuentes canónicas, resto enlaza (0 duplicación)
```

## 6. Decisión (Δquality > 0 ∧ Δcost < 0)

- Índice + archivo + TOC: coste bajo, mejora directa de navegabilidad.
- Gate determinista: coste ~150 líneas Python + 3 tests; evita regresiones de
  enlaces y drift de índice.
- Sin dependencias nuevas ni sitio estático.

## 7. No aplicado (y por qué)

- **mdBook/MkDocs**: no hay requisito de publicación externa; añadiría cadena de
  build y tema que mantener.
- **Vale/spellcheck**: requiere instalación y diccionario español; candidato
  futuro si se publica la documentación.
- **Chequeo de anclas**: el gate valida ficheros, no fragmentos; las anclas
  manuales son pocas y el coste/beneficio no compensa aún.
- **Traducción EN completa**: el repo mantiene README público en inglés y docs
  internas en español; traducir todo duplicaría mantenimiento.
