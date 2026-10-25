//! Tests de aceptación de la feature `geo-proj` (SPEC-0140).

#![allow(clippy::expect_used)]

use ruscadb::GeoPoint;

#[test] // @spec AC-0140-01
fn test_ac_0140_01_haversine_distance() {
    let a = GeoPoint::new(40.7128, -74.0060);
    let b = GeoPoint::new(51.5074, -0.1278);
    let dist = a.haversine(&b);
    assert!((dist - 5570000.0).abs() < 100000.0);
}
