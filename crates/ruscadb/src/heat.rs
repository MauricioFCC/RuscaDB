//! Eviccion por calor estilo MemoryOS (STM/MTM/LPM).
//!
//! Cada entrada acumula accesos y se envejece exponencialmente: `score =
//! accesos * decay^(epochs_sin_acceso)`. Las entradas se clasifican en
//! `Hot`/`Warm`/`Cold` por umbral de score y la eviccion expulsa primero las
//! mas frias.
//!
//! Complejidad: registrar es `O(1)`; evacuar `b` entradas es `O(n log n + b)`
//! (una sola ordenacion), nunca `O(n^2)`.
//!
//! Referencia: Kang et al., "MemoryOS", EMNLP 2025 (eviccion por calor
//! segmentada STM/MTM/LPM).

use std::collections::BTreeMap;

/// Nivel de calor de una entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Fria: candidata a eviccion.
    Cold,
    /// Tibia: se conserva con presupuesto.
    Warm,
    /// Caliente: se conserva siempre que se pueda.
    Hot,
}

/// Estado de calor de una entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeatEntry {
    /// Accesos acumulados.
    pub accesses: u64,
    /// Ultimo epoch en que se accedio.
    pub last_epoch: u64,
}

/// Tracker de calor con decaimiento exponencial.
#[derive(Debug, Clone)]
pub struct HeatTracker {
    entries: BTreeMap<u64, HeatEntry>,
    /// Score minimo para `Hot`.
    pub hot_threshold: f64,
    /// Score minimo para `Warm`.
    pub warm_threshold: f64,
    /// Factor de decaimiento por epoch (`0 < decay <= 1`).
    pub decay: f64,
}

impl Default for HeatTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl HeatTracker {
    /// Crea el tracker con umbrales razonables.
    ///
    /// Returns:
    ///     Tracker vacio (`hot = 4.0`, `warm = 1.0`, `decay = 0.9`).
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            hot_threshold: 4.0,
            warm_threshold: 1.0,
            decay: 0.9,
        }
    }

    /// Registra un acceso en `epoch`.
    ///
    /// Args:
    ///     id: Identificador de la entrada.
    ///     epoch: Paso logico actual.
    pub fn record(&mut self, id: u64, epoch: u64) {
        let entry = self.entries.entry(id).or_insert(HeatEntry {
            accesses: 0,
            last_epoch: epoch,
        });
        entry.accesses = entry.accesses.saturating_add(1);
        entry.last_epoch = entry.last_epoch.max(epoch);
    }

    /// Alias de [`Self::record`].
    ///
    /// Args:
    ///     id: Identificador.
    ///     epoch: Paso logico.
    pub fn record_access(&mut self, id: u64, epoch: u64) {
        self.record(id, epoch);
    }

    /// Score de calor actual (con decaimiento por inactividad).
    ///
    /// Args:
    ///     id: Identificador.
    ///     epoch: Paso logico actual.
    ///
    /// Returns:
    ///     Score `>= 0`; `0.0` si la entrada no existe.
    pub fn score(&self, id: u64, epoch: u64) -> f64 {
        let Some(entry) = self.entries.get(&id) else {
            return 0.0;
        };
        let age = epoch.saturating_sub(entry.last_epoch) as i32;
        entry.accesses as f64 * self.decay.powi(age)
    }

    /// Clasifica una entrada.
    ///
    /// Args:
    ///     id: Identificador.
    ///     epoch: Paso logico actual.
    ///
    /// Returns:
    ///     `Some(tier)` o `None` si no existe.
    pub fn tier(&self, id: u64, epoch: u64) -> Option<Tier> {
        let score = self.score(id, epoch);
        if self.entries.contains_key(&id) {
            Some(if score >= self.hot_threshold {
                Tier::Hot
            } else if score >= self.warm_threshold {
                Tier::Warm
            } else {
                Tier::Cold
            })
        } else {
            None
        }
    }

    /// Expulsa hasta `budget` entradas, empezando por las mas frias.
    ///
    /// Args:
    ///     budget: Maximo de entradas a expulsar.
    ///     epoch: Paso logico actual.
    ///
    /// Returns:
    ///     Ids expulsados, de mas frio a mas caliente.
    pub fn evict(&mut self, budget: usize, epoch: u64) -> Vec<u64> {
        if budget == 0 || self.entries.is_empty() {
            return Vec::new();
        }
        let mut ranked: Vec<(f64, u64)> = self
            .entries
            .keys()
            .map(|&id| (self.score(id, epoch), id))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let victims: Vec<u64> = ranked.into_iter().take(budget).map(|(_, id)| id).collect();
        for id in &victims {
            self.entries.remove(id);
        }
        victims
    }

    /// Numero de entradas rastreadas.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Indica si no hay entradas.
    ///
    /// Returns:
    ///     `true` si vacio.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{HeatTracker, Tier};

    /// Los accesos suben de tier y la inactividad los enfria.
    #[test]
    fn tiers_follow_access_and_decay() {
        let mut heat = HeatTracker::new();
        heat.record(7, 0);
        assert_eq!(heat.tier(7, 0), Some(Tier::Warm));
        for epoch in 1..=5 {
            heat.record(7, epoch);
        }
        assert_eq!(heat.tier(7, 5), Some(Tier::Hot));
        // 100 epochs sin acceso -> frio.
        assert_eq!(heat.tier(7, 105), Some(Tier::Cold));
        assert_eq!(heat.tier(99, 0), None);
    }

    /// La eviccion prioriza lo mas frio y respeta el presupuesto.
    #[test]
    fn eviction_is_coldest_first() {
        let mut heat = HeatTracker::new();
        heat.record(1, 0);
        for epoch in 0..10 {
            heat.record(2, epoch);
        }
        heat.record(3, 10);
        let victims = heat.evict(2, 10);
        assert_eq!(victims.len(), 2);
        assert_eq!(victims[0], 1, "la entrada 1 es la mas fria");
        assert_eq!(heat.len(), 1);
        assert!(heat.is_empty() || heat.tier(2, 10).is_some());
        assert!(heat.evict(0, 10).is_empty());
    }
}
