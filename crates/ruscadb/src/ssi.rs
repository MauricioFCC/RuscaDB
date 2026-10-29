//! Serializable Snapshot Isolation (SSI) con grafo de dependencias rw, SPEC-0170.
//!
//! SSI (Cahill, Rohm y Fekete, SIGMOD 2009) detecta anomalias de
//! serializacion sin bloquear lectores: construye el grafo de dependencias
//! `write -> read` (rw) y aborta cuando aparece un ciclo o una **estructura
//! peligrosa** (dos aristas rw consecutivas con el mismo pivote). Este modulo
//! implementa el grafo y la deteccion; no gestiona el ciclo de vida del
//! snapshot (eso vive en `ruscadb-txn`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Niveles de aislamiento expuestos al usuario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IsoLevel {
    /// Lecturas no repetibles permitidas.
    ReadCommitted,
    /// Snapshot isolation (MVCC).
    #[default]
    Snapshot,
    /// Serializable (snapshot + deteccion SSI).
    Serializable,
}

/// Grafo de dependencias rw para deteccion SSI.
#[derive(Debug, Clone, Default)]
pub struct SsiTracker {
    /// Transacciones registradas.
    nodes: BTreeSet<u64>,
    /// Aristas rw `(desde, hacia)`: "desde" escribio lo que "hacia" leyo.
    rw_edges: BTreeSet<(u64, u64)>,
}

impl SsiTracker {
    /// Crea un tracker vacio.
    ///
    /// Returns:
    ///     Grafo sin nodos ni aristas.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra una transaccion activa.
    ///
    /// Args:
    ///     tx: Identificador de transaccion.
    pub fn register(&mut self, tx: u64) {
        self.nodes.insert(tx);
    }

    /// Anade una arista rw.
    ///
    /// Args:
    ///     from: Transaccion que escribio.
    ///     to: Transaccion que leyo lo escrito.
    pub fn add_rw_edge(&mut self, from: u64, to: u64) {
        if from == to {
            return;
        }
        self.nodes.insert(from);
        self.nodes.insert(to);
        self.rw_edges.insert((from, to));
    }

    /// Numero de aristas rw registradas.
    ///
    /// Returns:
    ///     Cardinalidad del conjunto de aristas.
    pub fn edge_count(&self) -> usize {
        self.rw_edges.len()
    }

    /// Lista de adyacencia `nodo -> sucesores`.
    ///
    /// Returns:
    ///     Mapa ordenado para un recorrido determinista.
    fn adjacency(&self) -> BTreeMap<u64, Vec<u64>> {
        let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for &node in &self.nodes {
            adjacency.entry(node).or_default();
        }
        for &(from, to) in &self.rw_edges {
            adjacency.entry(from).or_default().push(to);
        }
        adjacency
    }

    /// Detecta un ciclo en el grafo rw (DFS con colores).
    ///
    /// Returns:
    ///     `true` si existe un ciclo alcanzable.
    pub fn has_cycle(&self) -> bool {
        let adjacency = self.adjacency();
        // 0 = blanco, 1 = gris (en la pila), 2 = negro (cerrado).
        let mut color: HashMap<u64, u8> = HashMap::new();
        for &root in adjacency.keys() {
            if color.get(&root).copied().unwrap_or(0) != 0 {
                continue;
            }
            let mut stack: Vec<(u64, usize)> = vec![(root, 0)];
            color.insert(root, 1);
            while let Some((node, next_child)) = stack.pop() {
                let children = adjacency.get(&node).map(Vec::as_slice).unwrap_or(&[]);
                if next_child < children.len() {
                    stack.push((node, next_child + 1));
                    let child = children[next_child];
                    match color.get(&child).copied().unwrap_or(0) {
                        1 => return true,
                        0 => {
                            color.insert(child, 1);
                            stack.push((child, 0));
                        }
                        _ => {}
                    }
                } else {
                    color.insert(node, 2);
                }
            }
        }
        false
    }

    /// Detecta una estructura peligrosa: `T1 -rw-> T2 -rw-> T3`.
    ///
    /// Returns:
    ///     `true` si existe un pivote `T2` con entrada y salida rw.
    pub fn has_dangerous_structure(&self) -> bool {
        self.dangerous_pivot().is_some()
    }

    /// Devuelve el pivote de la primera estructura peligrosa encontrada.
    ///
    /// Returns:
    ///     `Some(tx)` con el pivote a abortar, o `None`.
    pub fn dangerous_pivot(&self) -> Option<u64> {
        let adjacency = self.adjacency();
        for (&pivot, successors) in &adjacency {
            if !successors.is_empty()
                && self
                    .rw_edges
                    .iter()
                    .any(|&(from, to)| to == pivot && from != pivot)
            {
                return Some(pivot);
            }
        }
        None
    }

    /// Decide si una transaccion debe abortar por SSI.
    ///
    /// Args:
    ///     tx: Transaccion candidata a commit.
    ///
    /// Returns:
    ///     `true` si es el pivote de una estructura peligrosa o participa en
    ///     un ciclo (conservador: aborta para preservar serializabilidad).
    pub fn should_abort(&self, tx: u64) -> bool {
        self.dangerous_pivot() == Some(tx) || self.has_cycle()
    }

    /// Comprueba si el historial actual es serializable.
    ///
    /// Returns:
    ///     `true` si no hay ciclos ni estructuras peligrosas.
    pub fn is_serializable(&self) -> bool {
        !self.has_cycle() && !self.has_dangerous_structure()
    }

    /// Elimina una transaccion y sus aristas (commit o abort).
    ///
    /// Args:
    ///     tx: Transaccion finalizada.
    pub fn retire(&mut self, tx: u64) {
        self.nodes.remove(&tx);
        self.rw_edges.retain(|&(from, to)| from != tx && to != tx);
    }
}

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
