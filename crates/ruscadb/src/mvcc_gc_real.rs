//! Recoleccion de basura MVCC con horizonte de snapshots (SPEC-0169).
//!
//! Una version es reclamable cuando **nadie puede volver a verla**. Con MVCC,
//! eso ocurre cuando existe una transaccion de borrado `D` y ningun snapshot
//! activo puede observar versiones anteriores a `D`; es decir, cuando
//! `D < horizonte`, con `horizonte = min(snapshots activos)` (o el watermark
//! global si no hay lectores). Las versiones *vivas* (sin `deleted_tx`) **nunca**
//! se reclaman: hacerlo perderia datos.

use std::collections::BTreeSet;

/// Version de una fila.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// Transaccion que creo la version.
    pub created_tx: u64,
    /// Transaccion que la borro, si existe.
    pub deleted_tx: Option<u64>,
}

/// Recolector MVCC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MvccGcReal {
    /// Watermark global (ultima transaccion completada).
    pub low_watermark: u64,
    /// Snapshots de lectura activos.
    active: BTreeSet<u64>,
}

impl Default for MvccGcReal {
    fn default() -> Self {
        Self::new()
    }
}

impl MvccGcReal {
    /// Crea el recolector vacio.
    ///
    /// Returns:
    ///     Recolector con watermark 0 y sin snapshots.
    pub fn new() -> Self {
        Self {
            low_watermark: 0,
            active: BTreeSet::new(),
        }
    }

    /// Fija el watermark global.
    ///
    /// Args:
    ///     tx: Ultima transaccion completada.
    pub fn set_watermark(&mut self, tx: u64) {
        self.low_watermark = tx;
    }

    /// Registra un snapshot de lectura activo.
    ///
    /// Args:
    ///     tx: Identificador del snapshot.
    pub fn register_snapshot(&mut self, tx: u64) {
        self.active.insert(tx);
    }

    /// Libera un snapshot de lectura.
    ///
    /// Args:
    ///     tx: Identificador del snapshot.
    pub fn release_snapshot(&mut self, tx: u64) {
        self.active.remove(&tx);
    }

    /// Horizonte por debajo del cual las versiones borradas son invisibles.
    ///
    /// Returns:
    ///     Minimo snapshot activo, o el watermark si no hay lectores.
    pub fn horizon(&self) -> u64 {
        self.active
            .iter()
            .next()
            .copied()
            .unwrap_or(self.low_watermark)
    }

    /// Decide si una version es reclamable.
    ///
    /// Args:
    ///     created_tx: Transaccion de creacion.
    ///     deleted_tx: Transaccion de borrado (`None` = viva).
    ///
    /// Returns:
    ///     `true` solo si esta borrada y su borrado es anterior al horizonte.
    pub fn is_obsolete(&self, created_tx: u64, deleted_tx: Option<u64>) -> bool {
        let _ = created_tx;
        match deleted_tx {
            Some(deleted) => deleted < self.horizon(),
            None => false,
        }
    }

    /// Separa versiones reclamables de las que hay que conservar.
    ///
    /// Args:
    ///     versions: Versiones de una fila.
    ///
    /// Returns:
    ///     `(conservadas, reclamables)`.
    pub fn reclaim_versions(&self, versions: &[Version]) -> (Vec<Version>, Vec<Version>) {
        let (dead, alive): (Vec<_>, Vec<_>) = versions
            .iter()
            .cloned()
            .partition(|version| self.is_obsolete(version.created_tx, version.deleted_tx));
        (alive, dead)
    }

    /// Cuenta versiones por debajo del watermark (metrica historica).
    ///
    /// Args:
    ///     versions: Transacciones de creacion.
    ///
    /// Returns:
    ///     Numero de versiones anteriores al watermark.
    pub fn reclaim_count(&self, versions: &[u64]) -> usize {
        versions
            .iter()
            .filter(|&&version| version < self.low_watermark)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::{MvccGcReal, Version};

    /// AC-0169-01: una version borrada antes del horizonte se reclama.
    #[test]
    // @spec AC-0169-01
    fn test_ac_0169_01_gc() {
        let mut gc = MvccGcReal::new();
        gc.set_watermark(5);
        assert!(gc.is_obsolete(2, Some(3)));
        let versions = vec![
            Version {
                created_tx: 1,
                deleted_tx: Some(3),
            },
            Version {
                created_tx: 2,
                deleted_tx: Some(4),
            },
        ];
        let (alive, dead) = gc.reclaim_versions(&versions);
        assert!(alive.is_empty());
        assert_eq!(dead.len(), 2);
    }

    /// AC-0169-02: un lector activo bloquea el GC de lo que aun puede ver, y
    /// una version viva nunca se reclama.
    #[test]
    // @spec AC-0169-02
    fn test_ac_0169_02_reader_blocks() {
        let mut gc = MvccGcReal::new();
        gc.set_watermark(10);
        gc.register_snapshot(3);
        assert_eq!(gc.horizon(), 3);
        // Borrada en 8: el lector del snapshot 3 aun podria necesitarla.
        assert!(!gc.is_obsolete(1, Some(8)));
        // Borrada en 2: ya es invisible para todos.
        assert!(gc.is_obsolete(1, Some(2)));
        // Viva: jamas se reclama.
        assert!(!gc.is_obsolete(1, None));

        gc.release_snapshot(3);
        assert_eq!(gc.horizon(), 10);
        assert!(gc.is_obsolete(1, Some(8)), "sin lectores el horizonte sube");
    }

    /// AC-0169-03: metricas de reclamacion.
    #[test]
    // @spec AC-0169-03
    fn test_ac_0169_03_metrics() {
        let mut gc = MvccGcReal::new();
        gc.set_watermark(5);
        assert_eq!(gc.reclaim_count(&[1, 2, 6]), 2);
        gc.register_snapshot(1);
        assert_eq!(gc.horizon(), 1);
        let (alive, dead) = gc.reclaim_versions(&[Version {
            created_tx: 1,
            deleted_tx: Some(9),
        }]);
        assert!(dead.is_empty());
        assert_eq!(alive.len(), 1);
    }

    /// El horizonte sigue al minimo snapshot activo y vuelve al watermark.
    #[test]
    fn horizon_is_min_active_snapshot() {
        let mut gc = MvccGcReal::new();
        gc.set_watermark(20);
        assert_eq!(gc.horizon(), 20);
        gc.register_snapshot(7);
        gc.register_snapshot(3);
        assert_eq!(gc.horizon(), 3);
        gc.release_snapshot(3);
        assert_eq!(gc.horizon(), 7);
        gc.release_snapshot(7);
        assert_eq!(gc.horizon(), 20);
        // Liberar un snapshot inexistente no altera el horizonte.
        gc.release_snapshot(99);
        assert_eq!(gc.horizon(), 20);
    }

    /// Particion correcta de versiones vivas frente a reclamables.
    #[test]
    fn reclaim_partitions_alive_and_dead() {
        let mut gc = MvccGcReal::new();
        gc.set_watermark(10);
        let versions = vec![
            Version {
                created_tx: 1,
                deleted_tx: Some(5),
            },
            Version {
                created_tx: 2,
                deleted_tx: None,
            },
            Version {
                created_tx: 3,
                deleted_tx: Some(10),
            },
        ];
        let (alive, dead) = gc.reclaim_versions(&versions);
        assert_eq!(dead.len(), 1, "solo deleted=5 < horizonte");
        assert_eq!(
            alive.len(),
            2,
            "viva y delete=10 (== horizonte) se conservan"
        );
    }
}
