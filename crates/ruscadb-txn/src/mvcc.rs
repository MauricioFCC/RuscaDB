//! MVCC snapshot isolation (roadmap §5.3).
//!
//! Cada registro versiona su procedencia con [`Version`]
//! (`created_tx`/`deleted_tx`). Un lector captura un [`Snapshot`] fijo y decide
//! si una versión le es visible sin tomar candados. El aislamiento es *snapshot
//! isolation*: sin lecturas sucias, con *first-committer-wins* como política de
//! conflicto.

use std::collections::BTreeSet;

use ruscadb_core::RuscaError;
use serde::{Deserialize, Serialize};

/// Identificador de transacción (monótono creciente, nunca reutilizado).
pub type TxId = u64;

/// Identificador reservado que representa «sin transacción» (bootstrap/físico).
pub const NO_TX: TxId = 0;

/// Primer `TxId` asignado por [`TxnManager::begin`].
const FIRST_TX: TxId = NO_TX + 1;

/// Versión MVCC de un registro.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// Transacción que creó la versión.
    pub created_tx: TxId,
    /// Transacción que borró la versión, o `None` si sigue viva.
    pub deleted_tx: Option<TxId>,
}

impl Version {
    /// Crea una versión viva (sin borrado).
    ///
    /// Args:
    ///     created_tx: Transacción que crea la versión.
    ///
    /// Returns:
    ///     La versión con `deleted_tx = None`.
    pub fn new(created_tx: TxId) -> Self {
        Self {
            created_tx,
            deleted_tx: None,
        }
    }
}

/// Vista de visibilidad fija (fotografía) de un lector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Cota superior: solo versiones con `created_tx <= tx_id` pueden verse.
    pub tx_id: TxId,
    /// Transacciones en vuelo en el instante del snapshot.
    in_flight: BTreeSet<TxId>,
}

impl Snapshot {
    /// Regla de visibilidad *snapshot isolation*.
    ///
    /// Una versión `v` es visible si:
    /// 1. `v.created_tx` no estaba en vuelo al tomar el snapshot;
    /// 2. `v.created_tx <= self.tx_id`; y
    /// 3. no fue borrada a la vista: `v.deleted_tx` es `None`, o su borrador
    ///    estaba en vuelo, o pertenece a una transacción posterior.
    ///
    /// Args:
    ///     version: Versión a evaluar.
    ///
    /// Returns:
    ///     `true` si la versión es visible para este snapshot.
    pub fn is_visible(&self, version: &Version) -> bool {
        if self.in_flight.contains(&version.created_tx) {
            return false;
        }
        if version.created_tx > self.tx_id {
            return false;
        }
        match version.deleted_tx {
            None => true,
            Some(deleter) => self.in_flight.contains(&deleter) || deleter > self.tx_id,
        }
    }
}

/// Gestor MVCC: asigna `TxId`, publica commits y fija snapshots.
///
/// # Invariantes
/// - Los `TxId` son estrictamente crecientes y únicos (nunca se reutilizan).
/// - [`commit`](TxnManager::commit) devuelve `Ok` solo para transacciones en
///   vuelo; tras `Ok`, la transacción pasa a
///   [`is_committed`](TxnManager::is_committed).
/// - Un [`snapshot`](TxnManager::snapshot) congela la visibilidad: los commits
///   posteriores no alteran snapshots ya tomados.
#[derive(Clone, Debug)]
pub struct TxnManager {
    next_tx: TxId,
    in_flight: BTreeSet<TxId>,
    committed: BTreeSet<TxId>,
}

impl TxnManager {
    /// Crea un gestor sin transacciones.
    ///
    /// Returns:
    ///     Gestor con el primer `TxId` disponible en `FIRST_TX`.
    #[allow(clippy::new_without_default)] // no hay un «default» con semántica útil
    pub fn new() -> Self {
        Self {
            next_tx: FIRST_TX,
            in_flight: BTreeSet::new(),
            committed: BTreeSet::new(),
        }
    }

    /// Inicia una transacción y devuelve su identificador.
    ///
    /// Returns:
    ///     Un `TxId` nuevo, estrictamente mayor que el anterior; la transacción
    ///     queda en vuelo.
    pub fn begin(&mut self) -> TxId {
        let tx = self.next_tx;
        self.next_tx += 1;
        self.in_flight.insert(tx);
        tx
    }

    /// Publica una transacción en vuelo.
    ///
    /// Args:
    ///     tx: Identificador devuelto por [`begin`](TxnManager::begin).
    ///
    /// Returns:
    ///     `Ok(())` si la transacción estaba en vuelo; `Err` en caso contrario.
    ///
    /// Raises:
    ///     [`RuscaError::InvalidConfig`] si `tx` no está en vuelo (inexistente,
    ///     ya confirmada o nunca iniciada).
    pub fn commit(&mut self, tx: TxId) -> Result<(), RuscaError> {
        if !self.in_flight.remove(&tx) {
            return Err(RuscaError::InvalidConfig(format!(
                "commit de transacción no en vuelo: tx_id {tx}"
            )));
        }
        self.committed.insert(tx);
        Ok(())
    }

    /// Aborta una transacción en vuelo: la retira de `in_flight` y **no** la
    /// publica (nunca entra en `committed`).
    ///
    /// Es la operación dual de [`commit`](TxnManager::commit): tras un `rollback`
    /// la transacción deja de bloquear el *low watermark* y sus versiones no
    /// quedan confirmadas. El llamador (p. ej. `Database::rollback`) es
    /// responsable de descartar las páginas que solo existían en memoria.
    ///
    /// Args:
    ///     tx: Identificador devuelto por [`begin`](TxnManager::begin).
    ///
    /// Returns:
    ///     `Ok(())` si la transacción estaba en vuelo; `Err` en caso contrario.
    ///
    /// Raises:
    ///     [`RuscaError::InvalidConfig`] si `tx` no está en vuelo (inexistente,
    ///     ya confirmada o ya abortada).
    pub fn rollback(&mut self, tx: TxId) -> Result<(), RuscaError> {
        if !self.in_flight.remove(&tx) {
            return Err(RuscaError::InvalidConfig(format!(
                "rollback de transacción no en vuelo: tx_id {tx}"
            )));
        }
        Ok(())
    }

    /// Toma un snapshot con la visibilidad actual.
    ///
    /// Semántica exacta: `tx_id` es el último identificador asignado
    /// (`next_tx - 1`, o [`NO_TX`] si aún no se inició ninguna transacción), y
    /// `in_flight` la copia del conjunto en vuelo. Todo `TxId` asignado es
    /// `<= tx_id`; combinado con el conjunto en vuelo, esto separa los commits
    /// visibles de las transacciones no confirmadas.
    ///
    /// Returns:
    ///     La vista fija de visibilidad.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tx_id: self.next_tx - 1,
            in_flight: self.in_flight.clone(),
        }
    }

    /// Indica si `tx` ya está confirmada.
    ///
    /// Args:
    ///     tx: Identificador a consultar.
    ///
    /// Returns:
    ///     `true` si la transacción se publicó con éxito.
    pub fn is_committed(&self, tx: TxId) -> bool {
        self.committed.contains(&tx)
    }

    /// Calcula el *low watermark* MVCC (menor transacción en vuelo).
    ///
    /// Es el menor [`TxId`] en vuelo; si no hay ninguna transacción en vuelo,
    /// devuelve el siguiente `TxId` a asignar. Todo lo `< watermark` está
    /// confirmado (nunca en vuelo) y es visible a cualquier snapshot nuevo, de
    /// modo que su purga no puede afectar a ningún lector futuro.
    ///
    /// Returns:
    ///     La cota inferior de transacciones potencialmente visibles.
    pub fn low_watermark(&self) -> TxId {
        self.in_flight.first().copied().unwrap_or(self.next_tx)
    }
}

/// Indica si `version` es obsoleta respecto a `watermark`.
///
/// Una versión es obsoleta cuando fue borrada por una transacción
/// `deleted_tx = Some(d)` con `d < watermark`: el borrador ya está confirmado y
/// por debajo del watermark, de modo que ningún snapshot futuro (con
/// `tx_id >= watermark`) puede volver a verla.
///
/// Invariante: una versión viva (`deleted_tx == None`) nunca es obsoleta, y una
/// borrada con `deleted_tx >= watermark` se conserva (aún visible a algún
/// snapshot).
///
/// Args:
///     version: Versión MVCC a evaluar.
///     watermark: Low watermark actual (`TxnManager::low_watermark`).
///
/// Returns:
///     `true` si la versión puede purgarse sin afectar a lectores futuros.
pub fn is_obsolete(version: &Version, watermark: TxId) -> bool {
    matches!(version.deleted_tx, Some(deleter) if deleter < watermark)
}

/// Purga *in-place* las versiones obsoletas y devuelve cuántas eliminó.
///
/// Conserva toda versión viva (`deleted_tx == None`) y toda versión borrada con
/// `deleted_tx >= watermark` (aún visible a algún snapshot). Es un *no-op* sin
/// pánicos para un vector vacío y es idempotente (una segunda pasada no elimina
/// nada).
///
/// Args:
///     versions: Versiones del registro; se filtran en el propio vector.
///     watermark: Low watermark actual (`TxnManager::low_watermark`).
///
/// Returns:
///     Número de versiones obsoletas eliminadas.
pub fn gc(versions: &mut Vec<Version>, watermark: TxId) -> usize {
    let before = versions.len();
    versions.retain(|version| !is_obsolete(version, watermark));
    before - versions.len()
}
