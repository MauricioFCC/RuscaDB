"use strict";

// RuscaDB — placeholder de reserva del nombre en npm.
// El binding Node.js real (napi-rs) se implementa en la Fase F5 del roadmap
// (docs/RuscaDB-roadmap.md §8). Este paquete solo reserva el nombre `ruscadb`.

const version = "0.1.0";

/**
 * Devuelve un mensaje indicando que el binding aun no esta disponible.
 * @returns {string} Mensaje informativo sobre el estado del paquete.
 */
function placeholder() {
  return "RuscaDB Node.js bindings arrive in Phase F5.";
}

module.exports = { version, placeholder };
