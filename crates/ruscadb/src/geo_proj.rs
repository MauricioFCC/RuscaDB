#![allow(missing_docs)]
#![allow(dead_code)]
//! Proyecciones geográficas (SPEC-0140): Haversine.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

impl GeoPoint {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
    pub fn haversine(&self, other: &Self) -> f64 {
        let r = 6371000.0;
        let lat1 = self.lat.to_radians();
        let lat2 = other.lat.to_radians();
        let dlat = (other.lat - self.lat).to_radians();
        let dlon = (other.lon - self.lon).to_radians();
        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
        r * c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0140-01
    fn test_ac_0140_01_haversine_distance() {
        let a = GeoPoint::new(40.7128, -74.0060);
        let b = GeoPoint::new(51.5074, -0.1278);
        let dist = a.haversine(&b);
        assert!((dist - 5570000.0).abs() < 100000.0);
    }
}
