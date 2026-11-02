# Frontier research — Recolector de artefactos regenerables (2026-11-01)

> Tarea explícita: implementar un recolector de basura **después de un build o
> test**, porque `target/`, `mutants.out/` y los perfiles de cobertura son
> regenerables tras la implementación. Paper trail con URLs leídas; lo no
> verificado se marca `[UNVERIFIED]`.

## 1. Estado del arte (fuentes leídas)

| Fuente | Qué aporta | URL |
|---|---|---|
| Cargo Book — `cargo clean` | Borra todo `target/`; `--dry-run`, `--doc`, `--release`, `--target-dir`. Sin política de antigüedad ni allowlist. | https://doc.rust-lang.org/cargo/commands/cargo-clean.html |
| Cargo Book — Build cache | `target/` contiene artefactos finales e intermedios (incremental, build scripts, doc, package); es regenerable. | https://doc.rust-lang.org/cargo/reference/build-cache.html |
| `cargo-sweep` | Borrado selectivo por tiempo, toolchain o tamaño (`--time`, `--maxsize`, `--dry-run`, `--stamp`). **Unmaintained** y dependencia externa. | https://github.com/holmgr/cargo-sweep |
| `cargo-cache` | Estadísticas y poda de `CARGO_HOME` (`--autoclean`, `--dry-run`, `--remove-if-older-than`), pensado para CI. No cubre `target/` del workspace ni `mutants.out`. | https://github.com/matthiaskrgr/cargo-cache |

## 2. Hipótesis comparadas

Criterios: **A** = exactitud (solo borra regenerable), **L** = latencia,
**E** = escalabilidad, **R** = robustez (symlinks/errores/idempotencia),
**P** = reproducibilidad (reporte determinista).

| # | Hipótesis | A | L | E | R | P | Veredicto |
|---|---|---|---|---|---|---|---|
| H1 | `cargo clean` tras cada test | Alta (todo `target/` es regenerable) | Alta | Alta | Media (sin dry-run ni reporte; borra también lo que otro agente necesita) | Baja | Descartada como única: agresiva y sin observabilidad. |
| H2 | `cargo-sweep --time N` | Media (por antigüedad de artefactos) | Media | Alta | Media (unmaintained; binario externo) | Media | Descartada: dependencia no mantenida y no cubre `mutants.out`. |
| H3 | `cargo-cache --autoclean` | Media (solo `CARGO_HOME`) | Alta | Alta | Alta | Media | Complementaria: útil para `CARGO_HOME`, no para `target/`. |
| H4 | `cargo xtask gc` con allowlist + antigüedad + dry-run + JSON | Alta (allowlist explícita) | `O(N)` stat | `O(N)` con `k` candidatos | Alta (no sigue symlinks, idempotente, omite el exe en uso) | Alta (rutas ordenadas, JSON estable) | **Elegida**. |

## 3. Gap analysis

- `clean-workspace.ps1` histórico: borraba todo sin dry-run, sin antigüedad,
  sin reporte y solo en Windows. No distinguía artefactos de fuente.
- `cargo clean`: no permite conservar un `target/` caliente ni auditar qué se
  borró; en CI borra el `target/` compartido del job.
- `cargo-sweep`/`cargo-cache`: resuelven partes (target por edad, `CARGO_HOME`),
  pero no una política única para `target/` + `mutants.out` + cobertura con
  allowlist y salida JSON.
- **Gap**: falta un recolector in-repo, std-only, cross-platform, seguro por
  defecto (dry-run) y observable, que se pueda invocar tras un build/test y en
  CI (`if: always()`).

## 4. Decisión

Implementar `cargo xtask gc` (SPEC-0192) con:

```text
A = {target, mutants.out, mutants.out.old, fuzz/target, coverage,
     crates/*/target, *.profraw, *.profdata, *.lcov}
borrar(p) ⇔ apply ∧ (all ∨ age(p) ≥ τ)
age(p) = now - max{ mtime(f) : f ∈ p }
```

Invariantes: `p ⊂ root`, `p ≠ root`, no seguir symlinks fuera de `root`, y
omitir el árbol que contiene el ejecutable de `xtask` en uso (Windows).

## 5. Lo no aplicado y por qué

- Poda de `CARGO_HOME`: fuera de alcance (la cubre `cargo-cache`).
- Caché de `sccache`: requiere servicio externo; no hay `sccache` en el repo.
- Borrado de `fuzz/artifacts`/corpus: **no** es regenerable (es entrada de
  fuzzing), por eso queda fuera de la allowlist.
- Poda por tamaño (`--maxsize`): se evaluó, pero la política por antigüedad +
  `--all` cubre el caso de CI sin añadir un umbral arbitrario.
