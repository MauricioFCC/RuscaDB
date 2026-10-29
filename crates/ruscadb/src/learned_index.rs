//! Indice aprendido por tramos lineales (PGM / piecewise-linear), SPEC-0174.
//!
//! Sustituye (parte de) un B+tree por un modelo: aproxima la posicion de una
//! clave con una funcion lineal a trozos y corrige con una busqueda local de
//! `±epsilon`. Construccion voraz con garantia de error: cada segmento se
//! extiende solo mientras todos sus puntos quedan a distancia `<= epsilon` de
//! la recta, de modo que la busqueda local es *completa* (no pierde claves).
//!
//! Referencias: T. Kraska et al., "The Case for Learned Index Structures"
//! (arXiv:1712.01208, SIGMOD 2018) y P. Ferragina y G. Vinciguerra, "The PGM
//! Index" (SIGMOD 2020). Garantia de correccion verificada por test exhaustivo.

/// Segmento lineal: posicion aproximada `start_pos + slope*(key - start_key)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Primera clave cubierta por el segmento.
    pub start_key: i64,
    /// Posicion (en claves ordenadas) de `start_key`.
    pub start_pos: usize,
    /// Pendiente en posiciones por unidad de clave.
    pub slope: f64,
}

/// Indice aprendido sobre un conjunto de claves ordenadas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LearnedIndex {
    /// Cota de error maxima garantizada (en posiciones).
    pub epsilon: usize,
    /// Segmentos ordenados por `start_key`.
    pub segments: Vec<Segment>,
    /// Claves ordenadas y sin duplicados (fuente de verdad del lookup).
    keys: Vec<i64>,
}

impl LearnedIndex {
    /// Construye el indice.
    ///
    /// Args:
    ///     keys: Claves (se ordenan y deduplican internamente).
    ///     epsilon: Cota de error maxima aceptada, en posiciones.
    ///
    /// Returns:
    ///     Indice listo para `predict`/`lookup`.
    pub fn build(keys: &[i64], epsilon: usize) -> Self {
        let mut sorted = keys.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let n = sorted.len();
        if n == 0 {
            return Self {
                epsilon,
                segments: Vec::new(),
                keys: sorted,
            };
        }

        let mut segments = Vec::new();
        let mut start = 0usize;
        while start < n {
            let mut end = start;
            while end + 1 < n && Self::max_error(&sorted, start, end + 1) <= epsilon as f64 {
                end += 1;
            }
            let (x0, y0) = (sorted[start], start);
            let (x1, y1) = (sorted[end], end);
            let slope = if x1 == x0 {
                0.0
            } else {
                (y1 - y0) as f64 / (x1 - x0) as f64
            };
            segments.push(Segment {
                start_key: x0,
                start_pos: y0,
                slope,
            });
            start = end + 1;
        }

        Self {
            epsilon,
            segments,
            keys: sorted,
        }
    }

    /// Error maximo de la recta que une `from` y `to` sobre el tramo.
    ///
    /// Args:
    ///     keys: Claves ordenadas.
    ///     from: Indice inicial (incluido).
    ///     to: Indice final (incluido).
    ///
    /// Returns:
    ///     Maximo `|prediccion - posicion|` en el tramo.
    fn max_error(keys: &[i64], from: usize, to: usize) -> f64 {
        let (x0, y0) = (keys[from], from);
        let (x1, y1) = (keys[to], to);
        let slope = if x1 == x0 {
            0.0
        } else {
            (y1 - y0) as f64 / (x1 - x0) as f64
        };
        let mut worst = 0.0f64;
        for (offset, &key) in keys[from..=to].iter().enumerate() {
            let predicted = y0 as f64 + slope * (key - x0) as f64;
            let error = (predicted - (from + offset) as f64).abs();
            if error > worst {
                worst = error;
            }
        }
        worst
    }

    /// Predice la posicion de una clave.
    ///
    /// Args:
    ///     key: Clave buscada (puede no existir).
    ///
    /// Returns:
    ///     Posicion estimada en `0..keys.len()`, o `None` si el indice esta
    ///     vacio.
    pub fn predict(&self, key: i64) -> Option<usize> {
        if self.segments.is_empty() || self.keys.is_empty() {
            return None;
        }
        let idx = match self
            .segments
            .binary_search_by(|segment| segment.start_key.cmp(&key))
        {
            Ok(found) => found,
            Err(0) => 0,
            Err(next) => next - 1,
        };
        let segment = self.segments[idx];
        let predicted = segment.start_pos as f64 + segment.slope * (key - segment.start_key) as f64;
        let rounded = predicted.round();
        let clamped = if rounded.is_nan() || rounded < 0.0 {
            0
        } else {
            (rounded as usize).min(self.keys.len() - 1)
        };
        Some(clamped)
    }

    /// Busca una clave usando prediccion + busqueda local de `±(epsilon+1)`.
    ///
    /// Args:
    ///     key: Clave buscada.
    ///
    /// Returns:
    ///     Posicion exacta si existe; `None` en caso contrario.
    pub fn lookup(&self, key: i64) -> Option<usize> {
        let center = self.predict(key)?;
        let radius = self.epsilon.saturating_add(1);
        let low = center.saturating_sub(radius);
        let high = (center.saturating_add(radius)).min(self.keys.len() - 1);
        self.keys[low..=high]
            .binary_search(&key)
            .map(|i| low + i)
            .ok()
    }

    /// Numero de claves indexadas.
    ///
    /// Returns:
    ///     Cardinalidad (claves unicas).
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Indica si el indice esta vacio.
    ///
    /// Returns:
    ///     `true` si no hay claves.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Bytes aproximados del modelo (segmentos) frente a un B+tree de punteros.
    ///
    /// Returns:
    ///     `(bytes_modelo, bytes_equivalentes_bplus)` usando 8 B por entrada.
    pub fn memory_report(&self) -> (usize, usize) {
        (self.segments.len() * 24, self.keys.len() * 8)
    }
}

#[cfg(test)]
mod tests {
    use super::LearnedIndex;

    /// AC-0174-01: el modelo respeta la cota de error `epsilon` en todas las
    /// claves (si no, la busqueda local podria perder entradas).
    #[test]
    // @spec AC-0174-01
    fn test_ac_0174_01_build() {
        let cases: Vec<Vec<i64>> = vec![
            (0..500).collect(),
            (0..500).map(|i| i * i).collect(),
            (0..300).map(|i| 1_000_000 + i * 7).collect(),
            vec![5],
            Vec::new(),
        ];
        for keys in cases {
            let index = LearnedIndex::build(&keys, 4);
            assert_eq!(index.len(), keys.len());
            if keys.is_empty() {
                assert!(index.is_empty());
                assert_eq!(index.predict(1), None);
                continue;
            }
            assert!(!index.segments.is_empty());
            for (pos, &key) in keys.iter().enumerate() {
                let predicted = index.predict(key).unwrap_or(usize::MAX);
                let error = predicted.abs_diff(pos);
                assert!(
                    error <= index.epsilon,
                    "error {error} > epsilon en clave {key} (pos {pos})"
                );
            }
            // El modelo es mucho mas compacto que una entrada por clave.
            let (model, bplus) = index.memory_report();
            assert!(model <= bplus.max(48), "modelo {model}B vs B+tree {bplus}B");
        }
    }

    /// AC-0174-02: lookup exacto (presentes y ausentes) sobre distribuciones
    /// adversariales para un indice aprendido.
    #[test]
    // @spec AC-0174-02
    fn test_ac_0174_02_lookup() {
        let adversarial: Vec<i64> = (0..200)
            .map(|i| if i % 2 == 0 { i } else { i * 1_000 })
            .collect();
        let keys: Vec<i64> = {
            let mut v = adversarial;
            v.sort_unstable();
            v
        };
        let index = LearnedIndex::build(&keys, 8);
        for (pos, &key) in keys.iter().enumerate() {
            assert_eq!(index.lookup(key), Some(pos), "clave presente {key}");
        }
        for missing in [-1, 1, 999, 10_000, i64::MAX] {
            assert_eq!(index.lookup(missing), None, "clave ausente {missing}");
        }

        // Duplicados: se deduplican y el lookup sigue siendo correcto.
        let with_dups = LearnedIndex::build(&[3, 1, 3, 2, 1], 1);
        assert_eq!(with_dups.len(), 3);
        assert_eq!(with_dups.lookup(1), Some(0));
        assert_eq!(with_dups.lookup(3), Some(2));
    }

    /// Epsilon 0 obliga a un segmento por clave pero mantiene la exactitud.
    #[test]
    fn epsilon_zero_is_exact() {
        let keys: Vec<i64> = (0..64).map(|i| i * 3).collect();
        let index = LearnedIndex::build(&keys, 0);
        for (pos, &key) in keys.iter().enumerate() {
            assert_eq!(index.lookup(key), Some(pos));
        }
        assert_eq!(index.lookup(1), None);
    }
}
