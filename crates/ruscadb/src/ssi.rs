//! Re-export del tracker SSI (SPEC-0170): la implementacion vive en `ruscadb-txn`.
//!
//! El `TxnManager` registra las aristas rw al confirmar (opt-in con
//! `enable_ssi(true)`); este modulo mantiene la API publica de la fachada.

pub use ruscadb_txn::{IsoLevel, SsiTracker};
#[cfg(test)]
mod tests {
    use super::{IsoLevel, SsiTracker};

    /// AC-0170-01: un ciclo rw se detecta y la estructura peligrosa con pivote
    /// tambien (dos aristas consecutivas sin ciclo).
    #[test]
    // @spec AC-0170-01
    fn test_ac_0170_01_cycle() {
        let mut tracker = SsiTracker::new();
        tracker.add_rw_edge(1, 2);
        assert!(!tracker.has_cycle(), "una arista no es ciclo");
        tracker.add_rw_edge(2, 1);
        assert!(tracker.has_cycle(), "1->2->1 es ciclo");
        assert!(tracker.should_abort(1));

        // Cadena de 3: sin ciclo, pero estructura peligrosa con pivote 2.
        let mut chain = SsiTracker::new();
        chain.add_rw_edge(10, 20);
        chain.add_rw_edge(20, 30);
        assert!(!chain.has_cycle());
        assert!(chain.has_dangerous_structure());
        assert_eq!(chain.dangerous_pivot(), Some(20));
        assert!(chain.should_abort(20));
        assert!(!chain.should_abort(10));
        assert!(!chain.is_serializable());
        assert_eq!(chain.edge_count(), 2);

        // Diamante sin ciclo: 1->2, 1->3, 2->4, 3->4.
        let mut diamond = SsiTracker::new();
        for (from, to) in [(1, 2), (1, 3), (2, 4), (3, 4)] {
            diamond.add_rw_edge(from, to);
        }
        assert!(!diamond.has_cycle());
        assert!(diamond.has_dangerous_structure());

        // Self-loop se ignora (no es una anomalia real).
        let mut noop = SsiTracker::new();
        noop.add_rw_edge(7, 7);
        assert_eq!(noop.edge_count(), 0);
        assert!(!noop.has_cycle());
    }

    /// AC-0170-02: lectores puros no bloquean ni se abortan.
    #[test]
    // @spec AC-0170-02
    fn test_ac_0170_02_readonly() {
        let mut tracker = SsiTracker::new();
        for tx in 1..=5 {
            tracker.register(tx);
        }
        assert!(!tracker.has_cycle());
        assert!(!tracker.has_dangerous_structure());
        assert!(tracker.is_serializable());
        for tx in 1..=5 {
            assert!(!tracker.should_abort(tx), "lector {tx} no debe abortar");
        }

        // Al retirar el escritor, el historial vuelve a ser serializable.
        let mut mixed = SsiTracker::new();
        mixed.add_rw_edge(1, 2);
        mixed.add_rw_edge(2, 3);
        assert!(!mixed.is_serializable());
        mixed.retire(2);
        assert!(mixed.is_serializable());
        assert_eq!(mixed.edge_count(), 0, "retirar 2 borra sus dos aristas");
    }

    /// Los niveles de aislamiento existen y Serializable es el mas estricto.
    #[test]
    fn iso_levels_are_distinct() {
        assert_ne!(IsoLevel::ReadCommitted, IsoLevel::Snapshot);
        assert_ne!(IsoLevel::Snapshot, IsoLevel::Serializable);
        assert_eq!(IsoLevel::default(), IsoLevel::Snapshot);
    }
}
