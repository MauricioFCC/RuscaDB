//! Tests de aceptación de la feature `materialized-views` (SPEC-0149).

#![allow(clippy::expect_used)]

use ruscadb::MaterializedView;

#[test] // @spec AC-0149-01
fn test_ac_0149_01_materialized_view_create() {
    let mut view = MaterializedView::new("v1", "SELECT * FROM t");
    view.create(vec![vec!["1".to_string(), "a".to_string()]]);
    assert_eq!(view.rows().len(), 1);
    assert_eq!(view.name(), "v1");
}
