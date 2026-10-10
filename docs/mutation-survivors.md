# Registro de mutantes supervivientes (proceso MutGen)

Proceso adversarial (roadmap §2.2, curso AI Evals W1 *flywheel*): cada corrida
acotada de `cargo mutants` clasifica a los supervivientes en una de estas
clases; toda clase salvo `EQUIVALENTE` genera tests que matan al mutante y se
re-mide hasta MS ≥ 70 % (merge) con objetivo ≥ 85 % (nightly):

- `EQUIVALENTE`: cambio sin efecto observable en ninguna entrada (se documenta
  y no se persigue; cazarlo exigiría tests frágiles o imposibles).
- `FALTA-TEST`: comportamiento distinto sin test que lo cubra → se añade test.
- `ORÁCULO-DÉBIL`: el test existe pero el assert no distingue → se endurece el
  assert (conteos exactos, no solo `contains`).
- `FALTA-BRANCH`: falta una entrada que active la rama (degenerados, bordes).

Comando canónico (con aislamiento anti-contaminación):

```bash
cargo clean -p <crate>
cargo mutants -p <crate> --in-place -f <fichero>.rs
cargo clean -p <crate>
```

## `crates/xtask/src/judge.rs` — 2026-10-10

- **Corrida 1**: 142 mutantes, 31 missed, 109 caught, 1 unviable, 1 timeout →
  MS 78,0 %. Acciones: conteos exactos por rúbrica en el fixture sucio,
  caso degenerado de κ, test fixture del contrato (K1/K2), tests estrictos de
  `specacs`/`ac_number`/`markers_in`.
- **Corrida 2**: 142 mutantes, 3 missed, 137 caught, 1 unviable, 1 timeout →
  MS 97,9 %. Restantes, todos `EQUIVALENTE`:

| Mutante | Clase | Justificación |
|---|---|---|
| `278:36,63` `\|\|` → `&&` (salto de cabeceras `+++`/`---`/`@@`) | EQUIVALENTE | Las líneas de protocolo nunca son contenido: toda línea `+++` es `+++ b/` (manejada en 273) o basura que no produce hallazgos en ninguna rama. |
| `379:33` `<` → `<=` (guarda anti-división-por-cero de κ) | EQUIVALENTE | Solo difiere si `\|1-chance\| == EPSILON` exacto (evento de medida cero en flotante); el degenerado real (`chance == 1`) está testeado y ambas ramas coinciden. |

## `crates/ruscadb-txn/src/mvcc.rs` — 2026-10-10

- **Corrida 1**: 49 mutantes, 1 missed, 43 caught, 3 unviable, 2 timeouts →
  MS 97,8 %. Restante:

| Mutante | Clase | Justificación |
|---|---|---|
| `366:55` `<<` → `>>` (backoff de `retry_on_conflict`) | EQUIVALENTE | Solo cambia la duración del sleep (0 µs vs backoff); la convergencia no depende del backoff. Matarlo exigiría asserts de tiempo wall-clock (frágiles en CI). |
