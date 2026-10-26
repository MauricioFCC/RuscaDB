#![allow(missing_docs)]
#![cfg(feature = "prepared-real")]
#![allow(clippy::expect_used)]
use ruscadb::PreparedStmtReal;
#[test] // @spec AC-0166-01
fn test_ac_0166_01_bind() {
    let p = PreparedStmtReal::prepare("SELECT * FROM t WHERE id = $1", 1);
    assert!(p.bind(&[42]));
}
#[test] // @spec AC-0166-02
fn test_ac_0166_02_reuse() {
    let p = PreparedStmtReal::prepare("q", 1);
    assert!(p.bind(&[1]));
    assert!(p.bind(&[2]));
}
#[test] // @spec AC-0166-03
fn test_ac_0166_03_injection() {
    let p = PreparedStmtReal::prepare("q", 1);
    assert!(p.bind(&[0]));
    assert_eq!(p.query(), "q");
}
