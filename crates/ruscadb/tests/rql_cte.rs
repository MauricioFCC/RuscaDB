#![allow(missing_docs)]
#![cfg(feature = "rql-cte")]
#![allow(clippy::expect_used)]
use ruscadb::RqlCte;
#[test] // @spec AC-0163-01
fn test_ac_0163_01_cte() {
    let c = RqlCte::new("t", false);
    assert_eq!(c.name(), "t");
    assert!(!c.is_recursive());
}
#[test] // @spec AC-0163-02
fn test_ac_0163_02_recursive() {
    let c = RqlCte::new("g", true);
    let res = c.fixed_point(
        vec![1],
        |rows| {
            if rows.len() < 4 {
                vec![rows.len() as i64 + 1]
            } else {
                vec![]
            }
        },
        10,
    );
    assert_eq!(res, vec![1, 2, 3, 4]);
}
