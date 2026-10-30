//! Serializable Snapshot Isolation (SSI) con grafo de dependencias rw (SPEC-0170).
//!
//! Implementacion del grafo que usa [`TxnManager`](crate::TxnManager) para
//! detectar anomalias de serializacion sin bloquear lectores: aristas
//! `write -> read` (rw), deteccion de ciclos por DFS y de la **estructura
//! peligrosa** (dos aristas rw consecutivas con el mismo pivote).
//!
//! Referencia: Cahill, Rohm y Fekete, "Serializable Isolation for Snapshot
//! Databases", SIGMOD 2009.

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
