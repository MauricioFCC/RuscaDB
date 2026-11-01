//! Presupuesto cooperativo de una query (SPEC-0054).

use super::*;

pub(crate) struct QueryBudget {
    /// Instante a partir del cual el scan debe abortar.
    deadline: Instant,
    /// Presupuesto total en ms (para el mensaje de error).
    millis: u64,
}
impl QueryBudget {
    /// Presupuesto con timeout explícito desde ahora.
    pub(crate) fn with_timeout(millis: u64) -> Self {
        Self {
            deadline: Instant::now() + Duration::from_millis(millis),
            millis,
        }
    }

    /// Presupuesto por defecto del planner.
    pub(crate) fn default_budget() -> Self {
        Self::with_timeout(DEFAULT_TIMEOUT_MILLIS)
    }

    /// Presupuesto ya expirado (solo tests: simula un deadline vencido).
    #[cfg(test)]
    pub(crate) fn expired() -> Self {
        Self {
            deadline: Instant::now()
                .checked_sub(Duration::from_secs(3600))
                .unwrap_or_else(Instant::now),
            millis: 0,
        }
    }

    /// Comprueba el deadline cada 1024 filas (`rows` = filas ya vistas).
    ///
    /// El muestreo evita llamar al reloj en cada fila (coste ~0).
    pub(crate) fn check(&self, rows: u64) -> Result<(), RuscaError> {
        if rows.is_multiple_of(1024) && Instant::now() > self.deadline {
            return Err(RuscaError::QueryTimeout {
                millis: self.millis,
            });
        }
        Ok(())
    }
}
