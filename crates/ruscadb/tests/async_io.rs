//! Tests de aceptación de la feature `async-io` (SPEC-0151).

#![allow(clippy::expect_used)]

use ruscadb::AsyncReader;

#[test] // @spec AC-0151-01
fn test_ac_0151_01_async_read() {
    let reader = AsyncReader::new(vec![1, 2, 3, 4, 5]);
    let data = reader.read_sync(1, 3);
    assert_eq!(data, vec![2, 3, 4]);
}
