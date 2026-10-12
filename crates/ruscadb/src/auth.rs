//! RBAC por rol de instancia + audit en memoria (SPEC-0066).
//!
//! Rol por defecto `Admin` (cero regresión). `Read` no escribe; `Write` no
//! administra; `Admin` lo puede todo. El audit (OFF por defecto) captura
//! operaciones (`insert*`, `execute`, `put_blob`, administración) en memoria,
//! en orden y con timestamp en nanos.

use ruscadb_core::RuscaError;

use crate::database::Database;

/// Rol de instancia de la base (SPEC-0066).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Solo lectura: puede consultar, no escribir ni administrar.
    Read,
    /// Lectura + escritura de datos; no administrar.
    Write,
    /// Todo: lectura, escritura y administración.
    Admin,
}

impl Default for Role {
    /// Una base nueva se abre como `Admin` (cero regresión).
    fn default() -> Self {
        Self::Admin
    }
}

/// Entrada del registro de auditoría en memoria (SPEC-0066).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// Operación auditada (p. ej. `insert`, `execute`, `put_blob`).
    pub action: String,
    /// Detalle libre (query o payload truncado).
    pub detail: String,
    /// Timestamp en nanos desde la época (monótono de reloj).
    pub nanos: u64,
}

impl Database {
    /// Cambia el rol de la instancia (solo `Admin`).
    ///
    /// Args:
    ///     role: Nuevo rol.
    ///
    /// Errors:
    ///     [`RuscaError::PermissionDenied`] si el rol actual no es `Admin`.
    pub fn set_role(&mut self, role: Role) -> Result<(), RuscaError> {
        self.require_admin("set_role")?;
        self.role = role;
        Ok(())
    }

    /// Rol vigente de la instancia.
    ///
    /// Returns:
    ///     El rol configurado (por defecto `Admin`).
    pub fn current_role(&self) -> Role {
        self.role
    }

    /// Habilita o deshabilita la captura de auditoría (solo `Admin`).
    ///
    /// Args:
    ///     enabled: `true` empieza a capturar operaciones.
    ///
    /// Errors:
    ///     [`RuscaError::PermissionDenied`] si el rol actual no es `Admin`.
    pub fn set_audit(&mut self, enabled: bool) -> Result<(), RuscaError> {
        self.require_admin("set_audit")?;
        self.audit_enabled = enabled;
        Ok(())
    }

    /// Devuelve el trail de auditoría acumulado.
    ///
    /// Returns:
    ///     Las entradas en orden de captura (append-only).
    pub fn audit_trail(&self) -> Vec<AuditEntry> {
        self.audit.clone()
    }

    /// Vacía el trail SIN deshabilitar la captura (solo `Admin`).
    ///
    /// Errors:
    ///     [`RuscaError::PermissionDenied`] si el rol actual no es `Admin`.
    pub fn clear_audit(&mut self) -> Result<(), RuscaError> {
        self.require_admin("clear_audit")?;
        self.audit.clear();
        Ok(())
    }

    /// Requiere rol `Write` o superior para operaciones de escritura.
    ///
    /// Args:
    ///     action: Operación que se autoriza.
    ///
    /// Errors:
    ///     [`RuscaError::PermissionDenied`] con la acción si el rol es `Read`.
    pub(crate) fn require_write(&self, action: &str) -> Result<(), RuscaError> {
        match self.role {
            Role::Read => Err(RuscaError::PermissionDenied {
                action: action.to_string(),
            }),
            Role::Write | Role::Admin => Ok(()),
        }
    }

    /// Requiere rol `Admin` para operaciones de administración.
    ///
    /// Args:
    ///     action: Operación que se autoriza.
    ///
    /// Errors:
    ///     [`RuscaError::PermissionDenied`] con la acción si el rol no es `Admin`.
    pub(crate) fn require_admin(&self, action: &str) -> Result<(), RuscaError> {
        if self.role == Role::Admin {
            Ok(())
        } else {
            Err(RuscaError::PermissionDenied {
                action: action.to_string(),
            })
        }
    }

    /// Registra una operación en el trail si el audit está habilitado.
    ///
    /// Args:
    ///     action: Operación ejecutada.
    ///     detail: Detalle de la operación.
    pub(crate) fn note_audit(&mut self, action: &str, detail: &str) {
        if !self.audit_enabled {
            return;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos() as u64);
        self.audit.push(AuditEntry {
            action: action.to_string(),
            detail: detail.to_string(),
            nanos,
        });
    }

    /// Comprueba si una sentencia RQL es DML de escritura por su verbo.
    ///
    /// Args:
    ///     query: Texto RQL.
    ///
    /// Returns:
    ///     `true` si empieza por `INSERT`/`UPDATE`/`DELETE`.
    pub(crate) fn is_write_statement(query: &str) -> bool {
        let trimmed = query.trim_start();
        trimmed
            .get(..7)
            .is_some_and(|prefix| prefix.to_ascii_uppercase().starts_with("INSERT"))
            || trimmed
                .get(..6)
                .is_some_and(|prefix| prefix.to_ascii_uppercase().starts_with("UPDATE"))
            || trimmed
                .get(..6)
                .is_some_and(|prefix| prefix.to_ascii_uppercase().starts_with("DELETE"))
    }
}
