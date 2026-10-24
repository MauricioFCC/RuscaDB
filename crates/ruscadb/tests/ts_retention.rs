//! Tests de aceptación de la feature `ts-retention` (SPEC-0122).

#![allow(clippy::expect_used)]

use ruscadb::{RetentionPolicy, TsPoint};

#[test] // @spec AC-0122-01
fn test_ac_0122_01_retention_ttl() {
    let policy = RetentionPolicy::new(1000);
    let now = 5000;
    let points = vec![
        TsPoint {
            ts: 1000,
            value: 1.0,
        },
        TsPoint {
            ts: 4500,
            value: 2.0,
        },
        TsPoint {
            ts: 4900,
            value: 3.0,
        },
    ];
    let result = policy.apply(now, &points);
    assert_eq!(result.len(), 2);
}
