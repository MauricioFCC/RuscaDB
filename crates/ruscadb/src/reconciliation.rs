//! Reconciliacion programada de memoria (estilo SSGM).
//!
//! Cada `interval` pasos se revalidan entradas contra el estado actual: las que
//! llevan mas de `stale_after_steps` sin validarse y tienen confianza baja se
//! ponen en cuarentena. El barrido es **incremental y acotado**: se procesan
//! como maximo `budget` entradas por ejecucion con un cursor circular, de modo
//! que el coste por paso es `O(budget)` y nunca `O(n^2)`.
//!
//! Referencia: SSGM (Lam et al., 2026), reconciliacion cada N pasos que acota
//! el drift.

/// Entrada reconciliable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcilableEntry {
    /// Identificador.
    pub id: u64,
    /// Ultimo paso en que se valido.
    pub last_validated_step: u64,
    /// Confianza (`0..=3`; `< 2` es baja).
    pub trust_tier: u8,
}

/// Politica de reconciliacion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconciliationPolicy {
    /// Se ejecuta cada `interval` pasos (0 = nunca).
    pub interval: u64,
    /// Entradas como maximo por ejecucion.
    pub budget: usize,
    /// Antiguedad (pasos) a partir de la cual una entrada es obsoleta.
    pub stale_after_steps: u64,
}

impl Default for ReconciliationPolicy {
    fn default() -> Self {
        Self {
            interval: 100,
            budget: 64,
            stale_after_steps: 1_000,
        }
    }
}

/// Resultado de una pasada.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconciliationReport {
    /// Entradas inspeccionadas.
    pub scanned: usize,
    /// Ids puestos en cuarentena.
    pub quarantined: Vec<u64>,
}

/// Job de reconciliacion con cursor circular.
#[derive(Debug, Clone)]
pub struct ReconciliationJob {
    /// Politica aplicada.
    pub policy: ReconciliationPolicy,
    cursor: usize,
}

impl Default for ReconciliationJob {
    fn default() -> Self {
        Self::new(ReconciliationPolicy::default())
    }
}

impl ReconciliationJob {
    /// Crea el job con una politica.
    ///
    /// Args:
    ///     policy: Intervalo, presupuesto y antiguedad.
    ///
    /// Returns:
    ///     Job listo.
    pub fn new(policy: ReconciliationPolicy) -> Self {
        Self { policy, cursor: 0 }
    }

    /// Indica si toca ejecutar en `step`.
    ///
    /// Args:
    ///     step: Paso logico.
    ///
    /// Returns:
    ///     `true` si `interval > 0 && step % interval == 0`.
    pub fn should_run(&self, step: u64) -> bool {
        self.policy.interval > 0 && step.is_multiple_of(self.policy.interval)
    }

    /// Ejecuta una pasada acotada si corresponde.
    ///
    /// Args:
    ///     step: Paso logico actual.
    ///     entries: Entradas a reconciliar (se marcan como validadas).
    ///
    /// Returns:
    ///     Reporte con lo escaneado y lo puesto en cuarentena.
    pub fn run(&mut self, step: u64, entries: &mut [ReconcilableEntry]) -> ReconciliationReport {
        let mut report = ReconciliationReport::default();
        if !self.should_run(step) || entries.is_empty() || self.policy.budget == 0 {
            return report;
        }
        let total = entries.len();
        let take = self.policy.budget.min(total);
        for offset in 0..take {
            let index = (self.cursor + offset) % total;
            let entry = &mut entries[index];
            report.scanned += 1;
            let age = step.saturating_sub(entry.last_validated_step);
            if entry.trust_tier < 2 && age >= self.policy.stale_after_steps {
                report.quarantined.push(entry.id);
                // La cuarentena no borra: sube la marca para no repetir el aviso
                // en cada pasada mientras siga sin validarse.
                entry.trust_tier = 0;
                entry.last_validated_step = step;
            } else {
                entry.last_validated_step = step;
            }
        }
        self.cursor = (self.cursor + take) % total;
        report
    }
}

#[cfg(test)]
mod tests {
    use super::{ReconcilableEntry, ReconciliationJob, ReconciliationPolicy};

    /// Solo corre en multiplos del intervalo.
    #[test]
    fn runs_on_interval() {
        let job = ReconciliationJob::new(ReconciliationPolicy {
            interval: 10,
            budget: 2,
            stale_after_steps: 5,
        });
        assert!(job.should_run(0));
        assert!(!job.should_run(7));
        assert!(job.should_run(20));
    }

    /// El presupuesto acota el trabajo y el cursor avanza.
    #[test]
    fn budget_bounds_work_and_cursor_rotates() {
        let policy = ReconciliationPolicy {
            interval: 1,
            budget: 2,
            stale_after_steps: 100,
        };
        let mut job = ReconciliationJob::new(policy);
        let mut entries: Vec<ReconcilableEntry> = (0..5)
            .map(|id| ReconcilableEntry {
                id,
                last_validated_step: 0,
                trust_tier: 3,
            })
            .collect();
        let first = job.run(1, &mut entries);
        assert_eq!(first.scanned, 2);
        assert_eq!(entries[0].last_validated_step, 1);
        assert_eq!(entries[1].last_validated_step, 1);
        assert_eq!(entries[2].last_validated_step, 0);
        let second = job.run(2, &mut entries);
        assert_eq!(second.scanned, 2);
        assert_eq!(entries[2].last_validated_step, 2, "el cursor rota");
        assert_eq!(entries[3].last_validated_step, 2);
    }

    /// Las entradas obsoletas y de baja confianza se ponen en cuarentena.
    #[test]
    fn quarantines_stale_low_trust() {
        let mut job = ReconciliationJob::new(ReconciliationPolicy {
            interval: 1,
            budget: 10,
            stale_after_steps: 5,
        });
        let mut entries = vec![
            ReconcilableEntry {
                id: 1,
                last_validated_step: 0,
                trust_tier: 1,
            },
            ReconcilableEntry {
                id: 2,
                last_validated_step: 0,
                trust_tier: 3,
            },
            ReconcilableEntry {
                id: 3,
                last_validated_step: 9,
                trust_tier: 1,
            },
        ];
        let report = job.run(10, &mut entries);
        assert_eq!(
            report.quarantined,
            vec![1],
            "solo la obsoleta y de baja confianza"
        );
        assert_eq!(
            entries[1].last_validated_step, 10,
            "la confiable se revalida"
        );
        assert_eq!(
            entries[2].last_validated_step, 10,
            "la reciente se revalida"
        );
        assert_eq!(job.run(11, &mut []).scanned, 0);
    }
}
