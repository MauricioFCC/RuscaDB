//! Epoch-based reclamation (SPEC-0146): dominios y celdas RCU seguras.
//!
//! Implementacion **sin `unsafe`** (la crate lo prohibe): un [`RcuCell`] guarda
//! el valor actual tras un `RwLock<Arc<T>>`. Los lectores clonan el `Arc` y no
//! se bloquean entre si ni durante la publicacion; el escritor solo toma el
//! lock para el intercambio. Los valores retirados se acumulan hasta
//! [`RcuCell::synchronize`], que los libera cuando ya nadie puede verlos (los
//! lectores conservan su propio `Arc`).
//!
//! Complejidad: leer/publicar es `O(1)`; `synchronize` es `O(retirados)`.

use std::ops::Deref;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Dominio de epochs (lectores activos + epoch actual).
#[derive(Debug, Default)]
pub struct RcuDomain {
    epoch: AtomicU64,
    active: AtomicUsize,
}

impl RcuDomain {
    /// Crea un dominio en epoch 0.
    ///
    /// Returns:
    ///     Dominio vacio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Entra en la seccion critica y devuelve el epoch observado.
    ///
    /// Returns:
    ///     Epoch actual (el lector queda registrado hasta `exit`).
    pub fn enter(&self) -> u64 {
        self.active.fetch_add(1, Ordering::AcqRel);
        self.epoch.load(Ordering::Acquire)
    }

    /// Sale de la seccion critica.
    pub fn exit(&self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }

    /// Avanza el epoch (punto de quiescencia) y devuelve el nuevo valor.
    ///
    /// Returns:
    ///     Epoch tras el `synchronize`.
    pub fn synchronize(&self) -> u64 {
        self.epoch.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Epoch actual.
    ///
    /// Returns:
    ///     Contador de quiescencia.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Lectores activos.
    ///
    /// Returns:
    ///     Numero de secciones criticas abiertas.
    pub fn active_readers(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

/// Celda RCU: valor actual + valores retirados pendientes de liberar.
#[derive(Debug)]
pub struct RcuCell<T> {
    current: RwLock<Arc<T>>,
    retired: Mutex<Vec<Arc<T>>>,
    epoch: AtomicU64,
}

impl<T> RcuCell<T> {
    /// Crea la celda con un valor inicial.
    ///
    /// Args:
    ///     value: Valor publicado inicialmente.
    ///
    /// Returns:
    ///     Celda lista.
    pub fn new(value: T) -> Self {
        Self {
            current: RwLock::new(Arc::new(value)),
            retired: Mutex::new(Vec::new()),
            epoch: AtomicU64::new(0),
        }
    }

    /// Lee el valor actual (snapshot inmutable).
    ///
    /// Returns:
    ///     Guard con un `Arc` al valor; no bloquea a otros lectores.
    pub fn read(&self) -> RcuGuard<T> {
        let guard = self
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        RcuGuard {
            value: Arc::clone(&guard),
        }
    }

    /// Publica un valor nuevo y retira el anterior.
    ///
    /// Args:
    ///     value: Valor nuevo.
    ///
    /// Returns:
    ///     Epoch de la publicacion.
    pub fn publish(&self, value: T) -> u64 {
        let mut guard = self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::mem::replace(&mut *guard, Arc::new(value));
        drop(guard);
        self.retired
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(previous);
        self.epoch.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Libera los valores retirados (los lectores conservan su `Arc`).
    ///
    /// Returns:
    ///     Numero de valores liberados.
    pub fn synchronize(&self) -> usize {
        let mut retired = self
            .retired
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let freed = retired.len();
        retired.clear();
        self.epoch.fetch_add(1, Ordering::AcqRel);
        freed
    }

    /// Epoch actual de la celda.
    ///
    /// Returns:
    ///     Contador de publicaciones/quiescencias.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Valores retirados pendientes de liberar.
    ///
    /// Returns:
    ///     Longitud de la lista de retirados.
    pub fn retired_len(&self) -> usize {
        self.retired
            .lock()
            .map(|retired| retired.len())
            .unwrap_or(0)
    }
}

/// Guard de lectura RCU.
#[derive(Debug)]
pub struct RcuGuard<T> {
    value: Arc<T>,
}

impl<T> Deref for RcuGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> Clone for RcuGuard<T> {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RcuCell, RcuDomain};

    #[test] // @spec AC-0146-01
    fn test_ac_0146_01_rcu_snapshot() {
        let domain = RcuDomain::new();
        let epoch = domain.enter();
        assert_eq!(epoch, 0);
        assert_eq!(domain.active_readers(), 1);
        domain.exit();
        assert_eq!(domain.active_readers(), 0);
        assert_eq!(domain.synchronize(), 1);
        assert_eq!(domain.epoch(), 1);
    }

    /// Un lector conserva su snapshot aunque el escritor publique.
    #[test]
    fn reader_keeps_snapshot_across_publish() {
        let cell = RcuCell::new(vec![1, 2, 3]);
        let snapshot = cell.read();
        let epoch = cell.publish(vec![4, 5]);
        assert_eq!(epoch, 1);
        assert_eq!(&*snapshot, &[1, 2, 3], "el lector ve su version");
        assert_eq!(&*cell.read(), &[4, 5], "un lector nuevo ve la nueva");
        assert_eq!(cell.retired_len(), 1);
        assert_eq!(cell.synchronize(), 1, "libera el valor retirado");
        assert_eq!(cell.retired_len(), 0);
        // El snapshot sigue vivo aunque se haya sincronizado.
        assert_eq!(&*snapshot, &[1, 2, 3]);
    }
}
