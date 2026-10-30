//! Tests de aceptacion de la feature `async-io` (SPEC-0151).

#![cfg(feature = "async-io")]
#![allow(clippy::expect_used)]

use ruscadb::AsyncReader;
use std::future::Future;
use std::task::{Context, Poll};

#[test] // @spec AC-0151-01
fn test_ac_0151_01_async_read() {
    let reader = AsyncReader::new(vec![1, 2, 3, 4, 5]);
    assert_eq!(reader.read_sync(1, 3), vec![2, 3, 4]);
    assert!(reader.read_sync(9, 3).is_empty());

    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    assert!(matches!(
        reader.poll_read(0, 2, &mut cx),
        Poll::Ready(bytes) if bytes == vec![1, 2]
    ));

    let mut future = std::pin::pin!(reader.read_async(2, 3));
    assert!(matches!(
        future.as_mut().poll(&mut cx),
        Poll::Ready(bytes) if bytes == vec![3, 4, 5]
    ));
}
