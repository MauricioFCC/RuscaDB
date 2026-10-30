//! Tests de aceptacion de la feature `vle` (SPEC-0137).

#![cfg(feature = "vle")]
#![allow(clippy::expect_used)]

use ruscadb::{VleDatabase, VleRecord};

#[test] // @spec AC-0137-01
fn test_ac_0137_01_vle_create() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("test.pvdb");
    let db = VleDatabase::new(path.to_string_lossy().into_owned());
    assert_eq!(db.path(), path.to_string_lossy());

    let records = vec![VleRecord {
        key: "k".to_string(),
        value: b"valor".to_vec(),
    }];
    assert_eq!(db.create(&records).expect("create"), 1);
    assert_eq!(db.read_all().expect("read"), records);
    assert_eq!(db.append("k2", b"otro".to_vec()).expect("append"), 2);
    assert_eq!(db.len().expect("len"), 2);
}
