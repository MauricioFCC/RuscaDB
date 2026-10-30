//! I/O asincrono sin Tokio (SPEC-0151): lector en memoria con `Future`.
//!
//! Implementa `poll_read`/`read_async` con `std::future` y `Waker`: el dato
//! esta en memoria, por lo que el futuro resuelve en el primer `poll`
//! (`Poll::Ready`). El offload real a hilos lo hace el executor del consumidor.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Lector en memoria con lectura acotada.
#[derive(Debug, Clone)]
pub struct AsyncReader {
    /// Bytes disponibles.
    pub data: Vec<u8>,
}

/// Futuro de lectura sobre [`AsyncReader`].
#[derive(Debug)]
pub struct AsyncReadFuture<'a> {
    reader: &'a AsyncReader,
    offset: usize,
    len: usize,
}

impl Future for AsyncReadFuture<'_> {
    type Output = Vec<u8>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(self.reader.read_sync(self.offset, self.len))
    }
}

impl AsyncReader {
    /// Crea el lector.
    ///
    /// Args:
    ///     data: Bytes a leer.
    ///
    /// Returns:
    ///     Lector listo.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Lee `len` bytes desde `offset` (vacio fuera de rango).
    ///
    /// Args:
    ///     offset: Posicion inicial.
    ///     len: Numero de bytes.
    ///
    /// Returns:
    ///     Copia de los bytes disponibles; vacio si el rango no cabe.
    pub fn read_sync(&self, offset: usize, len: usize) -> Vec<u8> {
        self.data
            .get(offset..offset.saturating_add(len))
            .unwrap_or(&[])
            .to_vec()
    }

    /// Version `poll` de la lectura (siempre `Ready` en memoria).
    ///
    /// Args:
    ///     offset: Posicion inicial.
    ///     len: Numero de bytes.
    ///     cx: Contexto de la tarea (se registra el waker; no se usa).
    ///
    /// Returns:
    ///     `Poll::Ready(bytes)`.
    pub fn poll_read(&self, offset: usize, len: usize, cx: &mut Context<'_>) -> Poll<Vec<u8>> {
        cx.waker().wake_by_ref();
        Poll::Ready(self.read_sync(offset, len))
    }

    /// Futuro de lectura.
    ///
    /// Args:
    ///     offset: Posicion inicial.
    ///     len: Numero de bytes.
    ///
    /// Returns:
    ///     Futuro que resuelve con los bytes.
    pub fn read_async(&self, offset: usize, len: usize) -> AsyncReadFuture<'_> {
        AsyncReadFuture {
            reader: self,
            offset,
            len,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AsyncReader;
    use std::future::Future;
    use std::task::{Context, Poll};

    #[test] // @spec AC-0151-01
    fn test_ac_0151_01_async_read() {
        let reader = AsyncReader::new(vec![1, 2, 3, 4, 5]);
        assert_eq!(reader.read_sync(1, 3), vec![2, 3, 4]);
        assert!(reader.read_sync(9, 3).is_empty());

        let waker = std::task::Waker::noop();
        let mut context = Context::from_waker(waker);
        match reader.poll_read(1, 3, &mut context) {
            Poll::Ready(bytes) => assert_eq!(bytes, vec![2, 3, 4]),
            Poll::Pending => panic!("en memoria debe estar listo"),
        }

        let mut future = std::pin::pin!(reader.read_async(0, 2));
        match future.as_mut().poll(&mut context) {
            Poll::Ready(bytes) => assert_eq!(bytes, vec![1, 2]),
            Poll::Pending => panic!("future en memoria debe estar listo"),
        }
    }
}
