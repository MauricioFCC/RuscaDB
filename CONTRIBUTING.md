# Contribuir a RuscaDB

Gracias por tu interés en RuscaDB. Esta guía describe cómo preparar el entorno,
ejecutar el gate de calidad T1, el flujo *spec-first* y la convención de commits.

El proyecto usa **Rust 2024 / 1.85+** (fijado en `rust-toolchain.toml`) y
Python 3.11+ solo para los *guards* de arquitectura y política de `unsafe`.

---

## Requisitos

- [rustup](https://rustup.rs/) — la toolchain correcta se instala sola al entrar
  en el repo (`rust-toolchain.toml`).
- Componentes `rustfmt` y `clippy`:
  `rustup component add rustfmt clippy`.
- Python 3.11+ para `scripts/check_*.py`.
- (Opcional) `cargo-nextest`, `cargo-mutants`, `cargo-deny`, `cargo-audit`,
  `gitleaks` para reproducir la CI/T3 localmente.

---

## Construir y probar

```bash
# Compilar todo el workspace
cargo build --workspace --all-features

# Ejecutar la suite completa
cargo test --workspace --all-features
```

### Gate T1 (determinista, < 90 s, bloquea merge)

Antes de abrir un PR, el gate T1 debe estar **verde**. Ejecuta exactamente:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features
python scripts/check_architecture.py
python scripts/check_core_unsafe.py
python scripts/check_ci_config.py
cargo xtask trace
```

Qué protege cada comando:

| Comando | Verifica |
|---|---|
| `cargo fmt --all -- --check` | Formato (`rustfmt.toml`). |
| `cargo clippy ... -D warnings` | Lints del workspace (incl. `undocumented_unsafe_blocks`). |
| `cargo test --workspace --all-features` | Tests unitarios, de integración y doctests. |
| `cargo doc --workspace --no-deps --all-features` | Referencia de API sin warnings de rustdoc. |
| `python scripts/check_architecture.py` | Hexagonal: el grafo de dependencias apunta a `ruscadb-core`; sin ciclos. |
| `python scripts/check_core_unsafe.py` | Presupuesto de `unsafe` (`unsafe-allowlist.toml`). |
| `python scripts/check_ci_config.py` | Consistencia de la configuración de CI. |
| `cargo xtask trace` | Trazabilidad *spec → test* (cada AC tiene su test). |

> El gate T1 corre en `.github/workflows/ci.yml`. T3 (mutación, fuzz, miri,
> recuperación de crash) es *nightly* y `alert-only`.

---

## Flujo *spec-first* (Spec-Driven Development)

**Ninguna feature empieza por el código.** Primero se escribe la especificación:

1. Crea `specs/<feature>.md` siguiendo la plantilla existente
   (`specs/*.md`): frontmatter con `id` (`SPEC-XXXX`), `feature`, `status`,
   `owner`, `appetite_days`, `boundaries`, `fr`/`nf`, `acceptance_criteria`,
   `exit_criteria`, `rollback` y `sandbox`.
2. Define los **acceptance criteria (AC)** con su `given/when/then` y el nombre
   del test que los cubre.
3. Escribe el test **antes** del código (rojo → verde → refactor).

### Trazabilidad de AC

Cada AC `AC-XXXX-NN` debe tener su test nombrado:

```
test_ac_XXXX_NN_<descripcion_corta>
```

Por ejemplo, `AC-0011-01` → `test_ac_0011_01_seal_open_roundtrip`.
`cargo xtask trace` falla si un AC no tiene test o un test `test_ac_*` no
referencia un AC válido.

### Reglas de `unsafe`

- Cualquier crate fuera de `ruscadb-ffi` usa `#![forbid(unsafe_code)]`.
- Todo bloque `unsafe` requiere comentario `// SAFETY:` justificando las
  precondiciones, test y ampliación del presupuesto en
  `unsafe-allowlist.toml` si añade bloques.

---

## Convención de commits

Se usa [**Conventional Commits**](https://www.conventionalcommits.org/) en
**español**, con `type(scope): descripción`:

```
feat(query): añade operador KNN a RQL
fix(wal): valida CRC antes de aplicar el frame
docs(security): documenta política de divulgación
chore(ci): cachea toolchain con Swatinem
```

Tipos habituales: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`,
`ci`, `chore`. El `scope` suele ser el crate afectado
(`query`, `wal`, `ffi`, `core`, `crypto`, …).

- Asunto ≤ 72 caracteres, en imperativo.
- Un commit = un cambio coherente; sin secretos ni artefactos generados.
- No se hace `push` forzado ni se reescribe historia ya publicada en `main`.

---

## Pull requests

- Una PR = un objetivo. Enlaza el `specs/<feature>.md` afectado.
- Verifica el gate T1 en local **y** muestra la evidencia (salida de los
  comandos) en la descripción del PR.
- Respeta el flujo *trunk-based*: PRs pequeñas, de vida corta, contra `main`.
- Los cambios de seguridad se reportan de forma privada — ver
  [`SECURITY.md`](SECURITY.md) — **nunca** por issues públicos.

---

## Estilo y documentación

- Código e identificadores en **inglés**; documentación y docstrings en
  **español**.
- Toda función pública lleva docstring con `Args`/`Returns`/`Raises` cuando
  aplique (`missing_docs` está en `warn`, se espera 0).
- Sin `except` silenciosos, sin `unwrap()`/`expect()` en rutas de producción
  (los lints lo marcan como `warn`).
