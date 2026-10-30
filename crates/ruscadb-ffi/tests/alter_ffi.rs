//! `ruscadb_alter_table` (SPEC-0160) sobre el C-ABI.

#![allow(missing_docs)]
#![allow(clippy::expect_used)]

use std::ffi::CString;

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig};
use ruscadb_ffi::{
    RC_DOMAIN_ERROR, RC_NULL_POINTER, RC_OK, RuscadbHandle, ruscadb_alter_table, ruscadb_close,
    ruscadb_open,
};

/// CString sin NUL interno.
fn c(text: &str) -> CString {
    CString::new(text).expect("sin NUL")
}

/// Alta de columna por ABI, error de duplicado y persistencia al reabrir.
#[test]
fn alter_table_abi() {
    let dir = tempfile::tempdir().expect("directorio");
    let path = dir.path().join("alter.db");
    {
        let mut database = Database::open(DbConfig::new(&path, 128)).expect("open");
        database
            .create_table(
                "t",
                vec![ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("create_table");
    }

    let mut handle: *mut RuscadbHandle = std::ptr::null_mut();
    let path_text = c(path.to_str().expect("ruta utf8"));
    assert_eq!(ruscadb_open(path_text.as_ptr(), 128, &mut handle), RC_OK);
    assert!(!handle.is_null());

    let table = c("t");
    let column = c("session_id");
    assert_eq!(
        ruscadb_alter_table(handle, table.as_ptr(), column.as_ptr(), 2),
        RC_OK
    );
    // Duplicada -> error de dominio.
    assert_eq!(
        ruscadb_alter_table(handle, table.as_ptr(), column.as_ptr(), 2),
        RC_DOMAIN_ERROR
    );
    // type_code invalido y puntero nulo.
    assert_eq!(
        ruscadb_alter_table(handle, table.as_ptr(), column.as_ptr(), 9),
        RC_DOMAIN_ERROR
    );
    assert_eq!(
        ruscadb_alter_table(handle, std::ptr::null(), column.as_ptr(), 2),
        RC_NULL_POINTER
    );
    assert_eq!(ruscadb_close(handle), RC_OK);

    // Persistido: al reabrir con Rust la columna existe.
    let mut database = Database::open(DbConfig::new(&path, 128)).expect("reapertura");
    assert!(database.has_column("t", "session_id").expect("has_column"));
}
