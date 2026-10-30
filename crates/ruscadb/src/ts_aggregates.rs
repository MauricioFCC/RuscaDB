//! Agregados continuos incrementales (SPEC-0145).
//!
//! Mantiene una ventana por bucket y actualiza **solo el bucket del punto**
//! (`O(log B)` por punto), en vez de recalcular todas las ventanas
//! (`O(n)` por punto -> `O(n^2)` en streaming). `merge` es asociativo y
//! conmutativo, lo que permite agregar por particiones y combinar.
//!
//! Formulas: `avg = sum / count`, `min = min(bucket, value)`,
//! `max = max(bucket, value)`. Buckets alineados con `div_euclid` (soporta
//! timestamps negativos).
//!
//! Referencia: Gupta & Mumick, "Maintenance of Materialized Views: Problems,
//! Techniques, and Applications", IEEE Data Eng. Bull. 1995 (agregados
//! incrementales).

use std::collections::BTreeMap;

/// Estado agregado de un bucket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    /// Suma de los valores.
    pub sum: f64,
    /// Numero de puntos.
    pub count: u64,
    /// Minimo observado.
    pub min: f64,
    /// Maximo observado.
    pub max: f64,
}

impl Bucket {
    /// Media del bucket (`0.0` si esta vacio).
    ///
    /// Returns:
    ///     `sum / count`.
    pub fn avg(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }
}

/// Agregado continuo por ventana temporal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContinuousAggregate {
    /// Nombre logico.
    pub name: String,
    /// Tamano de ventana en milisegundos (0 = un bucket por timestamp).
    pub window_ms: i64,
    buckets: BTreeMap<i64, Bucket>,
}

impl ContinuousAggregate {
    /// Crea el agregado.
    ///
    /// Args:
    ///     name: Nombre logico.
    ///     window_ms: Tamano de ventana en ms.
    ///
    /// Returns:
    ///     Agregado vacio.
    pub fn new(name: impl Into<String>, window_ms: i64) -> Self {
        Self {
            name: name.into(),
            window_ms,
            buckets: BTreeMap::new(),
        }
    }

    /// Tamano de ventana.
    ///
    /// Returns:
    ///     `window_ms`.
    pub fn window_ms(&self) -> i64 {
        self.window_ms
    }

    /// Inicio del bucket al que pertenece `ts`.
    ///
    /// Args:
    ///     ts: Timestamp en ms.
    ///
    /// Returns:
    ///     Inicio alineado (`ts` si `window_ms <= 0`).
    pub fn bucket_start(&self, ts: i64) -> i64 {
        if self.window_ms <= 0 {
            ts
        } else {
            ts.div_euclid(self.window_ms) * self.window_ms
        }
    }

    /// Anade un punto y actualiza **solo** su bucket.
    ///
    /// Args:
    ///     ts: Timestamp en ms.
    ///     value: Valor observado.
    ///
    /// Returns:
    ///     Inicio del bucket actualizado.
    pub fn add_point(&mut self, ts: i64, value: f64) -> i64 {
        let start = self.bucket_start(ts);
        let bucket = self.buckets.entry(start).or_insert(Bucket {
            sum: 0.0,
            count: 0,
            min: value,
            max: value,
        });
        bucket.sum += value;
        bucket.count += 1;
        bucket.min = bucket.min.min(value);
        bucket.max = bucket.max.max(value);
        start
    }

    /// Fusiona otro agregado (asociativo y conmutativo).
    ///
    /// Args:
    ///     other: Agregado parcial con la misma ventana.
    pub fn merge(&mut self, other: &Self) {
        if other.window_ms != self.window_ms {
            // Ventanas distintas no son fusionables sin reinterpretar; se
            // ignoran para no corromper el resultado.
            return;
        }
        for (start, bucket) in &other.buckets {
            let target = self.buckets.entry(*start).or_insert(Bucket {
                sum: 0.0,
                count: 0,
                min: bucket.min,
                max: bucket.max,
            });
            target.sum += bucket.sum;
            target.count += bucket.count;
            target.min = target.min.min(bucket.min);
            target.max = target.max.max(bucket.max);
        }
    }

    /// Numero de puntos agregados.
    ///
    /// Returns:
    ///     Suma de `count`.
    pub fn point_count(&self) -> u64 {
        self.buckets.values().map(|bucket| bucket.count).sum()
    }

    /// Numero de buckets.
    ///
    /// Returns:
    ///     Cardinalidad.
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// Instantanea `(inicio, media)` ordenada por tiempo.
    ///
    /// Returns:
    ///     Pares `(bucket_start, avg)`.
    pub fn snapshot(&self) -> Vec<(i64, f64)> {
        self.buckets
            .iter()
            .map(|(&start, bucket)| (start, bucket.avg()))
            .collect()
    }

    /// Instantanea completa por bucket (sum/count/min/max).
    ///
    /// Returns:
    ///     Pares `(bucket_start, bucket)`.
    pub fn snapshot_buckets(&self) -> Vec<(i64, Bucket)> {
        self.buckets
            .iter()
            .map(|(&start, bucket)| (start, *bucket))
            .collect()
    }

    /// Agrega un lote desde cero (compatibilidad con la version batch).
    ///
    /// Args:
    ///     points: Pares `(timestamp_ms, valor)`.
    ///
    /// Returns:
    ///     Pares `(inicio, media)`.
    pub fn compute(&self, points: &[(i64, f64)]) -> Vec<(i64, f64)> {
        let mut aggregate = Self::new(self.name.clone(), self.window_ms);
        for &(ts, value) in points {
            aggregate.add_point(ts, value);
        }
        aggregate.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::ContinuousAggregate;

    #[test] // @spec AC-0145-01
    fn test_ac_0145_01_continuous_aggregate() {
        let aggregate = ContinuousAggregate::new("avg_1m", 60_000);
        let points = vec![(0, 1.0), (1_000, 2.0), (60_000, 3.0), (61_000, 4.0)];
        let result = aggregate.compute(&points);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], (0, 1.5));
        assert_eq!(result[1], (60_000, 3.5));
    }

    /// Incremental == batch (misma media, sin recalcular todo).
    #[test]
    fn incremental_matches_batch() {
        let points: Vec<(i64, f64)> = (0..1_000)
            .map(|index| (index * 500, (index % 7) as f64))
            .collect();
        let mut incremental = ContinuousAggregate::new("a", 10_000);
        for &(ts, value) in &points {
            incremental.add_point(ts, value);
        }
        let batch = ContinuousAggregate::new("a", 10_000).compute(&points);
        assert_eq!(incremental.snapshot(), batch);
        assert_eq!(incremental.point_count(), points.len() as u64);

        // Min/max por bucket.
        let mut stats = ContinuousAggregate::new("s", 60_000);
        stats.add_point(0, 5.0);
        stats.add_point(1_000, -2.0);
        stats.add_point(2_000, 10.0);
        let buckets = stats.snapshot_buckets();
        assert_eq!(buckets[0].1.count, 3);
        assert_eq!(buckets[0].1.min, -2.0);
        assert_eq!(buckets[0].1.max, 10.0);
    }

    /// `merge` es asociativo y conmutativo (agregacion por particiones).
    #[test]
    fn merge_is_associative_and_commutative() {
        let mut left = ContinuousAggregate::new("m", 1_000);
        let mut right = ContinuousAggregate::new("m", 1_000);
        for ts in 0..100 {
            left.add_point(ts * 100, 1.0);
        }
        for ts in 50..150 {
            right.add_point(ts * 100, 2.0);
        }
        let mut a = ContinuousAggregate::new("m", 1_000);
        a.merge(&left);
        a.merge(&right);
        let mut b = ContinuousAggregate::new("m", 1_000);
        b.merge(&right);
        b.merge(&left);
        assert_eq!(a.snapshot(), b.snapshot());
        assert_eq!(a.point_count(), 200);

        // Ventanas distintas no se fusionan.
        let mut other = ContinuousAggregate::new("o", 2_000);
        other.add_point(0, 1.0);
        a.merge(&other);
        assert_eq!(a.point_count(), 200);
    }

    /// Timestamps negativos y ventana cero.
    #[test]
    fn negative_timestamps_and_zero_window() {
        let mut aggregate = ContinuousAggregate::new("n", 1_000);
        assert_eq!(aggregate.bucket_start(-1), -1_000);
        assert_eq!(aggregate.bucket_start(-1_001), -2_000);
        aggregate.add_point(-1, 1.0);
        assert_eq!(aggregate.snapshot()[0].0, -1_000);

        let mut zero = ContinuousAggregate::new("z", 0);
        assert_eq!(zero.bucket_start(42), 42);
        zero.add_point(42, 7.0);
        assert_eq!(zero.snapshot(), vec![(42, 7.0)]);
    }
}
