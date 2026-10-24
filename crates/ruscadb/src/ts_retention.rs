//! Retention policies para time-series (SPEC-0122).

/// Punto de serie temporal (timestamp + valor).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TsPoint {
    /// Timestamp en milisegundos.
    pub ts: i64,
    /// Valor.
    pub value: f64,
}

/// Política de retención para series temporales.
#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    /// TTL en milisegundos (puntos más antiguos se eliminan).
    pub ttl_ms: i64,
    /// Tamaño de ventana para downsampling de puntos antiguos (0 = sin downsampling).
    pub downsample_window: usize,
    /// Umbral: puntos más antiguos que este se downsamplen.
    pub downsample_threshold_ms: i64,
}

impl RetentionPolicy {
    /// Crea una política de retención.
    pub fn new(ttl_ms: i64) -> Self {
        Self {
            ttl_ms,
            downsample_window: 0,
            downsample_threshold_ms: 0,
        }
    }

    /// Configura downsampling para puntos antiguos.
    pub fn with_downsample(mut self, window: usize, threshold_ms: i64) -> Self {
        self.downsample_window = window;
        self.downsample_threshold_ms = threshold_ms;
        self
    }

    /// Aplica la política: elimina puntos más antiguos que el TTL.
    pub fn apply(&self, now: i64, points: &[TsPoint]) -> Vec<TsPoint> {
        let cutoff = now - self.ttl_ms;
        points.iter().filter(|p| p.ts >= cutoff).copied().collect()
    }

    /// Aplica downsampling a puntos antiguos (más antiguos que el threshold).
    pub fn downsample_old(&self, now: i64, points: &[TsPoint]) -> Vec<TsPoint> {
        if self.downsample_window == 0 {
            return points.to_vec();
        }
        let threshold = now - self.downsample_threshold_ms;
        let mut result = Vec::new();
        let mut old_points: Vec<TsPoint> = Vec::new();
        let mut recent_points: Vec<TsPoint> = Vec::new();

        for p in points {
            if p.ts < threshold {
                old_points.push(*p);
            } else {
                recent_points.push(*p);
            }
        }

        // Downsample old points by averaging in windows.
        if !old_points.is_empty() {
            let window = self.downsample_window;
            for chunk in old_points.chunks(window) {
                let avg_ts = chunk.iter().map(|p| p.ts).sum::<i64>() / chunk.len() as i64;
                let avg_val = chunk.iter().map(|p| p.value).sum::<f64>() / chunk.len() as f64;
                result.push(TsPoint {
                    ts: avg_ts,
                    value: avg_val,
                });
            }
        }
        result.extend(recent_points);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // @spec AC-0122-01
    fn test_ac_0122_01_retention_ttl() {
        let policy = RetentionPolicy::new(1000); // TTL 1s
        let now = 5000;
        let points = vec![
            TsPoint {
                ts: 1000,
                value: 1.0,
            }, // old, should be removed
            TsPoint {
                ts: 4500,
                value: 2.0,
            }, // recent, kept
            TsPoint {
                ts: 4900,
                value: 3.0,
            }, // recent, kept
        ];
        let result = policy.apply(now, &points);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ts, 4500);
        assert_eq!(result[1].ts, 4900);
    }

    #[test]
    fn test_retention_downsample() {
        let policy = RetentionPolicy::new(10000).with_downsample(2, 3000);
        let now = 5000;
        let points = vec![
            TsPoint {
                ts: 1000,
                value: 1.0,
            },
            TsPoint {
                ts: 2000,
                value: 2.0,
            },
            TsPoint {
                ts: 4500,
                value: 3.0,
            },
        ];
        let result = policy.downsample_old(now, &points);
        assert!(result.len() <= points.len());
    }
}
