//! Proyecciones geograficas (SPEC-0140): distancia Haversine.
//!
//! Distancia de gran circulo entre dos puntos `(lat, lon)` en metros, con radio
//! terrestre medio de 6 371 km.

/// Punto geografico en grados decimales.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    /// Latitud en grados (`-90..=90`).
    pub lat: f64,
    /// Longitud en grados (`-180..=180`).
    pub lon: f64,
}

impl GeoPoint {
    /// Crea un punto.
    ///
    /// Args:
    ///     lat: Latitud en grados.
    ///     lon: Longitud en grados.
    ///
    /// Returns:
    ///     Punto geografico.
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// Distancia Haversine a otro punto, en metros.
    ///
    /// Args:
    ///     other: Punto destino.
    ///
    /// Returns:
    ///     Distancia de gran circulo en metros.
    pub fn haversine(&self, other: &Self) -> f64 {
        let radius = 6_371_000.0;
        let lat1 = self.lat.to_radians();
        let lat2 = other.lat.to_radians();
        let dlat = (other.lat - self.lat).to_radians();
        let dlon = (other.lon - self.lon).to_radians();
        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
        radius * c
    }
}

#[cfg(test)]
mod tests {
    use super::GeoPoint;

    #[test] // @spec AC-0140-01
    fn test_ac_0140_01_haversine_distance() {
        let new_york = GeoPoint::new(40.7128, -74.0060);
        let london = GeoPoint::new(51.5074, -0.1278);
        let distance = new_york.haversine(&london);
        assert!((distance - 5_570_000.0).abs() < 100_000.0, "{distance}");
        // Mismo punto -> 0.
        assert!(new_york.haversine(&new_york) < 1e-6);
    }
}
