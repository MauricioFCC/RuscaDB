# RuscaDB — Reserva de identidad (naming, dominios, logo)

> Runbook de reserva del nombre e identidad de RuscaDB. Estado: **artefactos
> listos, publicación pendiente de credenciales** (ver §4).

---

## 1. Disponibilidad verificada (2026-10-06)

| Registro | Nombre | Estado | Evidencia |
|---|---|---|---|
| crates.io | `ruscadb` | **Disponible** | API: `crate 'ruscadb' does not exist` |
| crates.io | `rusca-db` | Disponible | API: `crate 'rusca-db' does not exist` |
| PyPI | `ruscadb` | **Disponible** | `GET /pypi/ruscadb/json` → HTTP 404 |
| npm | `ruscadb` | **Disponible** | `GET /ruscadb` → HTTP 404 |
| Dominio | `ruscadb.dev` | Probablemente disponible | RDAP → HTTP 404 |
| Dominio | `ruscadb.io` | Probablemente disponible | RDAP → HTTP 404 |
| Dominio | `ruscadb.com` | Probablemente disponible | RDAP → HTTP 404 |
| Dominio | `ruscadb.org` | Probablemente disponible | RDAP → HTTP 404 |
| Dominio | `rusca.rs` | Probablemente disponible | RDAP → HTTP 404 |

> RDAP 404 indica que no hay registro en el registro autoritativo; confirmar en
> el registrar antes de comprar.

---

## 2. Artefactos preparados

| Registro | Archivo | Notas |
|---|---|---|
| crates.io | `crates/ruscadb/Cargo.toml` | Crate fachada; metadatos de publicación listos |
| PyPI | `packaging/pypi/pyproject.toml` + `packaging/pypi/src/ruscadb/__init__.py` | Placeholder hatchling |
| npm | `packaging/npm/package.json` + `index.js` + `index.d.ts` | Placeholder napi-rs |
| Logo | `assets/logo.svg` (completo), `assets/logo-mark.svg` (icono) | Cucaracha geométrica en chip hexagonal con nodos de grafo |

---

## 3. Comandos de publicación (cuando haya credenciales)

### 3.1 crates.io

```bash
# Autenticación (una vez): obtener token en https://crates.io/settings/tokens
cargo login <CRATES_IO_TOKEN>

# Dry-run y publicación del crate fachada
cargo publish -p ruscadb --dry-run
cargo publish -p ruscadb
```

### 3.2 PyPI

```bash
# Build y publicación con token (nunca en texto plano en el repo)
python -m build packaging/pypi
python -m twine upload --username __token__ --password "$TWINE_PASSWORD" packaging/pypi/dist/*
```

### 3.3 npm

```bash
npm login                      # o `npm adduser`
cd packaging/npm
npm publish --access public
```

---

## 4. Bloqueo actual (requiere acción del usuario)

La máquina **no tiene credenciales** configuradas:

- `~/.cargo/credentials.toml`: ausente.
- `~/.pypirc` y `TWINE_PASSWORD`: ausentes.
- `npm whoami`: `ENEEDAUTH` (sin login).

Por seguridad (SEG: 0 secrets) el agente **no crea cuentas ni introduce tokens**.
Para completar la reserva, el usuario debe aportar:

1. **crates.io**: token de API (`cargo login`).
2. **PyPI**: API token en `TWINE_PASSWORD`.
3. **npm**: sesión con `npm login` (o token `NPM_TOKEN`).
4. **Dominio**: cuenta de registrar (Cloudflare/Porkbun/Namecheap) — requiere pago.
5. **GitHub org**: `gh auth login` para crear la organización `ruscadb`.

---

## 5. Política anti-name-squatting

crates.io y PyPI desaconsejan publicar paquetes vacíos solo para reservar el
nombre. Alternativa recomendada (cumple las políticas y da valor real):

1. Publicar `crates/ruscadb` cuando el crate fachada tenga contenido funcional
   mínimo (F1 lo aporta: `Record` + WAL reexportados).
2. Publicar el binding Python/npm al llegar a la Fase F5 (placeholder → real).
3. Mientras tanto, asegurar la identidad con **dominio + organización GitHub +
   logo**, que no dependen de políticas de registries.

---

## 6. Identidad visual

- Logo completo: `assets/logo.svg` (fondo oscuro redondeado).
- Marca/icono: `assets/logo-mark.svg` (sin fondo, para favicon/avatar).
- Paleta: `#22d3ee` (cyan) → `#6366f1` (indigo), fondo `#0b1020`.
- Concepto: **cucaracha geométrica** (Rusca = roach, resistente/ubicua) dentro
  de un **chip hexagonal** con **nodos de grafo** — multi-modelo + embebida.
