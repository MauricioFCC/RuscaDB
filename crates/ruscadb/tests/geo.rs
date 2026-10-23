//! Tests de aceptación de la feature `geo` (SPEC-0112).

#![allow(clippy::expect_used)]

use ruscadb::Point;

#[test] // @spec AC-0112-01
fn test_ac_0112_01_geo_distance() {
    let a = Point::new(0.0, 0.0);
    let b = Point::new(3.0, 4.0);
    assert!((a.distance(&b) - 5.0).abs() < 1e-9);
    assert!((a.manhattan(&b) - 7.0).abs() < 1e-9);
}
