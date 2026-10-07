// compaction-context.js — ADR-0039 #3: inyectar/preservar contexto persistente
// al compactar (estado de tarea, decisiones, archivos activos).
//
// Carga: opencode auto-descubre plugins en .opencode/plugin/ (1.18.x; el path
// .opencode/plugin/ esta embebido en el binario 1.18.1, verificado).
// Sin dependencias: NO importa @opencode-ai/plugin (no requiere npm install).
//
// Hooks registrados:
//  1. "experimental.session.compacting" — existe en opencode 1.18 (verificado en
//     el binario instalado y en docs oficiales de plugins). Recibe (input, output).
//     Mutacion SEGURA: solo agrega contexto si output.context es un array y no
//     hay output.prompt seteado (la doc indica que si output.prompt esta set,
//     output.context se ignora por completo — no lo pisamos).
//  2. "event" — red de seguridad / observabilidad: filtra eventos de
//     session.compacting / session.compacted y solo loguea a stderr.
//     Nunca rompe nada; si el hook experimental no existiera en otra version,
//     este filtro sigue activo y el plugin retorna {} sin romper.
//
// Logging: process.stderr.write (console.error NO permitido en este repo).

const PERSISTENT_CONTEXT = `## Contexto persistente (compaction-context plugin, ADR-0039 #3)
Preserva en el resumen de compactacion el estado que debe sobrevivir a la sesion:
- Estado de la tarea actual: objetivo, progreso, bloqueos y siguiente accion.
- Decisiones tomadas: que se decidio, por que, y que archivos las registran.
- Archivos activos: rutas y rol de cada archivo en la tarea.
- Hand-offs de subagentes: que se delego, a quien, y que artefactos produjo cada uno.`;

export default async ({ client, project, directory, $ }) => {
  return {
    "experimental.session.compacting": async (input, output) => {
      try {
        if (
          output &&
          Array.isArray(output.context) &&
          !output.prompt
        ) {
          output.context.push(PERSISTENT_CONTEXT);
        }
      } catch (err) {
        // nunca romper la compactacion: log y pasar
        process.stderr.write(
          "[compaction-context] error en experimental.session.compacting: " +
            (err && err.message) + "\n"
        );
      }
    },
    event: async ({ event }) => {
      if (
        event &&
        typeof event.type === "string" &&
        (event.type === "session.compacting" || event.type === "session.compacted")
      ) {
        process.stderr.write("[compaction-context] evento: " + event.type + "\n");
      }
    },
  };
};
