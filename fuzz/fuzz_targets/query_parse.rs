//! Fuzz target del parser RQL (`ruscadb_query::parse_statement` / `parse`).
//!
//! Contrato de no-pánico: ante CUALQUIER texto arbitrario el parser debe
//! resolver con `Ok(_)` o `Err(RuscaError::ParseError)`; jamás debe abortar el
//! proceso. Ninguna ruta de este target usa `unwrap` ni `expect`: un `Result`
//! de error es la salida esperada (STRIDE: DoS por parser y elevación por
//! input malformado, roadmap §6.2).
//!
//! Gramática RQL ejercitada por este target (SPEC-0042/AC-0042-01):
//! - `SELECT` con proyección `*` o lista de columnas.
//! - `WHERE` con comparaciones (`=`, `!=`, `<`, `<=`, `>`, `>=`) unidas por `AND`.
//! - `MATCH(columna, 'texto')` (búsqueda full-text).
//! - `KNN <col> <|k|> [v1, v2, ...]` (búsqueda vectorial).
//! - `TRAVERSE <col> DEPTH <n>` (recorrido de grafo).
//! - `ORDER BY <col> [ASC|DESC]` (orden canónico antes de `LIMIT`).
//! - `GROUP BY <col>` (forma reservada: el corpus la siembra y el parser la
//!   rechaza con `ParseError`, nunca con pánico).
//! - `LIMIT <n>`.
//! - `EXPLAIN <select>` (envoltura de sentencia).
//!
//! Se invocan ambas entradas públicas: `parse_statement` cubre `SELECT` y
//! `EXPLAIN <select>`; `parse` desempaqueta únicamente `SELECT` (rechaza
//! `EXPLAIN` con `ParseError`).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|query: &str| {
    // `parse_statement` acepta `SELECT` y `EXPLAIN <select>`.
    let _ = ruscadb_query::parse_statement(query);
    // `parse` acepta solo `SELECT` (rechaza `EXPLAIN` de forma controlada).
    let _ = ruscadb_query::parse(query);
});
