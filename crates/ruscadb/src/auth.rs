//! RBAC por rol de instancia + audit en memoria (SPEC-0066).
//!
//! Rol por defecto `Admin` (cero regresión). `Read` no escribe; `Write` no
//! administra; `Admin` lo puede todo. El audit (OFF por defecto) captura
//! operaciones (`insert*`, `execute`, `put_blob`, administración) en memoria,
//! en orden y con timestamp en nanos.

use ruscadb_core::RuscaError;

use crate::database::Database;

/// Tope del fichero de audit antes de rotar (SPEC-0073).
pub(crate) const MAX_AUDIT_BYTES: u64 = 1_048_576;
/// Máximo de entradas cargadas en memoria al abrir (SPEC-0073).
pub(crate) const MAX_AUDIT_ENTRIES: usize = 10_000;

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
        // Coherencia con la persistencia (SPEC-0073): el fichero también se vacía.
        let path = self.audit_path().to_path_buf();
        if path.exists() {
            std::fs::write(&path, b"")?;
        }
        Ok(())
    }

    /// Ruta del fichero de audit persistido (SPEC-0073).
    ///
    /// Returns:
    ///     La ruta `<data>.audit.jsonl` vigente.
    pub(crate) fn audit_path(&self) -> &std::path::Path {
        &self.audit_path
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
        // Reloj monótono (SPEC-0070): base `Instant` + contador estrictamente
        // creciente (SystemTime puede retroceder con ajustes de NTP).
        let elapsed = self.audit_epoch.elapsed().as_nanos() as u64;
        let nanos = if elapsed > self.audit_last_nanos {
            elapsed
        } else {
            self.audit_last_nanos + 1
        };
        self.audit_last_nanos = nanos;
        self.audit.push(AuditEntry {
            action: action.to_string(),
            detail: detail.to_string(),
            nanos,
        });
        // Persistencia append-only (SPEC-0073): mejor esfuerzo, el trail en
        // memoria sigue siendo la fuente inmediata.
        if let Err(error) = self.append_audit_line(action, detail, nanos) {
            self.audit_push_error(&error);
        }
    }

    /// Añade una línea al fichero de audit (con rotación por tamaño).
    ///
    /// Formato: `{"action":..,"detail":..,"nanos":N}\t<crc32c>\n`.
    ///
    /// Args:
    ///     action: Operación auditada.
    ///     detail: Detalle libre.
    ///     nanos: Timestamp monótono.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si falla la escritura o la rotación.
    fn append_audit_line(&self, action: &str, detail: &str, nanos: u64) -> Result<(), RuscaError> {
        use std::io::Write;
        let path = self.audit_path();
        if path.metadata().map(|meta| meta.len()).unwrap_or(0) > MAX_AUDIT_BYTES {
            let rotated = std::path::PathBuf::from(format!("{}.1", path.display()));
            std::fs::rename(path, rotated)?;
        }
        let payload = serde_json::json!({
            "action": action,
            "detail": detail,
            "nanos": nanos,
        });
        let payload_text = payload.to_string();
        let crc = crc32c::crc32c(payload_text.as_bytes());
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(format!("{payload_text}\t{crc}\n").as_bytes())?;
        file.flush()?;
        Ok(())
    }

    /// Carga el audit persistido al abrir la base (descarta líneas corruptas).
    ///
    /// Args:
    ///     self: Base recién abierta.
    ///
    /// Errors:
    ///     [`RuscaError::Io`] si el fichero existe pero no se puede leer.
    pub(crate) fn restore_audit(&mut self) -> Result<(), RuscaError> {
        let path = self.audit_path().to_path_buf();
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&path)?;
        for line in content.lines().take(MAX_AUDIT_ENTRIES) {
            let Some((payload_text, crc_text)) = line.split_once('\t') else {
                continue; // Línea sin CRC: corrupta, se descarta.
            };
            let Ok(expected) = crc_text.trim().parse::<u32>() else {
                continue;
            };
            if crc32c::crc32c(payload_text.as_bytes()) != expected {
                continue; // CRC alterado: línea corrupta.
            }
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_text) else {
                continue;
            };
            let (Some(action), Some(detail), Some(nanos)) = (
                payload.get("action").and_then(serde_json::Value::as_str),
                payload.get("detail").and_then(serde_json::Value::as_str),
                payload.get("nanos").and_then(serde_json::Value::as_u64),
            ) else {
                continue;
            };
            self.audit.push(AuditEntry {
                action: action.to_string(),
                detail: detail.to_string(),
                nanos,
            });
            self.audit_last_nanos = self.audit_last_nanos.max(nanos);
        }
        Ok(())
    }

    /// Registra un fallo de persistencia del audit en el trail (observable).
    ///
    /// Args:
    ///     error: Error de escritura del fichero.
    fn audit_push_error(&mut self, error: &RuscaError) {
        let nanos = self.audit_last_nanos + 1;
        self.audit_last_nanos = nanos;
        self.audit.push(AuditEntry {
            action: "audit_persist_error".to_string(),
            detail: error.to_string(),
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
