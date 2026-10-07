//! Fuzz target del parser RQL (`ruscadb_query::parse`).
//!
//! Invariante: `parse` nunca entra en pánico ante texto arbitrario; toda
//! entrada inválida se resuelve con `RuscaError::ParseError` (STRIDE: DoS por
//! parser y elevación por input malformado, roadmap §6.2).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|query: &str| {
    // El parser debe devolver un `Result`, nunca abortar el proceso.
    let _ = ruscadb_query::parse(query);
});
