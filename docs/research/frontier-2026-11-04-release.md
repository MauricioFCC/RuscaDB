# Frontera: disponibilidad de release e instalación (2026-11-04)

> Gap operativo: `releases/latest` devuelve 404 porque aún no hay tag `v*`
> publicado. La instalación por script falla hasta crear el release; mientras
> tanto, `cargo install --git ... --locked` o `--dry-run` validan target/URL.

## 1. Research SOTA — fuentes leídas

| Fuente | URL | Aporte |
|---|---|---|
| Cargo Book — `cargo install` | https://doc.rust-lang.org/cargo/commands/cargo-install.html | `--git`, `--package`, `--locked`, `--dry-run`; reinstalación por versión/fuente |
| GitHub Docs — About releases | https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases | releases basadas en tags; assets y límites (1000 assets, 2 GiB/fichero) |
| GitHub Docs — Managing releases | https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository | crear release desde tag; permisos de escritura |

## 2. Hipótesis comparadas

| # | Estrategia | Disponibilidad | Coste | Robustez |
|---|---|---|---|---|
| H1 | Solo `cargo install --git` | alta (repo público) | alto (compila) | alta |
| H2 | Solo binarios de release | nula hasta tag | bajo al instalar | media si falta asset |
| H3 | Release + fallback documentado a git (elegida) | alta | bajo | alta |
| H4 | Publicar en crates.io/PyPI/npm | alta | medio (identidad) | media |

**Decisión:** H3. El release es la vía primaria; si `releases/latest` no existe,
el script debe fallar con mensaje accionable y el README documenta el fallback
reproducible `cargo install --git ... --package ruscadb-cli --locked`.
`cargo install ... --dry-run` valida URL/target sin instalar.

## 3. Diseño

```text
matriz target -> asset (una sola fuente: scripts/check_release_assets.py)
install.sh/ps1: releases/latest -> tag -> download/<tag>/ruscadb-<target>.tar.gz
404/latest ausente: exit 1 + fallback git (WHAT/WHY/WHERE)
release.yml: tags v* -> build matriz -> release con assets + sha256
```

Complejidad: O(targets) offline; sin red en el gate.

## 4. Gap analysis

| Gap | Acción |
|---|---|
| `releases/latest` 404 | spec de release/tag + fallback documentado |
| Scripts con error genérico | mensaje accionable con fallback git |
| Matriz target duplicada | validador offline compartido |
| Sin tag v* | checklist de release y verificación de versión |

## 5. No aplicado (aún)

- **Publicar el tag/release**: requiere push y permisos; fuera de esta sesión.
- **crates.io/PyPI/npm**: reserva de identidad y credenciales (SPEC-0088/0196
  fuera de alcance).
- **Firma de artefactos (cosign/Sigstore)**: ola de supply chain posterior.
