//! Suite de aceptación de la versión y ABI de la C-ABI (SPEC-0078).
//!
//! Verifica los símbolos de estabilidad expuestos por `ruscadb-ffi`:
//!
//! - `RUSCADB_ABI_VERSION` (invariante del ABI, entero monótono).
//! - `ruscadb_version()` (semver semántica del crate, según el patrón
//!   buffer + longitud de `ruscadb_last_error`).

#![forbid(unsafe_code)]

use core::ffi::c_char;

use ruscadb_ffi::{RUSCADB_ABI_VERSION, ruscadb_version};

/// AC-0078-02 — `RUSCADB_ABI_VERSION >= 1` y `ruscadb_version()` coincide con
/// `CARGO_PKG_VERSION` del workspace.
///
/// La versión semántica del crate `ruscadb-ffi` hereda la del workspace; el
/// símbolo C debe devolver exactamente la misma cadena, y el ABI declarado debe
/// ser al menos 1 (primer contrato estable).
#[test]
// @spec AC-0078-02
fn test_ac_0078_02_abi_version_symbols() {
    assert!(
        RUSCADB_ABI_VERSION >= 1,
        "RUSCADB_ABI_VERSION debe ser >= 1, se obtuvo {RUSCADB_ABI_VERSION}"
    );

    let expected = env!("CARGO_PKG_VERSION");
    let mut buffer = [0 as c_char; 64];
    let length = ruscadb_version(buffer.as_mut_ptr(), buffer.len());
    assert!(
        length < buffer.len(),
        "el buffer de versión es demasiado pequeño (requiere {length} bytes)"
    );

    let bytes: Vec<u8> = buffer[..length].iter().map(|byte| *byte as u8).collect();
    let version = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => panic!("la versión devuelta no es UTF-8 válido: {error}"),
    };
    assert_eq!(
        version, expected,
        "ruscadb_version() debe coincidir con CARGO_PKG_VERSION del workspace"
    );
    assert_eq!(
        buffer[length], 0,
        "ruscadb_version() debe terminar la copia en NUL"
    );

    // Consulta de longitud con buffer nulo (patrón de `ruscadb_last_error`).
    assert_eq!(
        ruscadb_version(core::ptr::null_mut(), 0),
        length,
        "con buffer nulo debe devolver solo la longitud"
    );
}
