#![allow(missing_docs)]
#![allow(dead_code)]
//! Observabilidad opcional (SPEC-0119): métricas en memoria.

use std::collections::BTreeMap;

/// Registry de métricas en memoria.
#[derive(Debug, Clone, Default)]
pub struct MetricsRegistry {
    /// Contadores por nombre.
    counters: BTreeMap<String, u64>,
}

impl MetricsRegistry {
    /// Crea un registry vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Incrementa un contador.
    pub fn inc(&mut self, name: &str) {
        *self.counters.entry(name.to_string()).or_insert(0) += 1;
    }

    /// Incrementa un contador por un valor.
    pub fn inc_by(&mut self, name: &str, value: u64) {
        *self.counters.entry(name.to_string()).or_insert(0) += value;
    }

    /// Obtiene el valor de un contador.
    pub fn get(&self, name: &str) -> u64 {
        self.counters.get(name).copied().unwrap_or(0)
    }

    /// Exporta las métricas en formato texto Prometheus.
    pub fn export(&self) -> String {
        let mut out = String::new();
        for (name, value) in &self.counters {
            out.push_str(&format!("# TYPE {name} counter\n"));
            out.push_str(&format!("{name} {value}\n"));
        }
        out
    }

    /// Número de métricas registradas.
    pub fn len(&self) -> usize {
        self.counters.len()
    }

    /// Indica si está vacío.
    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0119-01
    fn test_ac_0119_01_metrics_export() {
        let mut reg = MetricsRegistry::new();
        reg.inc("queries_total");
        reg.inc("queries_total");
        reg.inc_by("errors_total", 3);
        let exported = reg.export();
        assert!(exported.contains("queries_total 2"));
        assert!(exported.contains("errors_total 3"));
        assert_eq!(reg.get("queries_total"), 2);
        assert_eq!(reg.len(), 2);
    }
}
