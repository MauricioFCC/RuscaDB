//! Identificador estable de registro basado en ULID.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::error::RuscaError;

/// Identificador de un [`crate::Record`] basado en ULID.
///
/// Un ULID combina 48 bits de timestamp (ms) y 80 bits aleatorios, codificados
/// en 26 caracteres Crockford base32. Su orden lexicográfico coincide con el
/// orden temporal, lo que permite paginar por tiempo sin índice adicional.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RecordId(Ulid);

impl RecordId {
    /// Genera un nuevo identificador ULID con el timestamp actual.
    ///
    /// Returns:
    ///     Un `RecordId` único y ordenable por tiempo de creación.
    pub fn new() -> Self {
        Self(Ulid::generate())
    }

    /// Construye un `RecordId` a partir de su representación textual canónica.
    ///
    /// Args:
    ///     text: Cadena ULID de 26 caracteres Crockford base32.
    ///
    /// Returns:
    ///     El identificador, o [`RuscaError::InvalidId`] si la cadena no es
    ///     un ULID válido.
    pub fn from_string(text: &str) -> Result<Self, RuscaError> {
        Ulid::from_string(text)
            .map(Self)
            .map_err(|error| RuscaError::InvalidId(error.to_string()))
    }
}

impl Default for RecordId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl FromStr for RecordId {
    type Err = RuscaError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::from_string(text)
    }
}
