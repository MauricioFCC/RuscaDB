# Aislamiento de compilación (anti-contaminación)

> Protocolo obligatorio del proyecto RuscaDB. Fuente: incidentes de tests
> falsos-negativos por `target/` compartido + `cargo-mutants --in-place`.

## Problema

Varios agentes/subagentes compilan en el mismo workspace. Si dos compilaciones
(o una corrida de mutación) coinciden, el `target/` conserva **binarios
rancios** y los tests fallan/pasan por el artefacto equivocado, no por el
código. Síntoma típico: un test "imposible" que falla, o un `cargo test` que no
refleja la fuente actual.

## Regla

1. **Nunca compartas `CARGO_TARGET_DIR` entre agentes.**
   - El plugin `.opencode/plugin/cargo-isolation.js` fija automáticamente un
     `CARGO_TARGET_DIR` **único por sesión** (`%TEMP%/ruscadb-targets/<sessionID>`)
     vía el hook `shell.env`. Los subagentes tienen sesiones distintas ⇒
     directorios distintos.
   - Para ejecución manual/CI: `pwsh scripts/isolated-cargo.ps1 <args>`.
   - Desactivar (no recomendado): `RUSCADB_SHARED_TARGET=1`.

2. **Mutación siempre en limpio.**
   - Prefiere `cargo mutants` **sin** `--in-place` (copia a temp).
   - Si usas `--in-place`, ejecuta `cargo clean -p <crate>` **antes y después**.
   - Nunca dos corridas de mutación en paralelo sobre el mismo target.

3. **Antes de verificar, limpia el crate afectado.**
   - `cargo clean -p <crate>` (o `pwsh scripts/clean-workspace.ps1` para todo).

4. **No uses `cargo fmt --all` durante trabajo paralelo**: formatea solo tu
   crate (`cargo fmt -p <crate>`). `fmt --all` reescribe archivos de otros
   agentes y crea conflictos espurios.

5. **Un crate por agente** cuando corran en paralelo: dos agentes no deben
   editar el mismo crate a la vez (comparten unidad de compilación y target de
   test). Particiona por crate, no por archivo.

## Diagnóstico rápido

| Síntoma | Causa probable | Acción |
|---|---|---|
| Test falla con fuente correcta | binario rancio en `target/` | `cargo clean -p <crate>` |
| `cargo test` no refleja cambios | target compartido | usar dir aislado |
| Mutante "MISSED" imposible | caché de mutación | target virgen + sin `--in-place` |
| Formato cambia archivos ajenos | `cargo fmt --all` | `cargo fmt -p <crate>` |

## Comandos

```powershell
pwsh scripts/clean-workspace.ps1                 # limpia target + mutants.out
pwsh scripts/isolated-cargo.ps1 test -p ruscadb  # cargo en target único
```
