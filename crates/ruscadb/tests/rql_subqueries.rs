#![allow(missing_docs)]
#![cfg(feature = "rql-subqueries")]
use ruscadb::{RqlSubquery, SubqueryError};

#[test] // @spec AC-0162-01
fn test_ac_0162_01_scalar() {
    let subquery = RqlSubquery::new();
    assert_eq!(subquery.scalar(&[42]), Ok(Some(42)));
    assert_eq!(subquery.scalar(&[]), Ok(None));
    assert_eq!(
        subquery.scalar(&[42, 7]),
        Err(SubqueryError::MultipleRows(2))
    );
}

#[test] // @spec AC-0162-02
fn test_ac_0162_02_in() {
    let subquery = RqlSubquery::new();
    assert!(subquery.in_subquery(2, &[1, 2, 3]));
    assert!(!subquery.in_subquery(9, &[1, 2, 3]));
}

#[test] // @spec AC-0162-03
fn test_ac_0162_03_exists() {
    let subquery = RqlSubquery::new();
    assert!(subquery.exists(&[1]));
    assert!(!subquery.exists(&[]));
}

#[test] // @spec AC-0162-04
fn test_ac_0162_04_derived() {
    let subquery = RqlSubquery::with_limit(2);
    let rows = vec![vec![Some(10)], vec![Some(20)], vec![Some(30)]];
    assert_eq!(
        subquery.eval_cells(ruscadb::SubqueryKind::In, Some(30), &rows),
        Ok(Some(false))
    );
}
