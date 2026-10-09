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

use ruscadb_btree::BPlusTree;
use ruscadb_core::{Metric, Record, RecordId, RuscaError, ScalarValue};
use ruscadb_fts::InvertedIndex;
use ruscadb_fvs::{
    FvsStrategy, VectorSet, choose_strategy, search_auto_indexed, search_filtered, selectivity,
};
use ruscadb_graph::{CsrGraph, Direction, NodeId};
use ruscadb_vector::{HnswIndex, HnswParams};

use crate::catalog::{Catalog, ColumnType, TableDef};
use crate::database::Database;
use crate::heap::{RowLocator, heap_read, heap_scan};

/// Orden (máximo de hijos por nodo) del B+tree del índice primario.
const PRIMARY_INDEX_ORDER: usize = 8;

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
    /// `RecordId` por id de nodo HNSW (orden de inserción).
    ids: Vec<RecordId>,
    /// Copia de los vectores indexados (paralela a `ids`), para FVS (SPEC-0024).
    vectors: Vec<Vec<f32>>,
    /// Métrica del espacio vectorial (fijada por el primer embedding).
    metric: Option<Metric>,
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

    /// Busca el top-k exacto restringido a `allowed` usando FVS (SPEC-0024).
    ///
    /// Construye el corpus vectorial de la tabla (sin las entradas borradas) y
    /// delega en FVS, que elige la estrategia por selectividad
    /// (`PreFilter`/`InFilter`/`PostFilter`). `PostFilter` es sonido pero puede
    /// devolver menos de `k` ítems; en ese caso se recalcula de forma exacta
    /// (`PreFilter`) para garantizar NF-0024-01.
    ///
    /// Args:
    ///     query: Vector de consulta (misma dimensión que el índice).
    ///     k: Número máximo de resultados.
    ///     allowed: `RecordId` que sobreviven al filtro `WHERE`.
    ///
    /// Returns:
    ///     Hasta `k` pares `(RecordId, distancia)` ordenados por distancia
    ///     ascendente; solo ids en `allowed`.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si `query` no coincide con la
    ///     dimensión del corpus.
    pub(crate) fn filtered_vector_search(
        &self,
        query: &[f32],
        k: usize,
        allowed: &BTreeSet<RecordId>,
    ) -> Result<Vec<(RecordId, f32)>, RuscaError> {
        let Some(metric) = self.vector.metric else {
            return Ok(Vec::new());
        };
        let (set, node_of) = self.vector_corpus(metric)?;
        let allowed_nodes: BTreeSet<u64> = allowed
            .iter()
            .filter_map(|id| node_of.get(id).copied())
            .collect();
        let hits = fvs_top_k(&set, query, k, &allowed_nodes)?;
        Ok(hits
            .into_iter()
            .filter_map(|(node, distance)| {
                self.vector
                    .ids
                    .get(node as usize)
                    .copied()
                    .map(|id| (id, distance))
            })
            .collect())
    }

    /// Busca el top-k restringido a `allowed` con iFVS sobre el índice HNSW.
    ///
    /// Cablea `ruscadb_fvs::search_auto_indexed` (SPEC-0047) sobre el
    /// `HnswIndex` real de la tabla (SPEC-0048). La estrategia se elige por
    /// selectividad `s = |allowed| / total`:
    ///
    /// - `PreFilter` (`s < 0.05`): fuerza bruta restringida; exacta.
    /// - `InFilter` (`0.05 <= s < 0.6`): iFVS (in-filter vector search,
    ///   arXiv:2607.22922) sobre el grafo; exacta cuando la amplitud de
    ///   búsqueda cubre el corpus (corpus pequeños/tests).
    /// - `PostFilter` (`s >= 0.6`): top-sobre-muestreado y filtro; sonora.
    ///
    /// En cualquier estrategia, si el resultado no alcanza `min(k, |allowed|)`
    /// (el vecindario cercano quedó fuera del filtro) se recalcula de forma
    /// exacta; si sí lo alcanza, el resultado es ya el top-k permitido exacto.
    ///
    /// Args:
    ///     query: Vector de consulta (misma dimensión que el índice).
    ///     k: Número máximo de resultados.
    ///     allowed: `RecordId` que sobreviven al filtro `WHERE`.
    ///
    /// Returns:
    ///     Hasta `k` pares `(RecordId, distancia)` ordenados por distancia
    ///     ascendente; solo ids en `allowed`.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si `query` no coincide con la
    ///     dimensión del índice.
    pub(crate) fn ifvs_vector_search(
        &self,
        query: &[f32],
        k: usize,
        allowed: &BTreeSet<RecordId>,
    ) -> Result<Vec<(RecordId, f32)>, RuscaError> {
        let Some(index) = self.vector.index.as_ref() else {
            return Ok(Vec::new());
        };
        if k == 0 || allowed.is_empty() {
            return Ok(Vec::new());
        }
        let node_of = self.node_map();
        let allowed_nodes: BTreeSet<u64> = allowed
            .iter()
            .filter_map(|id| node_of.get(id).copied())
            .collect();
        let total = index.len();
        let mut hits = search_auto_indexed(index, query, k, &allowed_nodes, total)?;
        // Si `search_auto_indexed` no alcanza `min(k, |allowed|)`, su resultado
        // no cubre el top-k permitido (el vecindario cercano quedó filtrado):
        // se recalcula de forma exacta. Alcanzarlo implica exactitud, porque el
        // top-`4k` global contiene los `k` permitidos más cercanos.
        let target = k.min(allowed_nodes.len());
        if hits.len() < target {
            hits = exact_indexed_top_k(index, query, k, &allowed_nodes)?;
        } else {
            hits.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        }
        Ok(hits
            .into_iter()
            .filter_map(|(node, distance)| {
                self.vector
                    .ids
                    .get(node as usize)
                    .copied()
                    .map(|id| (id, distance))
            })
            .collect())
    }

    /// Mapa `RecordId -> id de nodo HNSW` (orden de inserción).
    ///
    /// Returns:
    ///     Un mapa de cada `RecordId` indexado a su id de nodo HNSW.
    fn node_map(&self) -> BTreeMap<RecordId, u64> {
        self.vector
            .ids
            .iter()
            .enumerate()
            .map(|(node, id)| (*id, node as u64))
            .collect()
    }

    /// Construye el corpus FVS de la tabla, omitiendo las entradas borradas.
    ///
    /// Args:
    ///     metric: Métrica del espacio vectorial.
    ///
    /// Returns:
    ///     El [`VectorSet`] (ids = índice de nodo) y el mapa `RecordId -> nodo`.
    ///
    /// Errors:
    ///     [`RuscaError::DimensionMismatch`] si los vectores del índice no son
    ///     homogéneos (no ocurre: HNSW los valida al insertar).
    fn vector_corpus(
        &self,
        metric: Metric,
    ) -> Result<(VectorSet, BTreeMap<RecordId, u64>), RuscaError> {
        let mut set = VectorSet::new(metric);
        let mut node_of = BTreeMap::new();
        for (node, (id, vector)) in self
            .vector
            .ids
            .iter()
            .zip(self.vector.vectors.iter())
            .enumerate()
        {
            if self.deleted.contains(id) {
                continue;
            }
            set.insert(node as u64, vector.clone())?;
            node_of.insert(*id, node as u64);
        }
        Ok((set, node_of))
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

    /// Reindexa el texto de un registro actualizado (sin tombstone).
    ///
    /// Retira el `RecordId` de todos los índices invertidos y lo reinserta con el
    /// texto actual del registro. Una actualización escalar no altera el vector
    /// ni las aristas, así que los índices HNSW y CSR no cambian (SPEC-0043).
    ///
    /// Args:
    ///     record: Registro actualizado (mismo `id`).
    ///     table: Definición de la tabla (columnas `TEXT`).
    pub(crate) fn refresh_text(&mut self, record: &Record, table: &TableDef) {
        for index in self.fts.values_mut() {
            index.remove(&record.id);
        }
        self.index_text(record, table);
    }

    /// Inserta el embedding del registro en el índice HNSW.
    fn index_vector(&mut self, record: &Record) -> Result<(), RuscaError> {
        let Some(embedding) = record.vector.as_ref() else {
            return Ok(());
        };
        if self.vector.index.is_none() {
            let params = HnswParams::new(embedding.meta.metric);
            self.vector.index = Some(HnswIndex::new(params, embedding.values.len())?);
            self.vector.metric = Some(embedding.meta.metric);
        }
        if let Some(index) = self.vector.index.as_mut() {
            index.insert(&embedding.values)?;
            self.vector.ids.push(record.id);
            self.vector.vectors.push(embedding.values.clone());
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

/// Top-k exacto restringido al filtro sobre el índice HNSW (fallback de post).
///
/// Recupera el corpus completo del índice (`k = ef = n`) y conserva solo los
/// nodos permitidos; equivale a la estrategia `PreFilter` exacta sobre el
/// índice (SPEC-0047) y evita que `PostFilter` pierda vecinos válidos.
///
/// Args:
///     index: Índice HNSW a recorrer.
///     query: Vector de consulta (misma dimensión que el índice).
///     k: Número máximo de resultados.
///     allowed: Ids de nodo permitidos por el filtro `WHERE`.
///
/// Returns:
///     Hasta `k` pares `(id, distancia)` exactos y sonoros.
///
/// Errors:
///     [`RuscaError::DimensionMismatch`] si `query` no coincide con la
///     dimensión del índice.
fn exact_indexed_top_k(
    index: &HnswIndex,
    query: &[f32],
    k: usize,
    allowed: &BTreeSet<u64>,
) -> Result<Vec<(u64, f32)>, RuscaError> {
    let total = index.len();
    if total == 0 {
        return Ok(Vec::new());
    }
    let candidates = index.search(query, total, total)?;
    let mut hits: Vec<(u64, f32)> = candidates
        .into_iter()
        .filter(|(id, _)| allowed.contains(&(*id as u64)))
        .map(|(id, distance)| (id as u64, distance))
        .collect();
    // Desempate estable por id de nodo (mismo criterio que FVS plano); HNSW
    // devuelve las distancias ordenadas pero no fija el orden de los empates.
    hits.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    hits.truncate(k);
    Ok(hits)
}

/// Top-k exacto restringido al filtro con FVS (SPEC-0024).
///
/// Elige la estrategia por selectividad (`s = |allowed| / total`). `PreFilter`
/// e `InFilter` son exactos; `PostFilter` es sonido y, si devuelve menos de `k`
/// ítems, se recalcula con `PreFilter` para no perder vecinos válidos.
///
/// Args:
///     set: Corpus vectorial de la tabla.
///     query: Vector de consulta.
///     k: Número máximo de resultados.
///     allowed: Ids de nodo permitidos por el filtro `WHERE`.
///
/// Returns:
///     Hasta `k` pares `(id, distancia)` ordenados por distancia ascendente.
///
/// Errors:
///     [`RuscaError::DimensionMismatch`] si `query` no coincide con el corpus.
fn fvs_top_k(
    set: &VectorSet,
    query: &[f32],
    k: usize,
    allowed: &BTreeSet<u64>,
) -> Result<Vec<(u64, f32)>, RuscaError> {
    let strategy = choose_strategy(selectivity(allowed.len(), set.len()));
    let hits = search_filtered(set, query, k, allowed, strategy)?;
    if strategy == FvsStrategy::PostFilter && hits.len() < k {
        return search_filtered(set, query, k, allowed, FvsStrategy::PreFilter);
    }
    Ok(hits)
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

    /// Reconstruye el índice primario `RecordId -> RowLocator` desde el heap.
    ///
    /// Se invoca al abrir la base (durabilidad, FR-0024-04): el índice primario
    /// es derivado del heap y no se persiste. Solo se indexan las versiones
    /// vivas (sin `deleted_tx`), por lo que un id borrado no reaparece en el
    /// point lookup tras reabrir.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si el catálogo referencia una tabla
    ///     ausente; [`RuscaError::CorruptManifest`] si el heap es inválido.
    pub(crate) fn rebuild_primary_index(&mut self) -> Result<(), RuscaError> {
        let catalog = Catalog::load(self)?;
        let mut primary = BTreeMap::new();
        for name in catalog.table_names() {
            let table = catalog.get(&name)?.clone();
            let mut tree = BPlusTree::new(PRIMARY_INDEX_ORDER)?;
            for (locator, record) in heap_scan(self, &table)? {
                if record.meta.deleted_tx.is_some() {
                    continue;
                }
                tree.insert(record.id, locator);
            }
            primary.insert(name, tree);
        }
        self.primary = primary;
        Ok(())
    }

    /// Registra (o actualiza) el localizador de `id` en el índice primario.
    ///
    /// Args:
    ///     table: Tabla dueña de la fila.
    ///     id: Identificador de la fila.
    ///     locator: Localizador físico `(PageId, slot)`.
    ///
    /// Errors:
    ///     [`RuscaError::InvalidConfig`] si el orden del B+tree fuese inválido
    ///     (no ocurre con [`PRIMARY_INDEX_ORDER`]).
    pub(crate) fn primary_insert(
        &mut self,
        table: &str,
        id: RecordId,
        locator: RowLocator,
    ) -> Result<(), RuscaError> {
        if !self.primary.contains_key(table) {
            let tree = BPlusTree::new(PRIMARY_INDEX_ORDER)?;
            self.primary.insert(table.to_string(), tree);
        }
        if let Some(tree) = self.primary.get_mut(table) {
            tree.insert(id, locator);
        }
        Ok(())
    }

    /// Localiza en el índice primario el localizador de `id`, si existe.
    ///
    /// Args:
    ///     table: Tabla dueña de la fila.
    ///     id: Identificador buscado.
    ///
    /// Returns:
    ///     El localizador `(PageId, slot)`, o `None` si no hay índice o entrada.
    pub(crate) fn primary_lookup(&self, table: &str, id: &RecordId) -> Option<RowLocator> {
        self.primary
            .get(table)
            .and_then(|tree| tree.get(id))
            .copied()
    }

    /// Retira la entrada de `id` del índice primario (borrado lógico).
    ///
    /// Args:
    ///     table: Tabla dueña de la fila.
    ///     id: Identificador borrado.
    pub(crate) fn primary_remove(&mut self, table: &str, id: &RecordId) {
        if let Some(tree) = self.primary.get_mut(table) {
            tree.remove(id);
        }
    }

    /// Recupera una fila viva por su identificador (point lookup, SPEC-0024).
    ///
    /// Usa el índice primario (B+tree, `O(log n)`); si la tabla no tiene
    /// entrada en el índice, recurre a un scan del heap como *fallback*
    /// documentado. Una fila borrada lógicamente no se devuelve.
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///     id: Identificador de la fila.
    ///
    /// Returns:
    ///     `Some(record)` si la fila existe y está viva; `None` en otro caso.
    ///
    /// Errors:
    ///     [`RuscaError::TableNotFound`] si la tabla no existe;
    ///     [`RuscaError::CorruptManifest`] si el heap es inválido.
    pub fn get_record(&mut self, table: &str, id: &RecordId) -> Result<Option<Record>, RuscaError> {
        let definition = Catalog::load(self)?.get(table)?.clone();
        if let Some(locator) = self.primary_lookup(table, id) {
            let record = heap_read(self, locator)?;
            return Ok((record.meta.deleted_tx.is_none()).then_some(record));
        }
        for (_, record) in heap_scan(self, &definition)? {
            if record.id == *id && record.meta.deleted_tx.is_none() {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    /// Número de entradas del índice primario de una tabla (0 si no existe).
    ///
    /// Args:
    ///     table: Nombre de la tabla.
    ///
    /// Returns:
    ///     La cardinalidad del B+tree primario.
    pub fn primary_index_len(&self, table: &str) -> usize {
        self.primary.get(table).map_or(0, |tree| tree.len())
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
        let allowed = BTreeSet::from([victim, survivor]);

        assert_eq!(
            indexes
                .filtered_vector_search(&[0.0, 0.0, 0.0], 2, &allowed)
                .expect("knn")
                .len(),
            2
        );
        assert_eq!(indexes.text_search("name", "gato", 10).len(), 2);

        indexes.remove_record(&sample_record(victim));

        let nearest = indexes
            .filtered_vector_search(&[0.0, 0.0, 0.0], 2, &allowed)
            .expect("knn");
        assert!(
            !nearest.iter().any(|(id, _)| *id == victim)
                && nearest.iter().any(|(id, _)| *id == survivor),
            "HNSW excluye al borrado y conserva al vivo"
        );
        let hits = indexes.text_search("name", "gato", 10);
        assert!(
            !hits.contains(&victim) && hits.contains(&survivor),
            "el índice invertido excluye al borrado y conserva al vivo"
        );
    }
}
