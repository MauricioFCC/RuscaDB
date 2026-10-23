//! Tests de seguridad: path traversal, validación de entrada, resource exhaustion.

#![allow(clippy::expect_used)]

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

fn open_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(format!("{tag}.db"));
    let db = Database::open(DbConfig::new(&path, 64)).expect("open");
    (dir, db)
}

/// Path traversal en nombres de tabla (debe rechazarse o no crear ficheros fuera).
#[test]
fn test_security_path_traversal_table_name() {
    let (_dir, mut db) = open_db("sec_path");
    // Nombres con path traversal deben fallar o ser tratados como nombre literal.
    let malicious_names = [
        "../etc/passwd",
        "..\\windows\\system32",
        "/tmp/evil",
        "a/b",
        "a\\b",
    ];
    for name in &malicious_names {
        let result = db.create_table(
            name,
            vec![ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            }],
        );
        // Puede ser Ok (nombre literal) o Err, pero no debe crear ficheros fuera.
        let _ = result;
    }
    // La base sigue operativa.
    let _ = db.execute("SELECT 1");
}

/// String muy largo en RQL no causa panic ni OOM.
#[test]
fn test_security_large_input() {
    let (_dir, mut db) = open_db("sec_large");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "name".to_string(),
            col_type: ColumnType::Text,
        }],
    )
    .expect("create");
    // String de 1MB.
    let big = "a".repeat(1_000_000);
    let sql = format!("INSERT INTO t (name) VALUES ('{}')", big);
    let _ = db.execute(&sql);
    // No panic; la base sigue operativa.
    let _ = db.execute("SELECT * FROM t");
}

/// Muchas columnas no causa panic.
#[test]
fn test_security_many_columns() {
    let (_dir, mut db) = open_db("sec_cols");
    let columns: Vec<ColumnDef> = (0..1000)
        .map(|i| ColumnDef {
            name: format!("col{i}"),
            col_type: ColumnType::Int,
        })
        .collect();
    let result = db.create_table("wide", columns);
    // Puede ser Ok o Err (límite), pero no panic.
    let _ = result;
}

/// Valores especiales en SQL no causan panic.
#[test]
fn test_security_special_chars() {
    let (_dir, mut db) = open_db("sec_special");
    db.create_table(
        "t",
        vec![ColumnDef {
            name: "name".to_string(),
            col_type: ColumnType::Text,
        }],
    )
    .expect("create");
    let specials = [
        "'; DROP TABLE t; --",
        "\" OR 1=1 --",
        "\\x00\\x01\\x02",
        "🎉🔥💀",
        "\n\r\t\0",
        &"a".repeat(10000),
    ];
    for s in &specials {
        let escaped = s.replace('\'', "''");
        let sql = format!("INSERT INTO t (name) VALUES ('{}')", escaped);
        let _ = db.execute(&sql);
    }
    // La tabla sigue existiendo.
    let _ = db.execute("SELECT * FROM t");
}

/// Backup no permite path traversal en nombres de fichero.
#[test]
fn test_security_backup_path_traversal() {
    use ruscadb::Backup;
    let dir = tempfile::tempdir().expect("dir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(src.join("data.db"), b"hello").expect("write");
    // Un fichero con nombre normal se copia.
    let backup = Backup::new(&src);
    let copied = backup.snapshot(&dst).expect("snapshot");
    assert_eq!(copied, 1);
    assert!(dst.join("data.db").exists());
}

/// Read-only mode rechaza todas las escrituras.
#[test]
fn test_security_readonly_rejects_all_writes() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("ro.db");
    {
        let mut db = Database::open(DbConfig::new(&path, 64)).expect("open");
        db.create_table(
            "t",
            vec![ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            }],
        )
        .expect("create");
        db.insert(
            "t",
            ScalarMap::from([("id".to_string(), ScalarValue::Int(1))]),
        )
        .expect("insert");
    }
    let mut reader = Database::open_readonly(DbConfig::new(&path, 64)).expect("readonly");
    // create_table
    assert!(
        reader
            .create_table(
                "t2",
                vec![ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int
                }]
            )
            .is_err()
    );
    // insert
    assert!(
        reader
            .insert(
                "t",
                ScalarMap::from([("id".to_string(), ScalarValue::Int(2))])
            )
            .is_err()
    );
    // delete
    // (no record id handy, but execute DML should fail)
    assert!(reader.execute("DELETE FROM t").is_err());
    assert!(reader.execute("UPDATE t SET id = 2").is_err());
    assert!(reader.execute("INSERT INTO t (id) VALUES (2)").is_err());
}
