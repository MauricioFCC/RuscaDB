//! # ruscadb-wasm
//!
//! Bindings **WebAssembly** de RuscaDB: expone el parser RQL, la búsqueda
//! vectorial (HNSW/FVS) y el full-text (BM25) para entornos JS/WASM, con una
//! API serializable en JSON. Especificación: `specs/wasm_bindings.md` (SPEC-0023).
//!
//! Diseño: `docs/RuscaDB-roadmap.md` §4.3 y ADR-011. El motor no requiere red ni
//! servidor: es apto para ejecutarse en el navegador vía WASM.
//!
//! La capa JS (`wasm-bindgen`) vive tras la feature `wasm`; la lógica de
//! serialización y búsqueda es testeable en el host sin esa feature ni el
//! target `wasm32` (NF-0023-02).
//!
//! ## Esquema JSON
//!
//! - `parse_query_json` → `{"ok":true,"statement":{...}}` o
//!   `{"ok":false,"error":"..."}`. El `statement` es `{"kind":"select",...}` o
//!   `{"kind":"explain","inner":{...select...}}`.
//! - `vector_search_json` → `{"ok":true,"hits":[{"id":1,"distance":..}]}`.
//! - `text_search_json` → `{"ok":true,"hits":[{"id":1,"score":..}]}`.
//! - `seal_json` → `{"ok":true,"nonce":"<hex>","ciphertext":"<hex>"}`.
//! - `open_json` → `{"ok":true,"plaintext":"<hex>"}`.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::mem::size_of;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use ruscadb_core::{Metric, RecordId};
use ruscadb_crypto::{KEY_SIZE, NONCE_SIZE, open, seal};
use ruscadb_fts::InvertedIndex;
use ruscadb_query::{
    Expr, KnnClause, Projection, Select, Statement, TraverseClause, parse_statement,
};
use ruscadb_vector::distance;
use serde_json::{Value, json};

/// Analiza una consulta RQL y devuelve el IR serializado como JSON.
///
/// Args:
///     sql: Texto de la sentencia RQL (`SELECT` o `EXPLAIN <select>`).
///
/// Returns:
///     `{"ok":true,"statement":{...}}` con el IR, o
///     `{"ok":false,"error":"..."}` si la consulta es inválida.
pub fn parse_query_json(sql: &str) -> String {
    match parse_statement(sql) {
        Ok(statement) => json!({ "ok": true, "statement": statement_json(&statement) }).to_string(),
        Err(error) => error_json(&error.to_string()),
    }
}

/// Busca los `k` vecinos más cercanos de `query_json` en `corpus_json`.
///
/// Usa distancia euclídea (L2) mediante [`ruscadb_vector::distance`].
///
/// Args:
///     corpus_json: Arreglo `[{"id":1,"vector":[..]}, ...]`.
///     query_json: Vector de consulta `[..]`.
///     k: Número máximo de resultados (`k == 0` devuelve vacío).
///
/// Returns:
///     `{"ok":true,"hits":[{"id":1,"distance":..}, ...]}` ordenado por
///     distancia ascendente, o `{"ok":false,"error":"..."}`.
pub fn vector_search_json(corpus_json: &str, query_json: &str, k: usize) -> String {
    let corpus = match parse_json(corpus_json) {
        Ok(value) => value,
        Err(error) => return error_json(&error),
    };
    let query = match parse_json(query_json) {
        Ok(value) => value,
        Err(error) => return error_json(&error),
    };
    match search_vectors(&corpus, &query, k) {
        Ok(hits) => json!({ "ok": true, "hits": hits }).to_string(),
        Err(error) => error_json(&error),
    }
}

/// Busca `query` en `docs_json` con BM25 y devuelve los mejores documentos.
///
/// Args:
///     docs_json: Arreglo `[{"id":1,"text":".."}, ...]`.
///     query: Texto de la consulta full-text.
///     k: Número máximo de resultados (`k == 0` devuelve vacío).
///
/// Returns:
///     `{"ok":true,"hits":[{"id":1,"score":..}, ...]}` ordenado por BM25
///     descendente, o `{"ok":false,"error":"..."}`.
pub fn text_search_json(docs_json: &str, query: &str, k: usize) -> String {
    let docs = match parse_json(docs_json) {
        Ok(value) => value,
        Err(error) => return error_json(&error),
    };
    let Some(array) = docs.as_array() else {
        return error_json("docs debe ser un arreglo JSON");
    };
    let mut index = InvertedIndex::new();
    let mut labels: HashMap<RecordId, i64> = HashMap::new();
    for entry in array {
        if let Err(error) = index_document(&mut index, &mut labels, entry) {
            return error_json(&error);
        }
    }
    let hits = ranked_hits(&index.search(query, k), &labels);
    json!({ "ok": true, "hits": hits }).to_string()
}

/// Sella `plaintext_hex` con AEAD XChaCha20-Poly1305 y un nonce fresco.
///
/// Args:
///     key_hex: Clave de 32 bytes en hexadecimal (64 dígitos).
///     plaintext_hex: Mensaje en claro codificado en hexadecimal.
///
/// Returns:
///     `{"ok":true,"nonce":"<hex>","ciphertext":"<hex>"}`, o
///     `{"ok":false,"error":"..."}` si la clave o el mensaje no son hex válidos.
pub fn seal_json(key_hex: &str, plaintext_hex: &str) -> String {
    let key = match decode_array::<KEY_SIZE>(key_hex, "clave") {
        Ok(key) => key,
        Err(error) => return error_json(&error),
    };
    let plaintext = match decode_hex(plaintext_hex) {
        Ok(plaintext) => plaintext,
        Err(error) => return error_json(&error),
    };
    let nonce = fresh_nonce();
    let ciphertext = seal(&key, &nonce, &plaintext);
    json!({
        "ok": true,
        "nonce": encode_hex(&nonce),
        "ciphertext": encode_hex(&ciphertext),
    })
    .to_string()
}

/// Abre un ciphertext sellado con [`seal_json`] verificando el tag AEAD.
///
/// Args:
///     key_hex: Clave de 32 bytes en hexadecimal (64 dígitos).
///     nonce_hex: Nonce de 24 bytes en hexadecimal (48 dígitos).
///     ciphertext_hex: Ciphertext (con tag) codificado en hexadecimal.
///
/// Returns:
///     `{"ok":true,"plaintext":"<hex>"}`, o `{"ok":false,"error":"..."}`.
pub fn open_json(key_hex: &str, nonce_hex: &str, ciphertext_hex: &str) -> String {
    let key = match decode_array::<KEY_SIZE>(key_hex, "clave") {
        Ok(key) => key,
        Err(error) => return error_json(&error),
    };
    let nonce = match decode_array::<NONCE_SIZE>(nonce_hex, "nonce") {
        Ok(nonce) => nonce,
        Err(error) => return error_json(&error),
    };
    let ciphertext = match decode_hex(ciphertext_hex) {
        Ok(ciphertext) => ciphertext,
        Err(error) => return error_json(&error),
    };
    match open(&key, &nonce, &ciphertext) {
        Ok(plaintext) => json!({ "ok": true, "plaintext": encode_hex(&plaintext) }).to_string(),
        Err(error) => error_json(&error.to_string()),
    }
}

/// Serializa un [`Statement`] del IR a `serde_json::Value`.
///
/// Args:
///     statement: Sentencia RQL analizada.
///
/// Returns:
///     Objeto JSON con `kind` (`select`, `explain`, `insert`, `update` o
///     `delete`).
fn statement_json(statement: &Statement) -> Value {
    match statement {
        Statement::Select(select) => select_json(select),
        Statement::Explain(explain) => {
            json!({ "kind": "explain", "inner": select_json(&explain.inner) })
        }
        Statement::Insert(insert) => json!({
            "kind": "insert",
            "table": &insert.table,
            "columns": &insert.columns,
            "rows": insert.rows.iter().map(|row| {
                row.iter().map(expr_json).collect::<Vec<_>>()
            }).collect::<Vec<_>>(),
        }),
        Statement::Update(update) => json!({
            "kind": "update",
            "table": &update.table,
            "assignments": update.assignments.iter().map(|(column, value)| {
                json!({ "column": column, "value": expr_json(value) })
            }).collect::<Vec<_>>(),
            "filter": update.filter.as_ref().map(expr_json),
        }),
        Statement::Delete(delete) => json!({
            "kind": "delete",
            "table": &delete.table,
            "filter": delete.filter.as_ref().map(expr_json),
        }),
    }
}

/// Serializa un [`Select`] a `serde_json::Value`.
///
/// Args:
///     select: Sentencia `SELECT` analizada.
///
/// Returns:
///     Objeto JSON con proyección, tabla, filtro y cláusulas (`null` si ausentes).
fn select_json(select: &Select) -> Value {
    json!({
        "kind": "select",
        "projection": projection_json(&select.projection),
        "from": &select.from,
        "filter": select.filter.as_ref().map(expr_json),
        "knn": select.knn.as_ref().map(knn_json),
        "traverse": select.traverse.as_ref().map(traverse_json),
        "limit": select.limit,
    })
}

/// Serializa una [`Projection`] a `serde_json::Value`.
///
/// Args:
///     projection: Proyección `*` o lista de columnas.
///
/// Returns:
///     `{"kind":"all"}` o `{"kind":"columns","columns":[...]}`.
fn projection_json(projection: &Projection) -> Value {
    match projection {
        Projection::All => json!({ "kind": "all" }),
        Projection::Columns(columns) => json!({ "kind": "columns", "columns": columns }),
    }
}

/// Serializa un [`Expr`] del filtro `WHERE` a `serde_json::Value`.
///
/// Args:
///     expr: Expresión del filtro.
///
/// Returns:
///     Objeto JSON con `kind` (`column`, `int`, `float`, `text`, `compare`,
///     `and` o `match`).
fn expr_json(expr: &Expr) -> Value {
    match expr {
        Expr::Column(name) => json!({ "kind": "column", "name": name }),
        Expr::Int(value) => json!({ "kind": "int", "value": value }),
        Expr::Float(value) => json!({ "kind": "float", "value": value }),
        Expr::Text(text) => json!({ "kind": "text", "value": text }),
        Expr::Compare { left, op, right } => json!({
            "kind": "compare",
            "left": expr_json(left),
            "op": op.as_str(),
            "right": expr_json(right),
        }),
        Expr::And(left, right) => json!({
            "kind": "and",
            "left": expr_json(left),
            "right": expr_json(right),
        }),
        Expr::Match { column, query } => {
            json!({ "kind": "match", "column": column, "query": query })
        }
        Expr::DocExtract { column, path } => {
            json!({ "kind": "doc_extract", "column": column, "path": path })
        }
        Expr::DocContains {
            column,
            json: literal,
        } => {
            json!({ "kind": "doc_contains", "column": column, "json": literal })
        }
    }
}

/// Serializa una cláusula `KNN` a `serde_json::Value`.
///
/// Args:
///     knn: Cláusula `KNN` analizada.
///
/// Returns:
///     `{"column":..,"k":..,"query":[...]}`.
fn knn_json(knn: &KnnClause) -> Value {
    json!({ "column": &knn.column, "k": knn.k, "query": &knn.query })
}

/// Serializa una cláusula `TRAVERSE` a `serde_json::Value`.
///
/// Args:
///     traverse: Cláusula `TRAVERSE` analizada.
///
/// Returns:
///     `{"column":..,"depth":..}`.
fn traverse_json(traverse: &TraverseClause) -> Value {
    json!({ "column": &traverse.column, "depth": traverse.depth })
}

/// Construye el JSON de error accionable (`NF-0023-01`).
///
/// Args:
///     message: Mensaje de error (WHAT + WHERE).
///
/// Returns:
///     `{"ok":false,"error":"<message>"}`.
fn error_json(message: &str) -> String {
    json!({ "ok": false, "error": message }).to_string()
}

/// Deserializa un texto JSON devolviendo un mensaje ante fallo.
///
/// Args:
///     text: Texto JSON de entrada.
///
/// Returns:
///     El [`Value`] parseado.
///
/// Errors:
///     Mensaje accionable si `text` no es JSON válido.
fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| format!("JSON malformado: {error}"))
}

/// Calcula el top-k vectorial con distancia L2 y lo serializa.
///
/// Args:
///     corpus: Arreglo JSON `[{"id":..,"vector":[..]}]`.
///     query: Arreglo JSON numérico con el vector de consulta.
///     k: Número máximo de resultados.
///
/// Returns:
///     Arreglo JSON de `{"id":..,"distance":..}` ordenado ascendente.
///
/// Errors:
///     Mensaje si el corpus/consulta tienen forma o dimensión inválida.
fn search_vectors(corpus: &Value, query: &Value, k: usize) -> Result<Value, String> {
    let entries = parse_corpus(corpus)?;
    let needle = parse_vector(query)?;
    if k == 0 {
        return Ok(json!([]));
    }
    let mut scored: Vec<(i64, f32)> = Vec::with_capacity(entries.len());
    for (id, vector) in &entries {
        let value = distance(Metric::L2, &needle, vector).map_err(|error| error.to_string())?;
        scored.push((*id, value));
    }
    scored.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    scored.truncate(k);
    Ok(json_hits(scored))
}

/// Serializa pares `(id, distancia)` como arreglo JSON.
///
/// Args:
///     scored: Pares ya ordenados por distancia ascendente.
///
/// Returns:
///     Arreglo de objetos `{"id":..,"distance":..}`.
fn json_hits(scored: Vec<(i64, f32)>) -> Value {
    Value::Array(
        scored
            .into_iter()
            .map(|(id, distance)| json!({ "id": id, "distance": distance }))
            .collect(),
    )
}

/// Parsea el corpus `[{"id":..,"vector":[..]}, ...]`.
///
/// Args:
///     corpus: Valor JSON del corpus.
///
/// Returns:
///     Lista de `(id, vector)` validada.
///
/// Errors:
///     Mensaje si el corpus no es un arreglo o una entrada es inválida.
fn parse_corpus(corpus: &Value) -> Result<Vec<(i64, Vec<f32>)>, String> {
    let array = corpus.as_array().ok_or("corpus debe ser un arreglo JSON")?;
    array.iter().map(parse_corpus_entry).collect()
}

/// Parsea una entrada `{"id":..,"vector":[..]}` del corpus.
///
/// Args:
///     entry: Valor JSON de la entrada.
///
/// Returns:
///     Par `(id, vector)` validado.
///
/// Errors:
///     Mensaje si falta `id`/`vector` o el vector no es numérico.
fn parse_corpus_entry(entry: &Value) -> Result<(i64, Vec<f32>), String> {
    let id = entry
        .get("id")
        .and_then(Value::as_i64)
        .ok_or("cada entrada del corpus requiere 'id' entero")?;
    let vector = entry
        .get("vector")
        .ok_or("cada entrada del corpus requiere 'vector'")?;
    Ok((id, parse_vector(vector)?))
}

/// Parsea un arreglo JSON numérico como vector `f32`.
///
/// Args:
///     value: Valor JSON del vector.
///
/// Returns:
///     Los valores convertidos a `f32`.
///
/// Errors:
///     Mensaje si no es un arreglo o contiene un elemento no numérico.
fn parse_vector(value: &Value) -> Result<Vec<f32>, String> {
    let array = value
        .as_array()
        .ok_or("el vector debe ser un arreglo JSON")?;
    array
        .iter()
        .map(|item| {
            item.as_f64()
                .map(|number| number as f32)
                .ok_or_else(|| "el vector debe contener números".to_string())
        })
        .collect()
}

/// Indexa un documento `{"id":..,"text":".."}` en el índice invertido.
///
/// Args:
///     index: Índice invertido destino.
///     labels: Mapa `RecordId -> id` numérico original.
///     entry: Valor JSON del documento.
///
/// Returns:
///     `Ok(())` si el documento se indexó.
///
/// Errors:
///     Mensaje si falta `id` o `text`, o no tienen el tipo esperado.
fn index_document(
    index: &mut InvertedIndex,
    labels: &mut HashMap<RecordId, i64>,
    entry: &Value,
) -> Result<(), String> {
    let id = entry
        .get("id")
        .and_then(Value::as_i64)
        .ok_or("cada documento requiere 'id' entero")?;
    let text = entry
        .get("text")
        .and_then(Value::as_str)
        .ok_or("cada documento requiere 'text' de texto")?;
    let record = RecordId::new();
    index.insert(record, text);
    labels.insert(record, id);
    Ok(())
}

/// Traduce los resultados BM25 a JSON conservando el id numérico original.
///
/// Args:
///     hits: Pares `(RecordId, score)` ordenados por BM25 descendente.
///     labels: Mapa `RecordId -> id` numérico original.
///
/// Returns:
///     Arreglo JSON `[{"id":..,"score":..}, ...]`.
fn ranked_hits(hits: &[(RecordId, f32)], labels: &HashMap<RecordId, i64>) -> Value {
    Value::Array(
        hits.iter()
            .filter_map(|(record, score)| {
                labels
                    .get(record)
                    .map(|id| json!({ "id": id, "score": score }))
            })
            .collect(),
    )
}

/// Genera un nonce XChaCha20 de 24 bytes único por llamada.
///
/// Combina el seed aleatorio por proceso de [`RandomState`] con un contador
/// atómico monótono, de modo que dos sellados nunca repiten nonce bajo la
/// misma clave (el espacio de 192 bits no exige coordinación).
///
/// Returns:
///     Nonce de [`NONCE_SIZE`] bytes.
fn fresh_nonce() -> [u8; NONCE_SIZE] {
    static STATE: OnceLock<RandomState> = OnceLock::new();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let state = STATE.get_or_init(RandomState::new);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut nonce = [0u8; NONCE_SIZE];
    for (index, chunk) in nonce.chunks_mut(size_of::<u64>()).enumerate() {
        let mut hasher = state.build_hasher();
        hasher.write_u64(counter);
        hasher.write_usize(index);
        chunk.copy_from_slice(&hasher.finish().to_le_bytes()[..chunk.len()]);
    }
    nonce
}

/// Decodifica hex y valida que tenga el tamaño exacto de un array.
///
/// Args:
///     text: Texto hexadecimal.
///     label: Nombre del campo (para el mensaje de error).
///
/// Returns:
///     El array de `SIZE` bytes.
///
/// Errors:
///     Mensaje si el hex es inválido o el tamaño no coincide.
fn decode_array<const SIZE: usize>(text: &str, label: &str) -> Result<[u8; SIZE], String> {
    let bytes = decode_hex(text)?;
    bytes
        .try_into()
        .map_err(|_| format!("{label} debe tener {SIZE} bytes ({} dígitos hex)", SIZE * 2))
}

/// Decodifica un texto hexadecimal a bytes.
///
/// Args:
///     text: Texto hexadecimal (longitud par, sin prefijo `0x`).
///
/// Returns:
///     Los bytes decodificados.
///
/// Errors:
///     Mensaje si la longitud es impar o hay un carácter no hexadecimal.
fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() % 2 != 0 {
        return Err("hex inválido: longitud impar".to_string());
    }
    let mut bytes = Vec::with_capacity(chars.len() / 2);
    for pair in chars.chunks(2) {
        bytes.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    Ok(bytes)
}

/// Devuelve el valor `0..=15` de un dígito hexadecimal.
///
/// Args:
///     character: Carácter hexadecimal.
///
/// Returns:
///     El valor numérico del dígito.
///
/// Errors:
///     Mensaje si el carácter no es un dígito hexadecimal.
fn hex_value(character: char) -> Result<u8, String> {
    match character {
        '0'..='9' => Ok(character as u8 - b'0'),
        'a'..='f' => Ok(character as u8 - b'a' + 10),
        'A'..='F' => Ok(character as u8 - b'A' + 10),
        _ => Err(format!("hex inválido: carácter '{character}'")),
    }
}

/// Codifica bytes como texto hexadecimal en minúsculas.
///
/// Args:
///     bytes: Bytes a codificar.
///
/// Returns:
///     Cadena hex de longitud `2 * bytes.len()`.
fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Capa JS de RuscaDB vía `wasm-bindgen` (feature `wasm`).
///
/// Requiere compilar para el target `wasm32` (`wasm-pack`/`wasm-bindgen`) para
/// que los símbolos `#[wasm_bindgen]` se exporten al módulo WebAssembly. En el
/// host (sin la feature) esta capa no existe y el crate es testeable con
/// `cargo test`.
#[cfg(feature = "wasm")]
pub mod js {
    use wasm_bindgen::prelude::wasm_bindgen;

    /// Analiza RQL y devuelve el IR en JSON (ver [`super::parse_query_json`]).
    ///
    /// Args:
    ///     sql: Texto de la sentencia RQL.
    ///
    /// Returns:
    ///     JSON con el `statement` o con el `error`.
    #[wasm_bindgen(js_name = parseQueryJson)]
    pub fn parse_query_json(sql: &str) -> String {
        super::parse_query_json(sql)
    }

    /// Top-k vectorial en JSON (ver [`super::vector_search_json`]).
    ///
    /// Args:
    ///     corpus_json: Corpus `[{"id":..,"vector":[..]}]`.
    ///     query_json: Vector de consulta `[..]`.
    ///     k: Número máximo de resultados.
    ///
    /// Returns:
    ///     JSON con `hits` o con `error`.
    #[wasm_bindgen(js_name = vectorSearchJson)]
    pub fn vector_search_json(corpus_json: &str, query_json: &str, k: usize) -> String {
        super::vector_search_json(corpus_json, query_json, k)
    }

    /// Búsqueda full-text BM25 en JSON (ver [`super::text_search_json`]).
    ///
    /// Args:
    ///     docs_json: Documentos `[{"id":..,"text":".."}]`.
    ///     query: Consulta full-text.
    ///     k: Número máximo de resultados.
    ///
    /// Returns:
    ///     JSON con `hits` o con `error`.
    #[wasm_bindgen(js_name = textSearchJson)]
    pub fn text_search_json(docs_json: &str, query: &str, k: usize) -> String {
        super::text_search_json(docs_json, query, k)
    }

    /// Sella bytes en hex (ver [`super::seal_json`]).
    ///
    /// Args:
    ///     key_hex: Clave de 32 bytes en hex.
    ///     plaintext_hex: Texto en claro en hex.
    ///
    /// Returns:
    ///     JSON con `nonce`/`ciphertext` o con `error`.
    #[wasm_bindgen(js_name = sealJson)]
    pub fn seal_json(key_hex: &str, plaintext_hex: &str) -> String {
        super::seal_json(key_hex, plaintext_hex)
    }

    /// Abre bytes sellados en hex (ver [`super::open_json`]).
    ///
    /// Args:
    ///     key_hex: Clave de 32 bytes en hex.
    ///     nonce_hex: Nonce de 24 bytes en hex.
    ///     ciphertext_hex: Ciphertext (con tag) en hex.
    ///
    /// Returns:
    ///     JSON con `plaintext` o con `error`.
    #[wasm_bindgen(js_name = openJson)]
    pub fn open_json(key_hex: &str, nonce_hex: &str, ciphertext_hex: &str) -> String {
        super::open_json(key_hex, nonce_hex, ciphertext_hex)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Clave de ejemplo de 32 bytes en hexadecimal.
    const KEY_HEX: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    /// Serializa una salida JSON y la deserializa como `Value`.
    fn value(output: &str) -> Value {
        serde_json::from_str(output).expect("la salida debe ser JSON válido")
    }

    /// Generador congruencial lineal determinista para los PBT/fuzzing ligero.
    struct Lcg(u64);

    impl Lcg {
        /// Crea el generador con la semilla dada.
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        /// Siguiente número pseudoaleatorio de 64 bits.
        fn next(&mut self) -> u64 {
            const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
            const INCREMENT: u64 = 1_442_695_040_888_963_407;
            self.0 = self.0.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
            self.0
        }

        /// Número en `0..bound` (0 si `bound == 0`).
        fn below(&mut self, bound: usize) -> usize {
            if bound == 0 {
                0
            } else {
                (self.next() % bound as u64) as usize
            }
        }

        /// Byte pseudoaleatorio.
        fn byte(&mut self) -> u8 {
            (self.next() >> 33) as u8
        }

        /// Cadena pseudoaleatoria con caracteres RQL/JSON y no-ASCII.
        fn text(&mut self, max_len: usize) -> String {
            const CHARS: [char; 28] = [
                's', 'e', 'l', 'c', 't', ' ', 'f', 'r', 'o', 'm', '0', '1', '9', ',', '\'', '[',
                ']', '<', '|', '>', '(', ')', '.', 'ñ', '\n', '\t', '中', '{',
            ];
            let length = self.below(max_len + 1);
            (0..length)
                .map(|_| CHARS[self.below(CHARS.len())])
                .collect()
        }
    }

    /// AC-0023-01 — una query RQL válida produce el JSON del IR.
    #[test]
    fn test_ac_0023_01_parse_query_json() {
        let output = parse_query_json("SELECT * FROM t KNN embedding <|5|> [0.1, 0.2] LIMIT 10");
        let json = value(&output);
        assert_eq!(json["ok"], Value::Bool(true));
        assert_eq!(json["statement"]["kind"], "select");
        assert_eq!(json["statement"]["from"], "t");
        assert_eq!(json["statement"]["projection"]["kind"], "all");
        assert_eq!(json["statement"]["filter"], Value::Null);
        assert_eq!(json["statement"]["knn"]["column"], "embedding");
        assert_eq!(json["statement"]["knn"]["k"], 5);
        assert_eq!(
            json["statement"]["knn"]["query"].as_array().unwrap().len(),
            2
        );
        assert_eq!(json["statement"]["traverse"], Value::Null);
        assert_eq!(json["statement"]["limit"], 10);
    }

    /// AC-0023-01 (continuación) — columnas, filtro compuesto y `EXPLAIN`.
    #[test]
    fn test_ac_0023_01_parse_query_json_clauses() {
        let query = "SELECT a, b FROM docs WHERE a = 1 AND b < 2.5 AND MATCH(c, 'gato') \
                     TRAVERSE edges DEPTH 2";
        let json = value(&parse_query_json(query));
        assert_eq!(json["statement"]["projection"]["kind"], "columns");
        assert_eq!(json["statement"]["projection"]["columns"][1], "b");
        assert_eq!(json["statement"]["filter"]["kind"], "and");
        assert_eq!(json["statement"]["filter"]["left"]["kind"], "and");
        assert_eq!(json["statement"]["filter"]["left"]["left"]["op"], "=");
        assert_eq!(
            json["statement"]["filter"]["left"]["right"]["kind"],
            "compare"
        );
        assert_eq!(json["statement"]["filter"]["right"]["kind"], "match");
        assert_eq!(json["statement"]["traverse"]["depth"], 2);
        assert_eq!(json["statement"]["knn"], Value::Null);

        let explain = value(&parse_query_json("EXPLAIN SELECT * FROM t"));
        assert_eq!(explain["statement"]["kind"], "explain");
        assert_eq!(explain["statement"]["inner"]["from"], "t");
    }

    /// AC-0023-02 — los k vecinos se devuelven ordenados por distancia.
    #[test]
    fn test_ac_0023_02_vector_search_json() {
        let corpus = r#"[{"id":1,"vector":[0.0,0.0]},{"id":2,"vector":[1.0,0.0]},
            {"id":3,"vector":[0.0,1.0]},{"id":4,"vector":[5.0,5.0]}]"#;
        let json = value(&vector_search_json(corpus, "[0.9,0.0]", 2));
        assert_eq!(json["ok"], Value::Bool(true));
        let hits = json["hits"].as_array().expect("hits");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0]["id"], 2);
        assert_eq!(hits[1]["id"], 1);
        assert!(hits[0]["distance"].as_f64().expect("dist") < 0.2);
    }

    /// AC-0023-02 (BVA) — `k = 0`, `k > N`, corpus vacío y desempate por id.
    #[test]
    fn test_ac_0023_02_vector_search_boundaries() {
        let corpus = r#"[{"id":2,"vector":[1.0,0.0]},{"id":1,"vector":[-1.0,0.0]}]"#;
        let empty = value(&vector_search_json(corpus, "[0.0,0.0]", 0));
        assert_eq!(empty["hits"].as_array().expect("hits").len(), 0);

        let all = value(&vector_search_json(corpus, "[0.0,0.0]", 99));
        let hits = all["hits"].as_array().expect("hits");
        assert_eq!(hits.len(), 2);
        // Distancias idénticas: desempate estable por id ascendente.
        assert_eq!(hits[0]["id"], 1);
        assert_eq!(hits[1]["id"], 2);

        let none = value(&vector_search_json("[]", "[0.0,0.0]", 3));
        assert_eq!(none["ok"], Value::Bool(true));
        assert_eq!(none["hits"].as_array().expect("hits").len(), 0);
    }

    /// AC-0023-03 — BM25 ordena los documentos más relevantes primero.
    #[test]
    fn test_ac_0023_03_text_search_json() {
        let docs = r#"[{"id":10,"text":"gato gato gato"},{"id":20,"text":"gato"},
            {"id":30,"text":"gato perro ave pez rata topo"}]"#;
        let json = value(&text_search_json(docs, "gato", 2));
        assert_eq!(json["ok"], Value::Bool(true));
        let hits = json["hits"].as_array().expect("hits");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0]["id"], 10);
        assert_eq!(hits[1]["id"], 20);
        assert!(hits[0]["score"].as_f64().expect("score") > 0.0);
    }

    /// AC-0023-03 (BVA) — `k = 0`, corpus vacío y términos ausentes.
    #[test]
    fn test_ac_0023_03_text_search_boundaries() {
        let docs = r#"[{"id":1,"text":"gato"}]"#;
        let zero = value(&text_search_json(docs, "gato", 0));
        assert_eq!(zero["hits"].as_array().expect("hits").len(), 0);

        let empty = value(&text_search_json("[]", "gato", 5));
        assert_eq!(empty["ok"], Value::Bool(true));
        assert_eq!(empty["hits"].as_array().expect("hits").len(), 0);

        let absent = value(&text_search_json(docs, "perro", 5));
        assert_eq!(absent["hits"].as_array().expect("hits").len(), 0);
    }

    /// AC-0023-04 — `seal_json` + `open_json` recuperan los bytes originales.
    #[test]
    fn test_ac_0023_04_crypto_roundtrip() {
        let plaintext = "527573636144422065732067656e69616c";
        let sealed = value(&seal_json(KEY_HEX, plaintext));
        assert_eq!(sealed["ok"], Value::Bool(true));
        let nonce = sealed["nonce"].as_str().expect("nonce");
        assert_eq!(nonce.len(), NONCE_SIZE * 2);
        let ciphertext = sealed["ciphertext"].as_str().expect("ciphertext");

        let opened = value(&open_json(KEY_HEX, nonce, ciphertext));
        assert_eq!(opened["ok"], Value::Bool(true));
        assert_eq!(opened["plaintext"], plaintext);

        // Mensaje vacío: roundtrip válido (solo queda el tag AEAD).
        let sealed_empty = value(&seal_json(KEY_HEX, ""));
        let empty_nonce = sealed_empty["nonce"].as_str().expect("nonce");
        let empty_ct = sealed_empty["ciphertext"].as_str().expect("ciphertext");
        let opened_empty = value(&open_json(KEY_HEX, empty_nonce, empty_ct));
        assert_eq!(opened_empty["plaintext"], "");
    }

    /// AC-0023-04 (seguridad) — nonce único y detección de manipulación.
    #[test]
    fn test_ac_0023_04_crypto_security() {
        let first = value(&seal_json(KEY_HEX, "00"));
        let second = value(&seal_json(KEY_HEX, "00"));
        assert_ne!(first["nonce"], second["nonce"], "el nonce debe ser único");

        let nonce = first["nonce"].as_str().expect("nonce").to_string();
        let ciphertext = first["ciphertext"]
            .as_str()
            .expect("ciphertext")
            .to_string();
        let mut tampered = ciphertext.clone().into_bytes();
        tampered[0] = if tampered[0] == b'0' { b'1' } else { b'0' };
        let broken = value(&open_json(
            KEY_HEX,
            &nonce,
            std::str::from_utf8(&tampered).expect("hex ascii"),
        ));
        assert_eq!(broken["ok"], Value::Bool(false));

        let other_key = "ff".repeat(KEY_SIZE);
        let wrong = value(&open_json(&other_key, &nonce, &ciphertext));
        assert_eq!(wrong["ok"], Value::Bool(false));
    }

    /// AC-0023-05 — entradas inválidas devuelven JSON de error, sin panics.
    #[test]
    fn test_ac_0023_05_errors_are_json() {
        for output in [
            parse_query_json("SELECT FROM"),
            parse_query_json("no es sql"),
            vector_search_json("no json", "[0.0]", 1),
            vector_search_json(r#"[{"id":1,"vector":[0.0,0.0]}]"#, "[0.0]", 1),
            vector_search_json("{}", "[0.0]", 1),
            vector_search_json(r#"[{"id":"x","vector":[0.0]}]"#, "[0.0]", 1),
            text_search_json("no json", "gato", 1),
            text_search_json("{}", "gato", 1),
            text_search_json(r#"[{"text":"gato"}]"#, "gato", 1),
            text_search_json(r#"[{"id":1}]"#, "gato", 1),
            seal_json("xyz", "00"),
            seal_json("00", "00"),
            seal_json(KEY_HEX, "0"),
            open_json("00", "00", "00"),
            open_json(KEY_HEX, "00", "00"),
            open_json(KEY_HEX, &"00".repeat(NONCE_SIZE), "zz"),
        ] {
            let json = value(&output);
            assert_eq!(json["ok"], Value::Bool(false), "salida: {output}");
            assert!(
                json["error"]
                    .as_str()
                    .is_some_and(|error| !error.is_empty()),
                "salida: {output}"
            );
        }
    }

    /// BVA de `decode_hex`: longitud impar, carácter inválido, mayúsculas y vacío.
    #[test]
    fn test_hex_codec_boundaries() {
        assert_eq!(decode_hex("").expect("vacío"), Vec::<u8>::new());
        assert_eq!(decode_hex("00ff").expect("minúsculas"), vec![0x00, 0xff]);
        assert_eq!(decode_hex("A0B1").expect("mayúsculas"), vec![0xa0, 0xb1]);
        assert_eq!(encode_hex(&[0x00, 0x0f, 0xff]), "000fff");
        assert_eq!(
            decode_hex(&encode_hex(&[1, 2, 3, 4])).expect("roundtrip"),
            vec![1, 2, 3, 4]
        );

        assert!(decode_hex("0").is_err(), "longitud impar");
        assert!(decode_hex("0g").is_err(), "carácter inválido");
        assert!(decode_hex("zz").is_err(), "carácter inválido");
        assert!(decode_key_too_short().is_err());
    }

    /// Decodifica una clave deliberadamente corta para el BVA anterior.
    fn decode_key_too_short() -> Result<[u8; KEY_SIZE], String> {
        decode_array::<KEY_SIZE>("00", "clave")
    }

    /// El parser nunca entra en panic ni produce JSON inválido (fuzzing ligero).
    #[test]
    fn test_prop_parse_query_never_panics() {
        let mut rng = Lcg::new(0x5EED_0023);
        for _ in 0..512 {
            let input = rng.text(48);
            let json = value(&parse_query_json(&input));
            assert!(json["ok"].is_boolean(), "input: {input:?}");
            if json["ok"] == Value::Bool(false) {
                assert!(
                    json["error"]
                        .as_str()
                        .is_some_and(|error| !error.is_empty())
                );
            }
        }
    }

    /// JSON malformado arbitrario nunca entra en panic (fuzzing ligero).
    #[test]
    fn test_prop_malformed_json_never_panics() {
        let mut rng = Lcg::new(0xBAD_0023);
        for _ in 0..512 {
            let input = rng.text(48);
            for output in [
                vector_search_json(&input, &input, 3),
                text_search_json(&input, "gato", 3),
                seal_json(&input, &input),
                open_json(&input, &input, &input),
            ] {
                let json = value(&output);
                assert!(json["ok"].is_boolean());
            }
        }
    }

    /// Roundtrip criptográfico sobre claves y mensajes aleatorios (PBT).
    #[test]
    fn test_prop_crypto_roundtrip() {
        let mut rng = Lcg::new(0xC0FF_EE23);
        for _ in 0..128 {
            let mut key = [0u8; KEY_SIZE];
            for byte in &mut key {
                *byte = rng.byte();
            }
            let plaintext: Vec<u8> = (0..rng.below(65)).map(|_| rng.byte()).collect();
            let sealed = value(&seal_json(&encode_hex(&key), &encode_hex(&plaintext)));
            let nonce = sealed["nonce"].as_str().expect("nonce");
            let ciphertext = sealed["ciphertext"].as_str().expect("ciphertext");
            let opened = value(&open_json(&encode_hex(&key), nonce, ciphertext));
            assert_eq!(opened["ok"], Value::Bool(true));
            assert_eq!(opened["plaintext"], encode_hex(&plaintext));
        }
    }

    /// La búsqueda vectorial aleatoria acota a `k` y ordena ascendente (PBT).
    #[test]
    fn test_prop_vector_search_is_bounded_and_sorted() {
        let mut rng = Lcg::new(0xF00D_0023);
        for _ in 0..64 {
            let dim = 1 + rng.below(4);
            let count = rng.below(6);
            let corpus: Vec<Value> = (0..count)
                .map(|index| {
                    let vector: Vec<f64> = (0..dim).map(|_| (rng.byte() as f64) - 128.0).collect();
                    json!({ "id": index as i64, "vector": vector })
                })
                .collect();
            let query: Vec<f64> = (0..dim).map(|_| (rng.byte() as f64) - 128.0).collect();
            let k = rng.below(8);
            let json = value(&vector_search_json(
                &Value::Array(corpus).to_string(),
                &json!(query).to_string(),
                k,
            ));
            assert_eq!(json["ok"], Value::Bool(true));
            let hits = json["hits"].as_array().expect("hits");
            assert!(hits.len() <= k.min(count));
            for pair in hits.windows(2) {
                let first = pair[0]["distance"].as_f64().expect("dist");
                let second = pair[1]["distance"].as_f64().expect("dist");
                assert!(first <= second);
            }
        }
    }
}
