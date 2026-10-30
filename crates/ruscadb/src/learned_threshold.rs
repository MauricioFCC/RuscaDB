//! Umbral aprendido online para cache semantico (estilo vCache).
//!
//! Mantiene histogramas de similitud para aciertos (`hit`) y fallos (`miss`) y
//! recalcula el umbral que maximiza la diferencia entre tasa de verdaderos
//! positivos y falsos positivos (indice de Youden).
//!
//! Complejidad: `observe` es `O(1)`; `recompute` es `O(buckets)` (barrido
//! acumulado), independiente del numero de observaciones. No hay `O(n^2)`.
//!
//! Referencia: vCache (ICLR 2026), umbral aprendido por prompt con cota de
//! error definida por el usuario.

/// Aprendiz de umbral por histogramas.
#[derive(Debug, Clone)]
pub struct LearnedThreshold {
    positive: Vec<u64>,
    negative: Vec<u64>,
    /// Umbral vigente en `[0, 1]`.
    pub threshold: f64,
    /// Observaciones totales.
    pub observations: u64,
    /// Recalculos ejecutados.
    pub updates: u64,
}

impl Default for LearnedThreshold {
    fn default() -> Self {
        Self::new(64)
    }
}

impl LearnedThreshold {
    /// Crea el aprendiz con `buckets` intervalos.
    ///
    /// Args:
    ///     buckets: Resolucion del histograma (minimo 2).
    ///
    /// Returns:
    ///     Aprendiz con umbral inicial 0.5.
    pub fn new(buckets: usize) -> Self {
        let buckets = buckets.max(2);
        Self {
            positive: vec![0; buckets],
            negative: vec![0; buckets],
            threshold: 0.5,
            observations: 0,
            updates: 0,
        }
    }

    /// Numero de intervalos.
    ///
    /// Returns:
    ///     Resolucion del histograma.
    pub fn buckets(&self) -> usize {
        self.positive.len()
    }

    /// Registra una observacion.
    ///
    /// Args:
    ///     similarity: Similitud observada en `[0, 1]` (se satura).
    ///     was_hit: `true` si fue un acierto real.
    pub fn observe(&mut self, similarity: f64, was_hit: bool) {
        let buckets = self.buckets();
        let clamped = similarity.clamp(0.0, 0.999_999);
        let index = ((clamped * buckets as f64) as usize).min(buckets - 1);
        if was_hit {
            self.positive[index] += 1;
        } else {
            self.negative[index] += 1;
        }
        self.observations += 1;
    }

    /// Recalcula el umbral (indice de Youden sobre los histogramas).
    ///
    /// Returns:
    ///     Umbral vigente tras el recalculo.
    pub fn recompute(&mut self) -> f64 {
        let buckets = self.buckets();
        let total_positive: u64 = self.positive.iter().sum();
        let total_negative: u64 = self.negative.iter().sum();
        if total_positive == 0 || total_negative == 0 {
            // Sin ambas clases no hay separacion aprendible: se conserva.
            self.updates += 1;
            return self.threshold;
        }
        let mut positives_above = 0u64;
        let mut negatives_above = 0u64;
        let mut best_j = f64::NEG_INFINITY;
        let mut best_index = 0usize;
        for index in (0..buckets).rev() {
            positives_above += self.positive[index];
            negatives_above += self.negative[index];
            let tpr = positives_above as f64 / total_positive as f64;
            let fpr = negatives_above as f64 / total_negative as f64;
            let youden = tpr - fpr;
            if youden > best_j {
                best_j = youden;
                best_index = index;
            }
        }
        self.threshold = best_index as f64 / buckets as f64;
        self.updates += 1;
        self.threshold
    }

    /// Decide si una similitud debe considerarse acierto.
    ///
    /// Args:
    ///     similarity: Similitud observada.
    ///
    /// Returns:
    ///     `true` si `similarity >= threshold`.
    pub fn should_hit(&self, similarity: f64) -> bool {
        similarity >= self.threshold
    }

    /// Tasa de error del umbral vigente sobre los histogramas.
    ///
    /// Returns:
    ///     `(falsos_positivos + falsos_negativos) / observaciones`.
    pub fn error_rate(&self) -> f64 {
        if self.observations == 0 {
            return 0.0;
        }
        let buckets = self.buckets();
        let split = ((self.threshold * buckets as f64) as usize).min(buckets - 1);
        let mut errors = 0u64;
        for index in 0..buckets {
            if index >= split {
                // Por encima del umbral: se aceptan; error = negativos.
                errors += self.negative[index];
            } else {
                // Por debajo: se rechazan; error = positivos.
                errors += self.positive[index];
            }
        }
        errors as f64 / self.observations as f64
    }
}

#[cfg(test)]
mod tests {
    use super::LearnedThreshold;

    /// El umbral aprende a separar aciertos de fallos.
    #[test]
    fn threshold_separates_classes() {
        let mut learner = LearnedThreshold::new(64);
        for _ in 0..100 {
            learner.observe(0.92, true);
        }
        for _ in 0..100 {
            learner.observe(0.30, false);
        }
        let threshold = learner.recompute();
        assert!(
            (0.30..=0.92).contains(&threshold),
            "umbral aprendido: {threshold}"
        );
        assert!(learner.should_hit(0.95));
        assert!(!learner.should_hit(0.20));
        assert!(
            learner.error_rate() < 0.05,
            "error: {}",
            learner.error_rate()
        );
        assert!(learner.updates >= 1);
    }

    /// Sin ambas clases el umbral no cambia (fail-safe).
    #[test]
    fn one_class_keeps_threshold() {
        let mut learner = LearnedThreshold::new(32);
        for _ in 0..10 {
            learner.observe(0.8, true);
        }
        assert!((learner.recompute() - 0.5).abs() < 1e-9);
        // Todos los positivos (0.8) estan por encima del umbral 0.5: sin error.
        assert_eq!(learner.error_rate(), 0.0);
        let empty = LearnedThreshold::new(2);
        assert_eq!(empty.error_rate(), 0.0);
    }

    /// El recalculo es barato y repetible (O(buckets), no O(n)).
    #[test]
    fn recompute_is_stable() {
        let mut learner = LearnedThreshold::new(16);
        for step in 0..1000 {
            learner.observe(if step % 2 == 0 { 0.9 } else { 0.1 }, step % 2 == 0);
        }
        let first = learner.recompute();
        let second = learner.recompute();
        assert!((first - second).abs() < 1e-9);
        assert_eq!(learner.buckets(), 16);
    }
}
