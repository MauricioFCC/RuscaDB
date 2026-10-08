//! # ruscadb-ts
//!
//! **Serie temporal** de RuscaDB: bucketing por intervalo, ventanas
//! deslizantes/tumbling con agregados (`count/sum/min/max/avg`) y remuestreo.
//! Especificación: `specs/time_series.md` (SPEC-0029).
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §5.2 (modelo time-series sobre
//! `TimestampMillis` de `ScalarValue`).
//!
//! Complejidades:
//! - bucketing: O(1) por punto.
//! - ventanas: O(n log n) por la ordenación (+ barrido lineal por ventana).
//! - remuestreo: O(n log n) por la ordenación + O(n + m) en la rejilla
//!   (`n` puntos, `m` ranuras).

#![forbid(unsafe_code)]

use ruscadb_core::RuscaError;
use serde::{Deserialize, Serialize};

/// Punto de una serie temporal: instante en milisegundos y valor asociado.
///
/// El orden total es por `ts_ms` (los empates se desempatan por `value` con
/// orden total) para que toda operación sea determinista ante permutaciones.
/// La igualdad sigue siendo por campos. Es serializable con `serde`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    /// Instante en milisegundos desde la época Unix (puede ser negativo).
    pub ts_ms: i64,
    /// Valor observado en ese instante.
    pub value: f64,
}

/// Agregado aplicable a los puntos de una ventana.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Agg {
    /// Número de puntos de la ventana (como `f64`).
    Count,
    /// Suma de los valores.
    Sum,
    /// Mínimo de los valores.
    Min,
    /// Máximo de los valores.
    Max,
    /// Media aritmética de los valores.
    Avg,
}

/// Ventana temporal `[start, end)` con su agregado ya calculado.
///
/// `value` es `None` si la ventana no contiene ningún punto.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// Inicio inclusivo de la ventana (ms).
    pub start: i64,
    /// Fin exclusivo de la ventana (ms).
    pub end: i64,
    /// Agregado de la ventana o `None` si está vacía.
    pub value: Option<f64>,
}

/// Estrategia de relleno para los huecos del remuestreo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fill {
    /// Repite el último valor observado (la primera ranura queda en `None`).
    Previous,
    /// Deja `None` en todo hueco sin observación exacta.
    Null,
}

/// Redondea un instante al inicio de su cubo (piso, correcto en negativos).
///
/// Complejidad: O(1) por punto.
///
/// Args:
///     ts_ms: Instante en milisegundos (puede ser negativo).
///     bucket_ms: Tamaño del cubo en milisegundos (debe ser > 0).
///
/// Returns:
///     Inicio del cubo (`ts_ms.div_euclid(bucket_ms) * bucket_ms`).
///
/// Raises:
///     RuscaError::InvalidConfig: Si `bucket_ms <= 0` o el inicio
///         desborda `i64`.
pub fn time_bucket(ts_ms: i64, bucket_ms: i64) -> Result<i64, RuscaError> {
    if bucket_ms <= 0 {
        return Err(RuscaError::InvalidConfig(format!(
            "bucket_ms debe ser > 0 (recibido {bucket_ms}; revisa el tamaño de cubo en time_bucket())"
        )));
    }
    ts_ms
        .div_euclid(bucket_ms)
        .checked_mul(bucket_ms)
        .ok_or_else(|| {
            RuscaError::InvalidConfig(format!(
                "el inicio del cubo de ts_ms={ts_ms} con bucket_ms={bucket_ms} desborda i64"
            ))
        })
}

/// Agrega los puntos en ventanas `[ancla + k·step, ancla + k·step + window)`.
///
/// La copia de puntos se ordena por `ts_ms` (determinista). Tumbling si
/// `step_ms == window_ms`; deslizante si `step_ms < window_ms`. El ancla es
/// el `ts_ms` mínimo. Cada ventana vacía produce `value: None`.
///
/// Complejidad: O(n log n) por la ordenación (+ barrido lineal por ventana).
///
/// Args:
///     points: Puntos de la serie (cualquier orden).
///     window_ms: Tamaño de la ventana en ms (debe ser > 0).
///     step_ms: Paso entre ventanas en ms (debe cumplir `0 < step <= window`).
///     agg: Agregado a calcular por ventana.
///
/// Returns:
///     Ventanas desde el mínimo hasta cubrir el máximo (vacío si no hay puntos).
///
/// Raises:
///     RuscaError::InvalidConfig: Si `window_ms <= 0`, `step_ms <= 0`
///         o `step_ms > window_ms`.
pub fn window(
    points: &[SeriesPoint],
    window_ms: i64,
    step_ms: i64,
    agg: Agg,
) -> Result<Vec<Window>, RuscaError> {
    validate_window_args(window_ms, step_ms)?;
    if points.is_empty() {
        return Ok(Vec::new());
    }
    let sorted = sorted_points(points);
    let last = sorted[sorted.len() - 1].ts_ms;
    let mut out = Vec::new();
    let mut start = sorted[0].ts_ms;
    while start <= last {
        let end = start.saturating_add(window_ms);
        let value = aggregate_in(&sorted, start, end, agg);
        out.push(Window { start, end, value });
        let next = start.saturating_add(step_ms);
        if next <= start {
            break;
        }
        start = next;
    }
    Ok(out)
}

/// Remuestrea la serie sobre la rejilla `[start, end)` con paso `step`.
///
/// Con `Fill::Previous` cada ranura toma el último valor con `ts <= ranura`
/// (la primera queda `None` si no hay nada previo); con `Fill::Null` solo
/// las ranuras con observación exacta (`ts == ranura`) tienen valor.
///
/// Complejidad: O(n log n) por la ordenación + O(n + m) en la rejilla.
///
/// Args:
///     points: Puntos de la serie (cualquier orden).
///     start: Inicio inclusivo de la rejilla (ms).
///     end: Fin exclusivo de la rejilla (ms, debe ser > `start`).
///     step: Paso de la rejilla en ms (debe ser > 0).
///     fill: Estrategia de relleno de huecos.
///
/// Returns:
///     Un valor por ranura (`Some(v)` o `None` según `fill`).
///
/// Raises:
///     RuscaError::InvalidConfig: Si `step <= 0` o `end <= start`.
pub fn resample(
    points: &[SeriesPoint],
    start: i64,
    end: i64,
    step: i64,
    fill: Fill,
) -> Result<Vec<Option<f64>>, RuscaError> {
    if step <= 0 {
        return Err(RuscaError::InvalidConfig(format!(
            "step debe ser > 0 (recibido {step}; revisa el paso de rejilla en resample())"
        )));
    }
    if end <= start {
        return Err(RuscaError::InvalidConfig(format!(
            "se requiere end > start (recibido start={start}, end={end} en resample())"
        )));
    }
    let sorted = sorted_points(points);
    let mut out = Vec::new();
    let mut idx = 0usize;
    let mut carry: Option<f64> = None;
    let mut slot = start;
    while slot < end {
        let mut exact: Option<f64> = None;
        while idx < sorted.len() && sorted[idx].ts_ms <= slot {
            carry = Some(sorted[idx].value);
            if sorted[idx].ts_ms == slot {
                exact = Some(sorted[idx].value);
            }
            idx += 1;
        }
        out.push(match fill {
            Fill::Previous => carry,
            Fill::Null => exact,
        });
        let next = slot.saturating_add(step);
        if next <= slot {
            break;
        }
        slot = next;
    }
    Ok(out)
}

/// Valida los tamaños de ventana y paso antes de agregar.
///
/// Args:
///     window_ms: Tamaño de la ventana en ms.
///     step_ms: Paso entre ventanas en ms.
///
/// Returns:
///     `Ok(())` si `window_ms > 0` y `0 < step_ms <= window_ms`.
///
/// Raises:
///     RuscaError::InvalidConfig: Si algún parámetro viola su rango.
fn validate_window_args(window_ms: i64, step_ms: i64) -> Result<(), RuscaError> {
    if window_ms <= 0 {
        return Err(RuscaError::InvalidConfig(format!(
            "window_ms debe ser > 0 (recibido {window_ms}; revisa la ventana en window())"
        )));
    }
    if step_ms <= 0 {
        return Err(RuscaError::InvalidConfig(format!(
            "step_ms debe ser > 0 (recibido {step_ms}; revisa el paso en window())"
        )));
    }
    if step_ms > window_ms {
        return Err(RuscaError::InvalidConfig(format!(
            "step_ms ({step_ms}) no puede superar window_ms ({window_ms} en window())"
        )));
    }
    Ok(())
}

/// Devuelve una copia de los puntos en orden total determinista.
///
/// Args:
///     points: Puntos de la serie (cualquier orden).
///
/// Returns:
///     Copia ordenada por `ts_ms` (empates por `value` con orden total).
fn sorted_points(points: &[SeriesPoint]) -> Vec<SeriesPoint> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| {
        a.ts_ms
            .cmp(&b.ts_ms)
            .then_with(|| a.value.total_cmp(&b.value))
    });
    sorted
}

/// Calcula el agregado de los puntos en `[start, end)`.
///
/// Args:
///     sorted: Puntos ya ordenados por `ts_ms`.
///     start: Inicio inclusivo del intervalo (ms).
///     end: Fin exclusivo del intervalo (ms).
///     agg: Agregado a calcular.
///
/// Returns:
///     El agregado o `None` si el intervalo está vacío.
fn aggregate_in(sorted: &[SeriesPoint], start: i64, end: i64, agg: Agg) -> Option<f64> {
    let mut count = 0_u64;
    let mut sum = 0.0_f64;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for point in sorted {
        if point.ts_ms >= start && point.ts_ms < end {
            count += 1;
            sum += point.value;
            min = min.min(point.value);
            max = max.max(point.value);
        }
    }
    apply_agg(count, sum, min, max, agg)
}

/// Materializa el agregado desde sus acumuladores.
///
/// Args:
///     count: Número de puntos del intervalo.
///     sum: Suma de los valores.
///     min: Mínimo observado (`INFINITY` si vacío).
///     max: Máximo observado (`NEG_INFINITY` si vacío).
///     agg: Agregado a materializar.
///
/// Returns:
///     El agregado o `None` si `count == 0`.
fn apply_agg(count: u64, sum: f64, min: f64, max: f64, agg: Agg) -> Option<f64> {
    if count == 0 {
        return None;
    }
    let value = match agg {
        Agg::Count => count as f64,
        Agg::Sum => sum,
        Agg::Min => min,
        Agg::Max => max,
        Agg::Avg => sum / count as f64,
    };
    Some(value)
}

/// Percentil con interpolación lineal sobre los valores de la serie.
///
/// Ordena los valores de forma ascendente (`f64::total_cmp`) y aplica
/// `idx = (p / 100) * (n - 1)`, interpolando linealmente entre las
/// posiciones `floor(idx)` y `ceil(idx)`. Complejidad O(n log n).
///
/// Args:
///     points: Puntos de la serie (cualquier orden).
///     p: Percentil deseado, en el rango `[0, 100]`.
///
/// Returns:
///     El percentil, o `None` si la serie está vacía.
///
/// Raises:
///     RuscaError::InvalidConfig: Si `p` no está en `[0, 100]` o no es finito.
pub fn percentile(points: &[SeriesPoint], p: f64) -> Result<Option<f64>, RuscaError> {
    if !p.is_finite() || !(0.0..=100.0).contains(&p) {
        return Err(RuscaError::InvalidConfig(format!(
            "p debe estar en [0, 100] (recibido {p}; revisa el percentil en percentile())"
        )));
    }
    if points.is_empty() {
        return Ok(None);
    }
    let mut values: Vec<f64> = points.iter().map(|point| point.value).collect();
    values.sort_by(f64::total_cmp);
    let rank = (p / 100.0) * (values.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    let fraction = rank - lower as f64;
    Ok(Some(
        values[lower] + (values[upper] - values[lower]) * fraction,
    ))
}

/// Tasa de cambio por segundo entre puntos consecutivos.
///
/// Ordena los puntos por `ts_ms` (determinista) y calcula
/// `(v2 - v1) / ((t2 - t1) / 1000)` para cada par adyacente. Los pares con
/// `dt == 0` se omiten. La clave del resultado es `t2`.
///
/// Args:
///     points: Puntos de la serie (cualquier orden).
///
/// Returns:
///     Pares `(t2, tasa_por_segundo)`; vacío si hay menos de 2 puntos.
pub fn rate(points: &[SeriesPoint]) -> Vec<(i64, f64)> {
    let sorted = sorted_points(points);
    let mut out = Vec::new();
    for pair in sorted.windows(2) {
        let previous = pair[0];
        let current = pair[1];
        let delta_ms = current.ts_ms.saturating_sub(previous.ts_ms);
        if delta_ms == 0 {
            continue;
        }
        let seconds = delta_ms as f64 / 1000.0;
        out.push((current.ts_ms, (current.value - previous.value) / seconds));
    }
    out
}

/// Media móvil simple sobre ventanas de `window` puntos consecutivos.
///
/// Ordena los puntos por `ts_ms` y recorre todas las ventanas de `window`
/// puntos. Si `window` supera el número de puntos, se usa `n` (una única
/// ventana con todos los puntos) para no devolver un resultado vacío.
/// Cada salida es `(ts_ms del último punto de la ventana, media)`.
///
/// Args:
///     points: Puntos de la serie (cualquier orden).
///     window: Número de puntos por ventana (debe ser > 0).
///
/// Returns:
///     `n - window + 1` medias móviles (o una sola si `window >= n`); vacío si
///     la serie está vacía.
///
/// Raises:
///     RuscaError::InvalidConfig: Si `window == 0`.
pub fn moving_average(
    points: &[SeriesPoint],
    window: usize,
) -> Result<Vec<(i64, f64)>, RuscaError> {
    if window == 0 {
        return Err(RuscaError::InvalidConfig(format!(
            "window debe ser > 0 (recibido {window}; revisa la ventana en moving_average())"
        )));
    }
    let sorted = sorted_points(points);
    let effective = window.min(sorted.len());
    if effective == 0 {
        return Ok(Vec::new());
    }
    if effective == 1 {
        // Sin agregación: cada punto es su propia media (identidad exacta).
        return Ok(sorted
            .iter()
            .map(|point| (point.ts_ms, point.value))
            .collect());
    }
    let mut sum: f64 = sorted[..effective].iter().map(|point| point.value).sum();
    let mut out = Vec::with_capacity(sorted.len() - effective + 1);
    out.push((sorted[effective - 1].ts_ms, sum / effective as f64));
    for (idx, current) in sorted.iter().enumerate().skip(effective) {
        sum += current.value - sorted[idx - effective].value;
        out.push((current.ts_ms, sum / effective as f64));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    /// Milisegundos de una hora (bote de referencia en los tests).
    const HOUR_MS: i64 = 3_600_000;

    /// Construye un punto sin ruido sintáctico en los tests.
    ///
    /// Args:
    ///     ts_ms: Instante en milisegundos.
    ///     value: Valor observado.
    ///
    /// Returns:
    ///     El `SeriesPoint` correspondiente.
    fn point(ts_ms: i64, value: f64) -> SeriesPoint {
        SeriesPoint { ts_ms, value }
    }

    /// Extrae solo los valores de una lista de ventanas.
    ///
    /// Args:
    ///     windows: Ventanas ya calculadas.
    ///
    /// Returns:
    ///     Sus agregados (`Some`/`None`) en orden.
    fn values_of(windows: &[Window]) -> Vec<Option<f64>> {
        windows.iter().map(|window| window.value).collect()
    }

    /// AC-0029-01 — cada ts cae en el inicio de su cubo (bote de 1h).
    #[test]
    fn test_ac_0029_01_time_bucket_floors() {
        // BVA: ts = 0, frontera exacta, interior, negativos y bote = 1.
        if let (Ok(zero), Ok(exact), Ok(inside)) = (
            time_bucket(0, HOUR_MS),
            time_bucket(HOUR_MS, HOUR_MS),
            time_bucket(HOUR_MS + 1, HOUR_MS),
        ) {
            assert_eq!(zero, 0);
            assert_eq!(exact, HOUR_MS);
            assert_eq!(inside, HOUR_MS);
        } else {
            panic!("cubos válidos sobre ts >= 0 fueron rechazados");
        }
        // Piso correcto en negativos (división euclidiana, no truncado).
        if let (Ok(neg_one), Ok(neg_exact), Ok(neg_inside)) = (
            time_bucket(-1, HOUR_MS),
            time_bucket(-HOUR_MS, HOUR_MS),
            time_bucket(-HOUR_MS - 1, HOUR_MS),
        ) {
            assert_eq!(neg_one, -HOUR_MS);
            assert_eq!(neg_exact, -HOUR_MS);
            assert_eq!(neg_inside, -2 * HOUR_MS);
        } else {
            panic!("cubos válidos sobre ts negativos fueron rechazados");
        }
        // BVA: bote = 1 es identidad; bote mayor que el rango ancla en cero.
        if let (Ok(identity), Ok(neg_identity), Ok(wide), Ok(neg_wide)) = (
            time_bucket(123, 1),
            time_bucket(-5, 1),
            time_bucket(5, 100),
            time_bucket(-5, 100),
        ) {
            assert_eq!(identity, 123);
            assert_eq!(neg_identity, -5);
            assert_eq!(wide, 0);
            assert_eq!(neg_wide, -100);
        } else {
            panic!("cubos límite (bote 1 o ancho) fueron rechazados");
        }
    }

    /// AC-0029-02 — los agregados coinciden con el cálculo manual.
    #[test]
    fn test_ac_0029_02_window_aggregates() {
        let points = vec![point(0, 1.0), point(1, 2.0), point(2, 3.0), point(3, 4.0)];
        // Tumbling: una sola ventana [0, 4) con los cuatro puntos.
        if let (Ok(count), Ok(sum), Ok(min), Ok(max), Ok(avg)) = (
            window(&points, 4, 4, Agg::Count),
            window(&points, 4, 4, Agg::Sum),
            window(&points, 4, 4, Agg::Min),
            window(&points, 4, 4, Agg::Max),
            window(&points, 4, 4, Agg::Avg),
        ) {
            assert_eq!(values_of(&count), vec![Some(4.0)]);
            assert_eq!(values_of(&sum), vec![Some(10.0)]);
            assert_eq!(values_of(&min), vec![Some(1.0)]);
            assert_eq!(values_of(&max), vec![Some(4.0)]);
            assert_eq!(values_of(&avg), vec![Some(2.5)]);
        } else {
            panic!("ventanas tumbling válidas fueron rechazadas");
        }
        // Deslizante: ventanas [0,2), [1,3), [2,4), [3,5) con suma manual.
        if let Ok(sliding) = window(&points, 2, 1, Agg::Sum) {
            assert_eq!(
                values_of(&sliding),
                vec![Some(3.0), Some(5.0), Some(7.0), Some(4.0)]
            );
        } else {
            panic!("ventanas deslizantes válidas fueron rechazadas");
        }
        // Ventana vacía intermedia produce `None` sin panics.
        let sparse = vec![point(0, 1.0), point(100, 2.0)];
        if let Ok(gaps) = window(&sparse, 10, 10, Agg::Sum) {
            assert_eq!(gaps.len(), 11);
            assert_eq!(gaps[0].value, Some(1.0));
            assert_eq!(gaps[10].value, Some(2.0));
            assert!(gaps[1..10].iter().all(|window| window.value.is_none()));
        } else {
            panic!("ventanas con huecos fueron rechazadas");
        }
    }

    /// AC-0029-03 — los huecos se rellenan con el último valor.
    #[test]
    fn test_ac_0029_03_resample_fills() {
        let points = vec![point(0, 1.0), point(2, 3.0)];
        if let (Ok(previous), Ok(nulls)) = (
            resample(&points, 0, 4, 1, Fill::Previous),
            resample(&points, 0, 4, 1, Fill::Null),
        ) {
            assert_eq!(previous, vec![Some(1.0), Some(1.0), Some(3.0), Some(3.0)]);
            assert_eq!(nulls, vec![Some(1.0), None, Some(3.0), None]);
        } else {
            panic!("remuestreos válidos fueron rechazados");
        }
        // La primera ranura queda en `None` si no hay nada previo.
        if let Ok(late) = resample(&[point(2, 3.0)], 0, 4, 1, Fill::Previous) {
            assert_eq!(late, vec![None, None, Some(3.0), Some(3.0)]);
        } else {
            panic!("remuestreo con arranque tardío fue rechazado");
        }
    }

    /// AC-0029-04 — el orden de entrada no altera el resultado.
    #[test]
    fn test_ac_0029_04_ordering_is_irrelevant() {
        let ordered = vec![
            point(0, 1.0),
            point(5, 2.0),
            point(3, 4.0),
            point(3, 0.5),
            point(-2, 7.0),
        ];
        let mut shuffled = ordered.clone();
        shuffled.reverse();
        if let (Ok(plain), Ok(mixed)) = (
            window(&ordered, 4, 2, Agg::Avg),
            window(&shuffled, 4, 2, Agg::Avg),
        ) {
            assert_eq!(plain, mixed);
        } else {
            panic!("ventanas válidas fueron rechazadas");
        }
        if let (Ok(plain), Ok(mixed)) = (
            resample(&ordered, -2, 6, 2, Fill::Previous),
            resample(&shuffled, -2, 6, 2, Fill::Previous),
        ) {
            assert_eq!(plain, mixed);
        } else {
            panic!("remuestreos válidos fueron rechazados");
        }
    }

    /// AC-0029-05 — entradas inválidas: error accionable, sin panics.
    #[test]
    fn test_ac_0029_05_invalid_inputs_are_safe() {
        assert!(time_bucket(0, 0).is_err());
        assert!(time_bucket(0, -100).is_err());
        assert!(window(&[point(0, 1.0)], 0, 1, Agg::Sum).is_err());
        assert!(window(&[point(0, 1.0)], 4, 0, Agg::Sum).is_err());
        assert!(window(&[point(0, 1.0)], 4, -1, Agg::Sum).is_err());
        assert!(window(&[point(0, 1.0)], 2, 3, Agg::Sum).is_err());
        assert!(resample(&[], 0, 4, 0, Fill::Null).is_err());
        assert!(resample(&[], 0, 4, -2, Fill::Null).is_err());
        assert!(resample(&[], 4, 4, 1, Fill::Null).is_err());
        assert!(resample(&[], 5, 2, 1, Fill::Null).is_err());
        // Entradas límite válidas: serie vacía, sin panics ni errores.
        if let (Ok(empty_windows), Ok(empty_grid)) = (
            window(&[], 4, 4, Agg::Sum),
            resample(&[], 0, 3, 1, Fill::Previous),
        ) {
            assert!(empty_windows.is_empty());
            assert_eq!(empty_grid, vec![None, None, None]);
        } else {
            panic!("entradas vacías válidas fueron rechazadas");
        }
    }

    /// AC-0039-01 — percentil 50 (mediana) con interpolación lineal.
    #[test]
    fn test_ac_0039_01_percentile_interpolates() {
        let points = vec![
            point(0, 10.0),
            point(1, 20.0),
            point(2, 30.0),
            point(3, 40.0),
        ];
        if let (Ok(median), Ok(zero), Ok(hundred), Ok(quarter)) = (
            percentile(&points, 50.0),
            percentile(&points, 0.0),
            percentile(&points, 100.0),
            percentile(&points, 25.0),
        ) {
            assert_eq!(median, Some(25.0));
            assert_eq!(zero, Some(10.0));
            assert_eq!(hundred, Some(40.0));
            assert_eq!(quarter, Some(17.5));
        } else {
            panic!("percentiles válidos fueron rechazados");
        }
        // El percentil ordena por valor, no por instante temporal.
        let shuffled = vec![
            point(0, 40.0),
            point(1, 10.0),
            point(2, 30.0),
            point(3, 20.0),
        ];
        if let Ok(median) = percentile(&shuffled, 50.0) {
            assert_eq!(median, Some(25.0));
        } else {
            panic!("percentil válido fue rechazado");
        }
    }

    /// AC-0039-02 — tasa por segundo entre puntos consecutivos.
    #[test]
    fn test_ac_0039_02_rate_per_second() {
        let points = vec![point(0, 0.0), point(1000, 10.0), point(2000, 30.0)];
        let expected = vec![(1000, 10.0), (2000, 20.0)];
        assert_eq!(rate(&points), expected);
        // El orden de entrada no altera el resultado.
        let mut shuffled = points.clone();
        shuffled.reverse();
        assert_eq!(rate(&shuffled), expected);
        // BVA dt == 0: el par simultáneo se omite.
        let degenerate = vec![point(0, 0.0), point(0, 5.0), point(1000, 10.0)];
        assert_eq!(rate(&degenerate), vec![(1000, 5.0)]);
        // dt distinto de 1 s: distingue dividir de multiplicar por `seconds`.
        assert_eq!(rate(&[point(0, 0.0), point(500, 10.0)]), vec![(500, 20.0)]);
    }

    /// AC-0039-03 — media móvil con longitud y valores correctos.
    #[test]
    fn test_ac_0039_03_moving_average() {
        let points = vec![
            point(0, 10.0),
            point(1, 20.0),
            point(2, 30.0),
            point(3, 40.0),
        ];
        if let (Ok(two), Ok(three), Ok(four), Ok(oversized)) = (
            moving_average(&points, 2),
            moving_average(&points, 3),
            moving_average(&points, 4),
            moving_average(&points, 5),
        ) {
            assert_eq!(two, vec![(1, 15.0), (2, 25.0), (3, 35.0)]);
            assert_eq!(three, vec![(2, 20.0), (3, 30.0)]);
            assert_eq!(four, vec![(3, 25.0)]);
            // window > n se satura a n: una única ventana con todo.
            assert_eq!(oversized, vec![(3, 25.0)]);
        } else {
            panic!("medias móviles válidas fueron rechazadas");
        }
        // Orden de entrada irrelevante.
        let mut shuffled = points.clone();
        shuffled.reverse();
        if let Ok(two) = moving_average(&shuffled, 2) {
            assert_eq!(two, vec![(1, 15.0), (2, 25.0), (3, 35.0)]);
        } else {
            panic!("media móvil válida fue rechazada");
        }
    }

    /// AC-0039-04 — entradas inválidas: error accionable, sin panics.
    #[test]
    fn test_ac_0039_04_invalid_inputs() {
        let points = vec![point(0, 1.0), point(1, 2.0)];
        assert!(percentile(&points, -1.0).is_err());
        assert!(percentile(&points, 101.0).is_err());
        assert!(percentile(&points, f64::NAN).is_err());
        assert!(percentile(&points, f64::INFINITY).is_err());
        assert!(moving_average(&points, 0).is_err());
        // Fronteras válidas: sin error.
        assert!(percentile(&points, 0.0).is_ok());
        assert!(percentile(&points, 100.0).is_ok());
        assert!(moving_average(&points, 1).is_ok());
    }

    /// AC-0039-05 — series vacías o de un solo punto, sin panics.
    #[test]
    fn test_ac_0039_05_boundary_series() {
        if let (Ok(none), Ok(single_median), Ok(empty_average)) = (
            percentile(&[], 50.0),
            percentile(&[point(7, 42.0)], 90.0),
            moving_average(&[], 3),
        ) {
            assert_eq!(none, None);
            // Un único punto: el percentil es ese valor.
            assert_eq!(single_median, Some(42.0));
            assert_eq!(empty_average, Vec::<(i64, f64)>::new());
        } else {
            panic!("fronteras válidas fueron rechazadas");
        }
        // rate con menos de 2 puntos es vacío.
        assert!(rate(&[]).is_empty());
        assert!(rate(&[point(7, 42.0)]).is_empty());
        // moving_average con un punto (o ventana saturada) devuelve ese punto.
        if let (Ok(single), Ok(saturated)) = (
            moving_average(&[point(7, 42.0)], 1),
            moving_average(&[point(7, 42.0)], 5),
        ) {
            assert_eq!(single, vec![(7, 42.0)]);
            assert_eq!(saturated, vec![(7, 42.0)]);
        } else {
            panic!("media móvil de un punto fue rechazada");
        }
    }

    proptest! {
        /// El bucket es idempotente: `bucket(bucket(x)) == bucket(x)`.
        #[test]
        fn bucket_is_idempotent(
            ts_ms in -10_000_000_000i64..10_000_000_000i64,
            bucket_ms in 1i64..1_000_000_000i64,
        ) {
            if let Ok(first) = time_bucket(ts_ms, bucket_ms) {
                if let Ok(second) = time_bucket(first, bucket_ms) {
                    prop_assert_eq!(first, second);
                } else {
                    panic!("el segundo bucket falló");
                }
            } else {
                panic!("un bucket válido fue rechazado");
            }
        }

        /// Ventanas y remuestreo deterministas ante permutaciones.
        #[test]
        fn series_ops_ignore_input_order(
            raw in proptest::collection::vec(
                (0i64..1000i64, -100.0f64..100.0f64),
                0..20usize,
            ),
        ) {
            let forward: Vec<SeriesPoint> = raw
                .iter()
                .map(|(ts_ms, value)| point(*ts_ms, *value))
                .collect();
            let mut backward = forward.clone();
            backward.reverse();
            if let (Ok(first), Ok(second)) =
                (window(&forward, 10, 5, Agg::Sum), window(&backward, 10, 5, Agg::Sum))
            {
                prop_assert_eq!(first, second);
            } else {
                panic!("ventanas válidas fueron rechazadas");
            }
            if let (Ok(first), Ok(second)) = (
                resample(&forward, 0, 1000, 100, Fill::Previous),
                resample(&backward, 0, 1000, 100, Fill::Previous),
            ) {
                prop_assert_eq!(first, second);
            } else {
                panic!("remuestreos válidos fueron rechazados");
            }
        }

        /// El percentil es monótono no decreciente en `p`.
        #[test]
        fn percentile_is_monotonic_in_p(
            raw in proptest::collection::vec(
                (0i64..1000i64, -1_000_000.0f64..1_000_000.0f64),
                1..20usize,
            ),
            p1 in 0.0f64..=100.0f64,
            p2 in 0.0f64..=100.0f64,
        ) {
            let points: Vec<SeriesPoint> = raw
                .iter()
                .map(|(ts_ms, value)| point(*ts_ms, *value))
                .collect();
            let (low, high) = if p1 <= p2 { (p1, p2) } else { (p2, p1) };
            if let (Ok(Some(low_value)), Ok(Some(high_value))) =
                (percentile(&points, low), percentile(&points, high))
            {
                prop_assert!(low_value <= high_value);
            } else {
                panic!("percentiles válidos fueron rechazados");
            }
        }

        /// La media móvil de ventana 1 reproduce los puntos ordenados.
        #[test]
        fn moving_average_window_one_is_identity(
            raw in proptest::collection::vec(
                (0i64..1000i64, -1000.0f64..1000.0f64),
                0..20usize,
            ),
        ) {
            let points: Vec<SeriesPoint> = raw
                .iter()
                .map(|(ts_ms, value)| point(*ts_ms, *value))
                .collect();
            let expected: Vec<(i64, f64)> = sorted_points(&points)
                .iter()
                .map(|sorted_point| (sorted_point.ts_ms, sorted_point.value))
                .collect();
            if let Ok(actual) = moving_average(&points, 1) {
                prop_assert_eq!(actual, expected);
            } else {
                panic!("media móvil de ventana 1 fue rechazada");
            }
        }
    }
}
