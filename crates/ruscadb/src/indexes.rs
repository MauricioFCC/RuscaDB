//! Índices por tabla mantenidos en memoria por la fachada (SPEC-0017).
//!
//! Para cada tabla se conservan tres estructuras derivadas de los `Record`:
//!
//! - un [`HnswIndex`] con los embeddings (`record.vector`), mapeado a
//!   [`RecordId`] por orden de inserción;
//! - un [`CsrGraph`] con las aristas salientes (`record.edges.out`), con una
//!   numeración densa de nodos (`NodeId`) ↔ [`RecordId`];
//! - un [`InvertedIndex`] por cada columna `TEXT` del esquema, indexando el
//!   escalar de texto de esa columna.
//!
//! Los índices son **derivados del heap**: no se serializan; al abrir la base
//! se reconstruyen escaneando las tablas ([`Database::rebuild_indexes`]), lo que
//! garantiza durabilidad sin un formato de índice persistente (fuera de
//! alcance de SPEC-0017).

use std::collections::{BTreeMap, BTreeSet};

use ruscadb_core::{Record, RecordId, RuscaError, ScalarValue};
use ruscadb_fts::InvertedIndex;
use ruscadb_graph::{CsrGraph, Direction, NodeId};
use ruscadb_vector::{HnswIndex, HnswParams};

use crate::catalog::{Catalog, ColumnType, TableDef};
use crate::database::Database;
use crate::heap::heap_scan;

/// Índices derivados de una tabla (vector, grafo y full-text por columna).
#[derive(Default)]
pub(crate) struct TableIndexes {
    vector: VectorIndex,
    graph: GraphIndex,
    fts: BTreeMap<String, InvertedIndex>,
    /// `RecordId` borrados lógicamente (tombstone): quedan fuera de los índices.
    deleted: BTreeSet<RecordId>,
}

/// Índice vectorial + mapeo de ids de nodo HNSW a `RecordId`.
#[derive(Default)]
struct VectorIndex {
    index: Option<HnswIndex>,
    ids: Vec<RecordId>,
}

/// Grafo CSR + mapeo bidireccional `RecordId` ↔ `NodeId`.
#[derive(Default)]
struct GraphIndex {
    graph: CsrGraph,
    to_node: BTreeMap<RecordId, NodeId>,
    from_node: Vec<RecordId>,
}

impl TableIndexes {
    /// Indica si la tabla tiene al menos un vector indexado.
    ///
    /// Returns:
    ///     `true` si hay embeddings en el índice HNSW.
    pub(crate) fn has_vector(&self) -> bool {
        self.vector
            .index
            .as_ref()
            .is_some_and(|index| !index.is_empty())
    }

    /// Indica si la tabla tiene al menos un nodo en el grafo (alguna arista).
    ///
    /// Returns:
    ///     `true` si el CSR tiene nodos.
    pub(crate) fn has_graph(&self) -> bool {
        self.graph.graph.node_count() > 0
    }

    /// Busca todos los vectores indexados por cercanía a `query`.
    ///
    /// Args:
    ///     query: Vector de consulta (misma dimensión que el índice).
    ///
    /// Returns:
    ///     `RecordId` en orden de distancia ascendente (el más cercano primero).
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si la dimensión no coincide.
    pub(crate) fn vector_search(&self, query: &[f32]) -> Result<Vec<RecordId>, RuscaError> {
        let Some(index) = self.vector.index.as_ref() else {
            return Ok(Vec::new());
        };
        let total = index.len();
        if total == 0 {
            return Ok(Vec::new());
        }
        let hits = index.search(query, total, total)?;
        Ok(hits
            .into_iter()
            .filter_map(|(node, _)| self.vector.ids.get(node).copied())
            .filter(|id| !self.deleted.contains(id))
            .collect())
    }

    /// Busca en el índice invertido de la columna `column`.
    ///
    /// Args:
    ///     column: Columna `TEXT` indexada.
    ///     query: Texto de la consulta (se tokeniza).
    ///     k: Número máximo de documentos.
    ///
    /// Returns:
    ///     `RecordId` ordenados por BM25 descendente; vacío si no hay índice.
    pub(crate) fn text_search(&self, column: &str, query: &str, k: usize) -> Vec<RecordId> {
        self.fts
            .get(column)
            .map(|index| {
                index
                    .search(query, k)
                    .into_iter()
                    .map(|(id, _)| id)
                    .filter(|id| !self.deleted.contains(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Calcula las semillas de un `TRAVERSE` a partir de los candidatos.
    ///
    /// Las semillas son los candidatos **raíz** (sin aristas entrantes); si
    /// ninguno es raíz (p. ej. un ciclo) se usan todos los candidatos que
    /// pertenecen al grafo. Los candidatos sin aristas quedan fuera.
    ///
    /// Args:
    ///     candidates: `RecordId` que sobreviven al `WHERE`, en orden.
    ///
    /// Returns:
    ///     Semillas en el orden recibido.
    pub(crate) fn traversal_seeds(&self, candidates: &[RecordId]) -> Vec<RecordId> {
        let roots: Vec<RecordId> = candidates
            .iter()
            .copied()
            .filter(|id| !self.deleted.contains(id) && self.is_root(id))
            .collect();
        if !roots.is_empty() {
            return roots;
        }
        candidates
            .iter()
            .copied()
            .filter(|id| !self.deleted.contains(id) && self.graph.to_node.contains_key(id))
            .collect()
    }

    /// Recorre el grafo desde `seeds` hasta `depth` y devuelve los registros.
    ///
    /// Args:
    ///     seeds: Nodos de origen (raíces del grafo).
    ///     depth: Profundidad máxima del recorrido (`DEPTH`).
    ///
    /// Returns:
    ///     `RecordId` alcanzables en orden BFS, sin duplicados.
    pub(crate) fn traverse(&self, seeds: &[RecordId], depth: u16) -> Vec<RecordId> {
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();
        for seed in seeds {
            let Some(&node) = self.graph.to_node.get(seed) else {
                continue;
            };
            for reached in self
                .graph
                .graph
                .traverse(node, Direction::Out, depth, usize::MAX)
            {
                if let Some(record) = self.graph.from_node.get(reached as usize).copied() {
                    if !self.deleted.contains(&record) && seen.insert(record) {
                        ordered.push(record);
                    }
                }
            }
        }
        ordered
    }

    /// Indexa un registro recién insertado en los tres modelos.
    ///
    /// Args:
    ///     record: Registro completo (escalares, vector y aristas).
    ///     table: Definición de la tabla (para localizar columnas `TEXT`).
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si un vector no coincide con la
    ///     dimensión del índice de la tabla.
    pub(crate) fn index_record(
        &mut self,
        record: &Record,
        table: &TableDef,
    ) -> Result<(), RuscaError> {
        self.index_vector(record)?;
        self.index_graph(record);
        self.index_text(record, table);
        Ok(())
    }

    /// Compila el CSR acumulado (idempotente; llamar tras indexar).
    pub(crate) fn finalize(&mut self) {
        self.graph.graph.build();
    }

    /// Retira un registro de los índices derivados (borrado lógico).
    ///
    /// Marca su `RecordId` como tombstone y lo elimina del índice invertido; las
    /// búsquedas vectorial y de grafo lo filtran por el tombstone para no
    /// devolverlo (SPEC-0022, FR-0022-02).
    ///
    /// Args:
    ///     record: Registro borrado (se usa su `id`).
    pub(crate) fn remove_record(&mut self, record: &Record) {
        self.deleted.insert(record.id);
        for index in self.fts.values_mut() {
            index.remove(&record.id);
        }
    }

    /// Inserta el embedding del registro en el índice HNSW.
    fn index_vector(&mut self, record: &Record) -> Result<(), RuscaError> {
        let Some(embedding) = record.vector.as_ref() else {
            return Ok(());
        };
        if self.vector.index.is_none() {
            let params = HnswParams::new(embedding.meta.metric);
            self.vector.index = Some(HnswIndex::new(params, embedding.values.len())?);
        }
        if let Some(index) = self.vector.index.as_mut() {
            index.insert(&embedding.values)?;
            self.vector.ids.push(record.id);
        }
        Ok(())
    }

    /// Añade las aristas salientes del registro al grafo (sin compilar).
    fn index_graph(&mut self, record: &Record) {
        for edge in &record.edges.out {
            let from = self.graph.node_id(record.id);
            let to = self.graph.node_id(edge.node);
            self.graph.graph.add_edge(from, to);
        }
    }

    /// Indexa el escalar de texto de cada columna `TEXT` del esquema.
    fn index_text(&mut self, record: &Record, table: &TableDef) {
        for column in &table.columns {
            if column.col_type != ColumnType::Text {
                continue;
            }
            let Some(ScalarValue::Text(text)) = record.scalars.get(&column.name) else {
                continue;
            };
            self.fts
                .entry(column.name.clone())
                .or_default()
                .insert(record.id, text);
        }
    }

    /// Indica si el registro es raíz (sin aristas entrantes en el grafo).
    fn is_root(&self, id: &RecordId) -> bool {
        self.graph
            .to_node
            .get(id)
            .is_some_and(|node| self.graph.graph.neighbors(*node, Direction::In).is_empty())
    }
}

impl GraphIndex {
    /// Devuelve el `NodeId` de `id`, asignándolo si aún no existe.
    ///
    /// Args:
    ///     id: Identificador de registro referenciado por una arista.
    ///
    /// Returns:
    ///     La numeración densa asignada al registro.
    fn node_id(&mut self, id: RecordId) -> NodeId {
        if let Some(&node) = self.to_node.get(&id) {
            return node;
        }
        let node = self.from_node.len() as NodeId;
        self.from_node.push(id);
        self.to_node.insert(id, node);
        node
    }
}

impl Database {
    /// Reconstruye los índices en memoria escaneando todas las tablas.
    ///
    /// Se invoca al abrir la base (durabilidad): los índices son derivados del
    /// heap y no se persisten (fuera de alcance de SPEC-0017).
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] / [`RuscaError::DimensionMismatch`]
    ///     si el heap contiene datos incoherentes con el catálogo.
    pub(crate) fn rebuild_indexes(&mut self) -> Result<(), RuscaError> {
        let catalog = Catalog::load(self)?;
        let mut indexes = BTreeMap::new();
        for name in catalog.table_names() {
            let table = catalog.get(&name)?.clone();
            let mut table_indexes = TableIndexes::default();
            for (_, record) in heap_scan(self, &table)? {
                // Las versiones borradas no entran en los índices derivados
                // (SPEC-0022): solo se indexa la versión viva.
                if record.meta.deleted_tx.is_some() {
                    continue;
                }
                table_indexes.index_record(&record, &table)?;
            }
            table_indexes.finalize();
            indexes.insert(name, table_indexes);
        }
        self.indexes = indexes;
        Ok(())
    }

    /// Restaura el agua (*watermark*) del gestor MVCC tras reabrir la base.
    ///
    /// Los registros persistidos conservan su `created_tx`/`deleted_tx` de la
    /// sesión anterior, pero [`TxnManager`] arranca en el primer `TxId`. Para
    /// que los commits previos sigan siendo visibles (sin dirty reads) se
    /// reconstruye el conjunto confirmado: se reservan y publican tantos `TxId`
    /// como indique el mayor `created_tx`/`deleted_tx` observado (SPEC-0019).
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si el catálogo referencia una tabla
    ///     ausente; [`RuscaError::CorruptManifest`] si el heap es inválido.
    pub(crate) fn restore_txn_watermark(&mut self) -> Result<(), RuscaError> {
        let catalog = Catalog::load(self)?;
        let mut max_tx = 0;
        for name in catalog.table_names() {
            let table = catalog.get(&name)?.clone();
            for (_, record) in heap_scan(self, &table)? {
                max_tx = max_tx.max(record.meta.created_tx);
                if let Some(deleted) = record.meta.deleted_tx {
                    max_tx = max_tx.max(deleted);
                }
            }
        }
        for _ in 0..max_tx {
            let tx = self.txn.begin();
            self.txn.commit(tx)?;
        }
        Ok(())
    }

    /// Acceso de solo lectura a los índices de una tabla.
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///
    /// Returns:
    ///     Los índices, o `None` si la tabla no tiene entrada en memoria.
    pub(crate) fn table_indexes(&self, table: &str) -> Option<&TableIndexes> {
        self.indexes.get(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ColumnDef;
    use ruscadb_core::{Embedding, EmbeddingMeta, Metric, RecordMeta, ScalarMap};
    use ruscadb_storage::PageId;

    /// Tabla mínima con una columna `TEXT` (para el índice invertido).
    fn table_def() -> TableDef {
        TableDef {
            name: "t".to_string(),
            columns: vec![ColumnDef {
                name: "name".to_string(),
                col_type: ColumnType::Text,
            }],
            heap_start: PageId(16),
            pages: vec![16],
            row_count: 0,
            index: None,
        }
    }

    /// Registro con vector (HNSW) y texto (invertido), sin aristas.
    fn sample_record(id: RecordId) -> Record {
        Record {
            id,
            scalars: ScalarMap::from([("name".to_string(), ScalarValue::Text("gato".to_string()))]),
            doc: None,
            edges: ruscadb_core::EdgeSet::default(),
            vector: Some(
                Embedding::new(
                    vec![0.0, 0.0, 0.0],
                    EmbeddingMeta {
                        model_id: "test".to_string(),
                        dim: 3,
                        metric: Metric::L2,
                    },
                )
                .expect("embedding válido"),
            ),
            blob: None,
            meta: RecordMeta::default(),
        }
    }

    /// SPEC-0022 — `remove_record` excluye el id de los índices vectorial y de
    /// texto (tombstone) sin afectar a las filas vivas.
    #[test]
    fn test_ac_0022_remove_record_clears_derived_indexes() {
        let table = table_def();
        let victim = RecordId::new();
        let survivor = RecordId::new();
        let mut indexes = TableIndexes::default();
        indexes
            .index_record(&sample_record(victim), &table)
            .expect("index victim");
        indexes
            .index_record(&sample_record(survivor), &table)
            .expect("index survivor");
        indexes.finalize();

        assert_eq!(
            indexes.vector_search(&[0.0, 0.0, 0.0]).expect("knn").len(),
            2
        );
        assert_eq!(indexes.text_search("name", "gato", 10).len(), 2);

        indexes.remove_record(&sample_record(victim));

        let nearest = indexes.vector_search(&[0.0, 0.0, 0.0]).expect("knn");
        assert!(
            !nearest.contains(&victim) && nearest.contains(&survivor),
            "HNSW excluye al borrado y conserva al vivo"
        );
        let hits = indexes.text_search("name", "gato", 10);
        assert!(
            !hits.contains(&victim) && hits.contains(&survivor),
            "el índice invertido excluye al borrado y conserva al vivo"
        );
    }
}
