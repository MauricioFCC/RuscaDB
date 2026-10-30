//! Contrato ABI de busqueda vectorial filtrada y cuantizada (SPEC-0171/0172).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use std::ffi::{CString, c_char};

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, ScalarMap, ScalarValue,
};
use ruscadb_ffi::{
    RC_BUFFER_TOO_SMALL, RC_DOMAIN_ERROR, RC_NULL_POINTER, RC_OK, RuscadbHandle, ruscadb_close,
    ruscadb_open, ruscadb_search_filtered, ruscadb_search_quantized,
};

/// CString sin NUL interno.
fn c(text: &str) -> CString {
    CString::new(text).expect("sin NUL")
}

/// Prepara una base con 20 documentos y devuelve sus ids en orden de insercion.
fn seed_database(path: &std::path::Path) -> Vec<String> {
    let mut ids = Vec::new();
    let mut database = Database::open(DbConfig::new(path, 128)).expect("open");
    database
        .create_table(
            "docs",
            vec![ColumnDef {
                name: "titulo".to_string(),
                col_type: ColumnType::Text,
            }],
        )
        .expect("create_table");
    for index in 0..20i32 {
        let id = RecordId::new();
        ids.push(id.to_string());
        let record = Record {
            id,
            scalars: ScalarMap::from([(
                "titulo".to_string(),
                ScalarValue::Text(format!("doc-{index}")),
            )]),
            doc: None,
            edges: EdgeSet::default(),
            vector: Some(
                Embedding::new(
                    vec![index as f32, (index % 3) as f32, 1.0],
                    EmbeddingMeta {
                        model_id: "test".to_string(),
                        dim: 3,
                        metric: Metric::L2,
                    },
                )
                .expect("embedding"),
            ),
            blob: None,
            meta: RecordMeta::default(),
        };
        database.insert_record("docs", record).expect("insert");
    }
    ids
}

/// Extrae el JSON escrito por la frontera C.
fn read_json(buffer: &[u8]) -> String {
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    std::str::from_utf8(&buffer[..end])
        .expect("json utf8")
        .to_string()
}

/// `ruscadb_search_quantized`: ANN + rescore y errores del contrato.
#[test] // @spec AC-0181-05
fn test_ac_0181_05_search_quantized_contract() {
    let dir = tempfile::tempdir().expect("directorio");
    let path = dir.path().join("quantized.db");
    let _ids = seed_database(&path);

    let mut handle: *mut RuscadbHandle = std::ptr::null_mut();
    let path_text = c(path.to_str().expect("ruta utf8"));
    assert_eq!(ruscadb_open(path_text.as_ptr(), 128, &mut handle), RC_OK);
    assert!(!handle.is_null());

    let query = [0.0f32, 0.0, 1.0];
    let table = c("docs");
    let mut buffer = vec![0u8; 4096];
    let rc = ruscadb_search_quantized(
        handle,
        table.as_ptr(),
        query.as_ptr(),
        3,
        5,
        buffer.as_mut_ptr() as *mut c_char,
        buffer.len(),
    );
    assert_eq!(rc, RC_OK);
    let json = read_json(&buffer);
    assert!(json.starts_with('['), "json: {json}");
    assert!(json.contains("\"id\""), "json: {json}");
    assert!(json.contains("\"distance\""), "json: {json}");

    // Buffer insuficiente y puntero nulo.
    let mut tiny = [0u8; 2];
    assert_eq!(
        ruscadb_search_quantized(
            handle,
            table.as_ptr(),
            query.as_ptr(),
            3,
            5,
            tiny.as_mut_ptr() as *mut c_char,
            tiny.len()
        ),
        RC_BUFFER_TOO_SMALL
    );
    assert_eq!(
        ruscadb_search_quantized(
            handle,
            table.as_ptr(),
            std::ptr::null(),
            3,
            5,
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len()
        ),
        RC_NULL_POINTER
    );

    assert_eq!(ruscadb_close(handle), RC_OK);
}

/// `ruscadb_search_filtered`: ACORN respeta el conjunto `allowed`.
#[test] // @spec AC-0181-06
fn test_ac_0181_06_search_filtered_contract() {
    let dir = tempfile::tempdir().expect("directorio");
    let path = dir.path().join("filtered.db");
    let ids = seed_database(&path);

    let mut handle: *mut RuscadbHandle = std::ptr::null_mut();
    let path_text = c(path.to_str().expect("ruta utf8"));
    assert_eq!(ruscadb_open(path_text.as_ptr(), 128, &mut handle), RC_OK);

    let query = [0.0f32, 0.0, 1.0];
    let table = c("docs");
    let mut buffer = vec![0u8; 4096];

    // Solo el primer documento cumple el predicado: debe ser el unico resultado.
    let allowed = serde_json::to_string(&[ids[0].clone()]).expect("json ids");
    let allowed_text = c(&allowed);
    let rc = ruscadb_search_filtered(
        handle,
        table.as_ptr(),
        query.as_ptr(),
        3,
        5,
        allowed_text.as_ptr(),
        buffer.as_mut_ptr() as *mut c_char,
        buffer.len(),
    );
    assert_eq!(rc, RC_OK);
    let json = read_json(&buffer);
    assert!(json.contains(&ids[0]), "json: {json}");
    assert_eq!(json.matches("\"id\"").count(), 1, "json: {json}");

    // Conjunto vacio: sin resultados, pero valido.
    let empty = c("[]");
    let rc = ruscadb_search_filtered(
        handle,
        table.as_ptr(),
        query.as_ptr(),
        3,
        5,
        empty.as_ptr(),
        buffer.as_mut_ptr() as *mut c_char,
        buffer.len(),
    );
    assert_eq!(rc, RC_OK);
    assert_eq!(read_json(&buffer), "[]");

    // JSON invalido y puntero nulo.
    let invalid = c("{\"no\":\"array\"}");
    assert_eq!(
        ruscadb_search_filtered(
            handle,
            table.as_ptr(),
            query.as_ptr(),
            3,
            5,
            invalid.as_ptr(),
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len()
        ),
        RC_DOMAIN_ERROR
    );
    assert_eq!(
        ruscadb_search_filtered(
            handle,
            table.as_ptr(),
            query.as_ptr(),
            3,
            5,
            std::ptr::null(),
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len()
        ),
        RC_NULL_POINTER
    );

    assert_eq!(ruscadb_close(handle), RC_OK);
}
