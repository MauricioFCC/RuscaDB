//! Navegación de grafo en la proyección `->label->tabla` / `<-label<-tabla`
//! (SPEC-0185).
//!
//! Cada path es una secuencia de saltos que se resuelve nivel a nivel (BFS):
//! se parte del registro origen, se siguen las aristas de la etiqueta pedida y
//! se deduplica por [`RecordId`] con un `BTreeSet`. Complejidad `O(V + E)` por
//! salto, sin producto de grados.

use std::collections::BTreeSet;

use ruscadb_query::{GraphPath, PathDirection};

use super::*;

/// Evalúa todos los paths de la proyección.
///
/// Args:
///     database: Base abierta (para leer los nodos intermedios).
///     paths: Paths en el orden de la proyección.
///     records: Registros origen.
///
/// Returns:
///     Un vector de valores por path, alineado con `records`; cada valor es el
///     `RecordId` del vecino final (o una lista separada por comas si hay
///     varios) o `NULL` si no hay camino con esas etiquetas.
///
/// Errors:
///     Los de lectura de las tablas destino.
pub(crate) fn evaluate_paths(
    database: &mut Database,
    paths: &[GraphPath],
    records: &[Record],
) -> Result<Vec<Vec<ScalarValue>>, RuscaError> {
    paths
        .iter()
        .map(|path| evaluate_path(database, path, records))
        .collect()
}

/// Evalúa un path (uno o más saltos) sobre todos los registros.
fn evaluate_path(
    database: &mut Database,
    path: &GraphPath,
    records: &[Record],
) -> Result<Vec<ScalarValue>, RuscaError> {
    let mut values = Vec::with_capacity(records.len());
    for record in records {
        let mut current = vec![record.clone()];
        for hop in &path.hops {
            let mut next = Vec::new();
            let mut seen = BTreeSet::new();
            for source in &current {
                let edges = match hop.direction {
                    PathDirection::Out => &source.edges.out,
                    PathDirection::In => &source.edges.incoming,
                };
                for edge in edges.iter().filter(|edge| edge.label == hop.label) {
                    if !seen.insert(edge.node) {
                        continue;
                    }
                    if let Some(target) = database.get_record(&hop.table, &edge.node)? {
                        next.push(target);
                    }
                }
            }
            current = next;
            if current.is_empty() {
                break;
            }
        }
        let ids: Vec<String> = current.iter().map(|node| node.id.to_string()).collect();
        values.push(if ids.is_empty() {
            ScalarValue::Null
        } else {
            ScalarValue::Text(ids.join(","))
        });
    }
    Ok(values)
}
