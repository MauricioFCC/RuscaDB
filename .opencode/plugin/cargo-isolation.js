// cargo-isolation.js — aislamiento del target dir de Cargo por sesion.
//
// PROBLEMA (contaminacion entre compilaciones):
//   Varios agentes/subagentes comparten el mismo workspace `target/`. Si una
//   compilacion o una corrida de `cargo-mutants --in-place` queda a medias,
//   el `target/` conserva binarios rancios que hacen fallar tests que en
//   realidad estan verdes (falsos negativos) o pasar tests rotos.
//
// SOLUCION:
//   El hook `shell.env` fija `CARGO_TARGET_DIR` a un directorio unico por
//   sesion. Cada subagente tiene su propio `sessionID` => cada uno compila en
//   un `target/` distinto => cero contaminacion cruzada. Los comandos pueden
//   sobrescribirlo (si exportan su propio `CARGO_TARGET_DIR`) y se puede
//   desactivar con `RUSCADB_SHARED_TARGET=1`.
//
// Ver `.opencode/config/rust-isolation.md`.
//
// Sin dependencias del paquete @opencode-ai/plugin (no requiere npm install).

import { tmpdir } from "os";
import { join } from "path";

/** Normaliza el id de sesion para usarlo como nombre de carpeta. */
function sanitize(value) {
  return (
    String(value || "shared")
      .replace(/[^a-zA-Z0-9._-]/g, "_")
      .slice(0, 64) || "shared"
  );
}

export default async () => {
  return {
    "shell.env": async (input, output) => {
      try {
        if (process.env.RUSCADB_SHARED_TARGET === "1") return;
        if (!output || typeof output.env !== "object" || output.env === null) {
          return;
        }
        // Respeta un override explicito del comando.
        if (output.env.CARGO_TARGET_DIR) return;
        const root =
          process.env.RUSCADB_TARGET_ROOT || join(tmpdir(), "ruscadb-targets");
        const session = sanitize(input && input.sessionID);
        output.env.CARGO_TARGET_DIR = join(root, session);
      } catch (err) {
        // Nunca romper la ejecucion por el aislamiento.
        process.stderr.write(
          "[cargo-isolation] error: " + (err && err.message) + "\n"
        );
      }
    },
  };
};
