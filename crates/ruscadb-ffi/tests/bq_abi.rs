//! Contrato ABI de cuantizacion RaBitQ (SPEC-0171 / `ruscadb-ivf-pq` §6.1).

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use std::ffi::{CString, c_char};

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, ScalarMap, ScalarValue,
};
use ruscadb_ffi::{
    RC_BUFFER_TOO_SMALL, RC_NULL_POINTER, RC_OK, RuscadbHandle, ruscadb_close, ruscadb_open,
    ruscadb_quantize, ruscadb_search_bq,
};

/// CString sin NUL interno.
fn c(text: &str) -> CString {
    CString::new(text).expect("sin NUL")
}

/// `ruscadb_quantize`: bits, normas y errores del contrato.
#[test]
fn quantize_contract() {
    let vectors = [1.0f32, -2.0, 3.0, -4.0];
    let mut codes = [0u8; 2];
    let mut norms = [0f32; 2];
    let rc = ruscadb_quantize(
        vectors.as_ptr(),
        2,
        2,
        codes.as_mut_ptr(),
        codes.len(),
        norms.as_mut_ptr(),
        norms.len(),
    );
    assert_eq!(rc, RC_OK);
    // Signo por dimension: [1,-2] -> 0b01 y [3,-4] -> 0b01.
    assert_eq!(codes, [0b01, 0b01]);
    assert!((norms[0] - 5f32.sqrt()).abs() < 1e-5, "norma de [1,-2]");
    assert!((norms[1] - 5.0).abs() < 1e-5, "norma de [3,-4]");

    // Puntero nulo y buffer corto.
    assert_eq!(
        ruscadb_quantize(
            std::ptr::null(),
            1,
            2,
            codes.as_mut_ptr(),
            2,
            norms.as_mut_ptr(),
            2
        ),
        RC_NULL_POINTER
    );
    let mut small = [0u8; 1];
    assert_eq!(
        ruscadb_quantize(
            vectors.as_ptr(),
            2,
            2,
            small.as_mut_ptr(),
            small.len(),
            norms.as_mut_ptr(),
            2
        ),
        RC_BUFFER_TOO_SMALL
    );
    // dim = 0 es error de dominio.
    assert_eq!(
        ruscadb_quantize(
            vectors.as_ptr(),
            1,
            0,
            codes.as_mut_ptr(),
            2,
            norms.as_mut_ptr(),
            2
        ),
        ruscadb_ffi::RC_DOMAIN_ERROR
    );
}

/// `ruscadb_search_bq`: KNN binario + rescore sobre una base real.
#[test]
fn search_bq_roundtrip() {
    let dir = tempfile::tempdir().expect("directorio");
    let path = dir.path().join("bq.db");
    {
        let mut database = Database::open(DbConfig::new(&path, 128)).expect("open");
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
            let vector = vec![index as f32, (index % 3) as f32, 1.0];
            let record = Record {
                id: RecordId::new(),
                scalars: ScalarMap::from([(
                    "titulo".to_string(),
                    ScalarValue::Text(format!("doc-{index}")),
                )]),
                doc: None,
                edges: EdgeSet::default(),
                vector: Some(
                    Embedding::new(
                        vector,
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
    }

    let mut handle: *mut RuscadbHandle = std::ptr::null_mut();
    let path_text = c(path.to_str().expect("ruta utf8"));
    assert_eq!(ruscadb_open(path_text.as_ptr(), 128, &mut handle), RC_OK);
    assert!(!handle.is_null());

    let query = [0.0f32, 0.0, 1.0];
    let table = c("docs");
    let mut buffer = vec![0u8; 4096];
    let rc = ruscadb_search_bq(
        handle,
        table.as_ptr(),
        query.as_ptr(),
        3,
        5,
        3,
        buffer.as_mut_ptr() as *mut c_char,
        buffer.len(),
    );
    assert_eq!(rc, RC_OK);
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    let json = std::str::from_utf8(&buffer[..end]).expect("json utf8");
    assert!(json.starts_with('['), "json: {json}");
    assert!(json.contains("\"id\""), "json: {json}");
    assert!(json.contains("\"distance\""), "json: {json}");

    // Buffer insuficiente y puntero nulo.
    let mut tiny = [0u8; 2];
    assert_eq!(
        ruscadb_search_bq(
            handle,
            table.as_ptr(),
            query.as_ptr(),
            3,
            5,
            3,
            tiny.as_mut_ptr() as *mut c_char,
            tiny.len()
        ),
        RC_BUFFER_TOO_SMALL
    );
    assert_eq!(
        ruscadb_search_bq(
            handle,
            std::ptr::null(),
            query.as_ptr(),
            3,
            5,
            3,
            buffer.as_mut_ptr() as *mut c_char,
            buffer.len()
        ),
        RC_NULL_POINTER
    );

    assert_eq!(ruscadb_close(handle), RC_OK);
}
