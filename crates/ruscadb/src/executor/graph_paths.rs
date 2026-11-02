//! Navegación de grafo en la proyección `->label->tabla` / `<-label<-tabla`
//! (SPEC-0185).
//!
//! Cada path es una secuencia de saltos que se resuelve nivel a nivel (BFS):
//! se parte del registro origen, se siguen las aristas de la etiqueta pedida y
//! se deduplica por [`RecordId`] con un `BTreeSet`. Complejidad `O(V + E)` por
//! salto, sin producto de grados.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use ruscadb_query::{GraphPath, PathDirection};

use crate::edge_meta::EdgeMeta;

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
    source_table: &str,
    snapshot: &ruscadb_txn::Snapshot,
) -> Result<Vec<Vec<ScalarValue>>, RuscaError> {
    paths
        .iter()
        .map(|path| evaluate_path(database, path, records, source_table, snapshot))
        .collect()
}

/// Evalúa un path (uno o más saltos) sobre todos los registros.
fn evaluate_path(
    database: &mut Database,
    path: &GraphPath,
    records: &[Record],
    source_table: &str,
    snapshot: &ruscadb_txn::Snapshot,
) -> Result<Vec<ScalarValue>, RuscaError> {
    if path
        .hops
        .iter()
        .take(path.hops.len().saturating_sub(1))
        .any(|hop| !hop.edge_fields.is_empty())
    {
        return Err(RuscaError::InvalidConfig(
            "los campos de arista {payload, ts_unix} solo se admiten en el último salto del path"
                .to_string(),
        ));
    }
    let last_has_fields = path
        .hops
        .last()
        .is_some_and(|hop| !hop.edge_fields.is_empty());
    let mut values = Vec::with_capacity(records.len());
    for record in records {
        let mut current: Vec<(Record, String, Option<EdgeMeta>)> =
            vec![(record.clone(), source_table.to_string(), None)];
        for (hop_index, hop) in path.hops.iter().enumerate() {
            let is_last = hop_index + 1 == path.hops.len();
            let mut next = Vec::new();
            let mut seen = BTreeSet::new();
            for (source, current_table, _) in &current {
                let edges = match hop.direction {
                    PathDirection::Out => &source.edges.out,
                    PathDirection::In => &source.edges.incoming,
                };
                for edge in edges.iter().filter(|edge| edge.label == hop.label) {
                    if !seen.insert(edge.node) {
                        continue;
                    }
                    if let Some(target) = database.get_record(&hop.table, &edge.node)? {
                        let meta = if is_last && last_has_fields {
                            database.edge_meta_at(
                                current_table,
                                &source.id,
                                &hop.label,
                                &hop.table,
                                &target.id,
                                snapshot,
                            )?
                        } else {
                            None
                        };
                        next.push((target, hop.table.clone(), meta));
                    }
                }
            }
            current = next;
            if current.is_empty() {
                break;
            }
        }
        if last_has_fields {
            let objects: Vec<Value> = current
                .iter()
                .map(|(_, _, meta)| match meta {
                    Some(meta) => json!({
                        "payload": meta.payload,
                        "ts_unix": meta.ts_unix,
                    }),
                    None => json!({
                        "payload": Value::Null,
                        "ts_unix": Value::Null,
                    }),
                })
                .collect();
            if objects.is_empty() {
                values.push(ScalarValue::Null);
            } else if objects.len() == 1 {
                values.push(ScalarValue::Text(objects[0].to_string()));
            } else {
                values.push(ScalarValue::Text(Value::Array(objects).to_string()));
            }
        } else {
            let ids: Vec<String> = current
                .iter()
                .map(|(node, _, _)| node.id.to_string())
                .collect();
            values.push(if ids.is_empty() {
                ScalarValue::Null
            } else {
                ScalarValue::Text(ids.join(","))
            });
        }
    }
    Ok(values)
}
