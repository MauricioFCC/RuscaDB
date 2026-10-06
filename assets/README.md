# RuscaDB - Guia de marca

Identidad visual de **RuscaDB**, la base de datos embebida, multi-modelo y
multimodal con core en Rust. Esta guia define el uso correcto de los activos
contenidos en `assets/`.

## Concepto

**Rusca** = cucaracha (resiliente, ubicua, rapida) + **DB** (chip + nodos de
grafo). La marca representa una **cucaracha geometrica dentro de un chip
hexagonal con seis nodos de grafo**: la silueta del insecto comunica
resistencia y adaptabilidad, mientras el chip y la red de nodos comunican
infraestructura embebida y datos multimodelo.

- El **hexagono** es el chip embebido; sus aristas conectan los seis nodos.
- Los **seis nodos** evocan los modelos soportados y el grafo de datos.
- El **pronoto** (escudo cefalico) y los **elitros divididos** por una linea
  central aportan la silueta geometrica reconocible del insecto.

## Archivos

| Archivo | Uso | ViewBox |
| --- | --- | --- |
| `logo-mark.svg` | Icono principal sin fondo (transparente) | 128 x 128 |
| `logo.svg` | Lockup horizontal: marca + wordmark | 440 x 128 |
| `logo-mark-dark.svg` | Icono sobre tile oscuro (avatar / app icon) | 128 x 128 |
| `favicon.svg` | Version simplificada y bold para <= 32 px | 32 x 32 |
| `og-image.svg` | Tarjeta social Open Graph / Twitter | 1200 x 630 |
| `brand-tokens.json` | Design tokens (primitive / semantic / component) | - |

## Paleta

| Rol | HEX | Uso |
| --- | --- | --- |
| Cyan | `#22d3ee` | Acento primario, nodos, patas y antenas |
| Indigo | `#6366f1` | Acento secundario, extremo del gradiente |
| Ink (fondo) | `#0b1020` | Fondo oscuro, tile y tinta sobre claro |
| Ink raised | `#111834` | Superficies elevadas |
| Text primary | `#f8fafc` | Texto principal sobre oscuro |
| Text muted | `#94a3b8` | Texto secundario sobre oscuro |

El gradiente de marca es **`#22d3ee` -> `#6366f1`** (135 grados). Se declara
como `linearGradient` en espacio de usuario para mantener continuidad entre las
piezas del mark.

### Contraste

- Texto `#f8fafc` sobre `#0b1020`: aprox. 19:1.
- Texto muted `#94a3b8` sobre `#0b1020`: aprox. 6.2:1.
- Cyan `#22d3ee` sobre `#0b1020`: aprox. 10.5:1.
- Elementos graficos del gradiente sobre `#0b1020`: >= 4.2:1 (supera el 3:1
  exigido a contenido no textual). Para texto, usa siempre `currentColor` o
  `#f8fafc`.

## Tipografia

- **UI / wordmark**: Inter (fallback `ui-sans-serif`, `system-ui`), weight 700.
- **Acentos tecnicos**: JetBrains Mono (fallback `ui-monospace`).
- Escala base 16 px; el wordmark del lockup usa 56 px y la tarjeta social 118 px.

## Uso correcto e incorrecto

**Correcto**

- Usa `logo-mark.svg` sobre fondos claros u oscuros; el mark a color mantiene
  contraste por si mismo.
- Usa `logo.svg` heredando el color del contexto: el wordmark emplea
  `currentColor`, por lo que se adapta a tema claro u oscuro.
- Para entornos de un solo color (impresion, grabados, bordados, iconos de
  sistema), activa la variante monocroma del mark: agrega la clase
  `rusca-mono` al elemento `<svg>` o incrusta el SVG y deja que
  `currentColor` herede el color del texto.
- Respeta el area de resguardo y los tamanos minimos.
- Manten la proporcion original al escalar.

**Incorrecto**

- No deformar, rotar ni inclinar la marca.
- No recolorear el gradiente con colores fuera de la paleta.
- No anadir sombras, biseles ni efectos ajenos a `brand-tokens.json`.
- No colocar el mark directamente sobre fotografias o fondos de bajo contraste
  sin el tile oscuro (`logo-mark-dark.svg`).
- No reconstruir el wordmark con otra tipografia ni separar la marca del texto
  a distancias arbitrarias.
- No usar el favicon por encima de 32 px: para tamanos mayores usa el mark.

## Area de resguardo (clearspace)

El resguardo minimo es **1/4 del ancho de la marca** (32 px sobre el icono de
128 px). Ningun texto, borde o elemento grafico debe invadir ese margen.

## Tamanos minimos

| Activo | Tamano minimo | Tamano recomendado |
| --- | --- | --- |
| Icono (`logo-mark.svg`) | 16 px | 32 px o mas |
| Favicon (`favicon.svg`) | 16 px | 32 px |
| Lockup (`logo.svg`) | 96 px de ancho | 220 px o mas |
| Tarjeta social | 600 px de ancho | 1200 px |

A 16 px usa siempre `favicon.svg` (silueta sin patas); el mark completo
conserva detalle suficiente a partir de 24-32 px.

## Design tokens

`brand-tokens.json` organiza la identidad en tres capas:

1. **primitive**: valores crudos (colores, tipografia, espaciado 4 px, radios,
   sombras, grosores de borde).
2. **semantic**: roles de uso (`brand`, `surface`, `text`, `border`, `state`,
   `typography`, `focusRing`) que referencian los primitivos con `{...}`.
3. **component**: consumidores finales (`button`, `card`, `code`, `badge`).

La escala de espaciado es multiplo de **4 px** (`space.1 = 4px`).

## Licencia

Los activos de marca de RuscaDB se distribuyen bajo **Apache-2.0**. Consulta el
archivo `LICENSE` del repositorio para el texto completo.
