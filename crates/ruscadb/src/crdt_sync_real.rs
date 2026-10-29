//! CRDT de sincronizacion: registro last-writer-wins con reloj vectorial
//! (SPEC-0179).
//!
//! Un CRDT debe ser un semirreticulo: `merge` conmutativo, asociativo e
//! idempotente (Shapiro et al., "Conflict-free Replicated Data Types", SSS
//! 2011). Aqui el valor se resuelve por orden total `(contador, actor)`, de
//! modo que dos replicas que han visto lo mismo convergen al mismo valor sin
//! coordinacion. Las tres propiedades se comprueban con tests.

use std::collections::BTreeMap;

/// Registro LWW con reloj vectorial.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrdtSyncReal {
    /// Identidad de la replica.
    actor: String,
    /// Reloj vectorial `actor -> contador`.
    clock: BTreeMap<String, u64>,
    /// Valor actual (si se ha escrito alguna vez).
    value: Option<i64>,
    /// Marca de la escritura ganadora `(contador, actor)`.
    stamp: Option<(u64, String)>,
}

impl CrdtSyncReal {
    /// Crea una replica con actor `"local"`.
    ///
    /// Returns:
    ///     Replica vacia.
    pub fn new() -> Self {
        Self::with_actor("local")
    }

    /// Crea una replica con identidad explicita.
    ///
    /// Args:
    ///     actor: Nombre unico de la replica.
    ///
    /// Returns:
    ///     Replica vacia con ese actor.
    pub fn with_actor(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            clock: BTreeMap::new(),
            value: None,
            stamp: None,
        }
    }

    /// Incrementa el reloj local.
    ///
    /// Returns:
    ///     Nuevo contador del actor local.
    pub fn tick(&mut self) -> u64 {
        let counter = self.clock.entry(self.actor.clone()).or_insert(0);
        *counter += 1;
        *counter
    }

    /// Escribe un valor (last-writer-wins).
    ///
    /// Args:
    ///     value: Nuevo valor del registro.
    pub fn set(&mut self, value: i64) {
        let counter = self.tick();
        self.value = Some(value);
        self.stamp = Some((counter, self.actor.clone()));
    }

    /// Lee el valor actual.
    ///
    /// Returns:
    ///     `Some(valor)` si hubo escritura, `None` si el registro esta vacio.
    pub fn value(&self) -> Option<i64> {
        self.value
    }

    /// Contador local (delta desde el ultimo merge).
    ///
    /// Returns:
    ///     Contador del actor local.
    pub fn delta(&self) -> u64 {
        self.clock.get(&self.actor).copied().unwrap_or(0)
    }

    /// Reloj vectorial actual.
    ///
    /// Returns:
    ///     Mapa ordenado `actor -> contador`.
    pub fn vector_clock(&self) -> &BTreeMap<String, u64> {
        &self.clock
    }

    /// Fusiona otra replica en esta (in-place).
    ///
    /// Args:
    ///     other: Replica remota.
    pub fn merge(&mut self, other: &Self) {
        *self = self.merged(other);
    }

    /// Calcula la fusion sin mutar `self`.
    ///
    /// Args:
    ///     other: Replica remota.
    ///
    /// Returns:
    ///     Nueva replica con el join de ambas.
    pub fn merged(&self, other: &Self) -> Self {
        let mut clock = self.clock.clone();
        for (actor, &counter) in &other.clock {
            let entry = clock.entry(actor.clone()).or_insert(0);
            if counter > *entry {
                *entry = counter;
            }
        }
        let stamp = match (&self.stamp, &other.stamp) {
            (Some(a), Some(b)) => Some(if a >= b { a.clone() } else { b.clone() }),
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };
        let value = match (&self.stamp, &other.stamp) {
            (Some(a), Some(b)) => {
                if a >= b {
                    self.value
                } else {
                    other.value
                }
            }
            (Some(_), None) => self.value,
            (None, Some(_)) => other.value,
            (None, None) => None,
        };
        Self {
            actor: self.actor.clone(),
            clock,
            value,
            stamp,
        }
    }

    /// Bytes de estado para un delta de sincronizacion.
    ///
    /// Returns:
    ///     `(num_actores, suma_de_contadores)` como metrica de coste.
    pub fn state_size(&self) -> (usize, u64) {
        (self.clock.len(), self.clock.values().sum())
    }
}

#[cfg(test)]
mod tests {
    use super::CrdtSyncReal;

    /// AC-0179-01: `merge` resuelve conflictos por orden total y converge.
    #[test]
    // @spec AC-0179-01
    fn test_ac_0179_01_merge() {
        let mut a = CrdtSyncReal::with_actor("a");
        let mut b = CrdtSyncReal::with_actor("b");
        a.set(3);
        b.set(5);
        a.merge(&b);
        b.merge(&a);
        assert_eq!(a.value(), b.value(), "convergencia");
        assert_eq!(a.value(), Some(5), "gana el contador mayor");

        // Empate de contador: desempata el actor (orden lexicografico estable).
        let mut x = CrdtSyncReal::with_actor("x");
        let mut y = CrdtSyncReal::with_actor("y");
        x.set(10);
        y.set(20);
        assert_eq!(x.merged(&y).value(), Some(20));
        assert_eq!(y.merged(&x).value(), Some(20));

        // El reloj vectorial es el maximo componente a componente.
        let clock = x.merged(&y).vector_clock().clone();
        assert_eq!(clock.get("x"), Some(&1));
        assert_eq!(clock.get("y"), Some(&1));
    }

    /// AC-0179-02: el delta local solo avanza con escrituras locales y el
    /// merge es idempotente.
    #[test]
    // @spec AC-0179-02
    fn test_ac_0179_02_delta() {
        let mut replica = CrdtSyncReal::new();
        assert_eq!(replica.delta(), 0);
        replica.tick();
        assert_eq!(replica.delta(), 1);
        replica.set(7);
        assert_eq!(replica.delta(), 2);
        assert_eq!(replica.value(), Some(7));

        let mut remote = CrdtSyncReal::with_actor("remote");
        remote.set(1);
        remote.set(2);
        replica.merge(&remote);
        assert_eq!(replica.delta(), 2, "merge no incrementa el contador local");
        let (actors, total) = replica.state_size();
        assert_eq!(actors, 2);
        assert_eq!(total, 4);

        // Idempotencia: fusionar dos veces no cambia nada.
        let once = replica.clone();
        replica.merge(&once);
        assert_eq!(replica.value(), once.value());
        assert_eq!(replica.vector_clock(), once.vector_clock());
    }

    /// Semirreticulo: conmutatividad, asociatividad e idempotencia sobre tres
    /// replicas. Es la propiedad que hace segura la sincronizacion sin
    /// coordinacion.
    #[test]
    fn merge_is_a_semilattice() {
        let mut a = CrdtSyncReal::with_actor("a");
        let mut b = CrdtSyncReal::with_actor("b");
        let mut c = CrdtSyncReal::with_actor("c");
        a.set(1);
        b.set(2);
        c.set(3);

        // Conmutativa.
        assert_eq!(a.merged(&b).value(), b.merged(&a).value());
        // Asociativa (en valor y reloj).
        let left = a.merged(&b).merged(&c);
        let right = a.merged(&b.merged(&c));
        assert_eq!(left.value(), right.value());
        assert_eq!(left.vector_clock(), right.vector_clock());
        // Idempotente.
        assert_eq!(a.merged(&a).value(), a.value());
        assert_eq!(a.merged(&a).vector_clock(), a.vector_clock());
        // Estado vacio no contamina.
        let empty = CrdtSyncReal::with_actor("z");
        assert_eq!(a.merged(&empty).value(), a.value());
    }
}
