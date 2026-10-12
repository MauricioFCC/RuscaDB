//! Tests de aceptación de la persistencia del audit (SPEC-0073).
//!
//! Cubre AC-0073-01..05: el audit habilitado se persiste en `<data>.audit.jsonl`
//! con patrón append-only (CRC32C por línea), sobrevive al reopen, descarta
//! líneas corruptas sin panic, respeta OFF por defecto, rota por tamaño
//! (`MAX_AUDIT_BYTES` -> `.1`) y `clear_audit` vacía fichero y trail.
//!
//! Estos tests son **RED por diseño** frente al estado actual: hoy el audit solo
//! vive en memoria (SPEC-0066), por lo que el fichero `<data>.audit.jsonl` no se
//! crea y el trail se pierde al reabrir. Las firmas usadas ya existen en la
//! fachada, así que el fallo es **por aserción**, no por compilación.
//!
//! Contrato de rutas: `<data>` es el `data_path` del `DbConfig`; el fichero de
//! audit se deriva con `data_path.with_extension("audit.jsonl")` y el rotado con
//! `data_path.with_extension("audit.jsonl.1")` (p. ej. `x.db` -> `x.audit.jsonl`
//! -> `x.audit.jsonl.1`).

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, ScalarMap, ScalarValue};

/// Capacidad del buffer pool de los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Nombre de la tabla usada en todos los escenarios.
const TABLE: &str = "items";

/// Tamaño máximo del fichero de audit antes de rotar (1 MiB, SPEC-0073).
const MAX_AUDIT_BYTES: u64 = 1_048_576;

/// Número máximo de entradas de audit aceptadas (SPEC-0073).
const MAX_AUDIT_ENTRIES: usize = 10_000;

/// Operaciones registradas en la prueba de persistencia (AC-0073-01).
const PERSISTED_OPS: usize = 3;

/// Operaciones registradas en los escenarios de corrupción y limpieza.
const SEEDED_OPS: usize = 2;

/// Líneas de relleno del fichero de audit sobredimensionado (AC-0073-04).
const FILLER_LINES: usize = 24;

/// Bytes por línea de relleno: `24 * 65_536 ≈ 1.5 MiB > MAX_AUDIT_BYTES`.
const FILLER_LINE_BYTES: usize = 65_536;

/// Crea un directorio temporal y la ruta del fichero de datos.
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la ruta `<tag>.db`.
fn temp_paths(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let data_path = dir.path().join(format!("{tag}.db"));
    (dir, data_path)
}

/// Abre una base en la ruta indicada con pool amplio.
///
/// Args:
///     data_path: Ruta del archivo de páginas.
///
/// Returns:
///     La base lista para operar.
fn open_db(data_path: &Path) -> Database {
    Database::open(DbConfig::new(data_path, POOL_CAPACITY)).expect("apertura de la base")
}

/// Deriva la ruta del fichero de audit `<data>.audit.jsonl`.
///
/// Args:
///     data_path: Ruta del archivo de páginas.
///
/// Returns:
///     La ruta del fichero de audit (patrón de `models_path`).
fn audit_path(data_path: &Path) -> PathBuf {
    data_path.with_extension("audit.jsonl")
}

/// Deriva la ruta del fichero de audit rotado `<data>.audit.jsonl.1`.
///
/// Args:
///     data_path: Ruta del archivo de páginas.
///
/// Returns:
///     La ruta del fichero rotado por tamaño.
fn rotated_audit_path(data_path: &Path) -> PathBuf {
    data_path.with_extension("audit.jsonl.1")
}

/// Crea `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base destino.
fn create_items(database: &mut Database) {
    database
        .create_table(
            TABLE,
            vec![
                ColumnDef {
                    name: "id".to_string(),
                    col_type: ColumnType::Int,
                },
                ColumnDef {
                    name: "name".to_string(),
                    col_type: ColumnType::Text,
                },
            ],
        )
        .expect("create_table");
}

/// Construye escalares `(id, name)`.
///
/// Args:
///     id: Valor entero de la columna `id`.
///     name: Valor textual de la columna `name`.
///
/// Returns:
///     El mapa de escalares listo para un registro.
fn scalars(id: i64, name: &str) -> ScalarMap {
    ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ])
}

/// Inserta `count` filas secuenciales en `items`.
///
/// Args:
///     database: Base destino (debe tener la tabla `items`).
///     count: Número de filas a insertar.
fn insert_rows(database: &mut Database, count: usize) {
    for index in 0..count {
        let id = index as i64 + 1;
        database
            .insert(TABLE, scalars(id, &format!("fila-{id}")))
            .expect("insert");
    }
}

/// Longitud en bytes del fichero indicado.
///
/// Args:
///     path: Ruta del fichero.
///
/// Returns:
///     El tamaño en bytes del fichero existente.
fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).expect("metadata del fichero").len()
}

/// Corrompe el último byte de la primera línea del fichero de audit.
///
/// Mantiene ASCII válido: sustituye el byte por `0` salvo que ya sea `0`, en
/// cuyo caso usa `1`. Así la línea deja de ser válida (CRC/JSON) sin romper la
/// codificación UTF-8.
///
/// Args:
///     path: Ruta del fichero JSONL a corromper.
fn corrupt_first_line(path: &Path) {
    let mut bytes = std::fs::read(path).expect("lectura del audit.jsonl");
    let newline = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("el fichero de audit debe tener al menos una línea terminada en \\n");
    assert!(newline > 0, "la primera línea no debe estar vacía");
    let target = newline - 1;
    bytes[target] = if bytes[target] == b'0' { b'1' } else { b'0' };
    std::fs::write(path, &bytes).expect("escritura del audit.jsonl corrompido");
}

/// Siembra un `audit.jsonl` de tamaño > `MAX_AUDIT_BYTES` con líneas basura.
///
/// Args:
///     path: Ruta del fichero de audit a sembrar.
fn write_oversized_audit(path: &Path) {
    let line = "x".repeat(FILLER_LINE_BYTES);
    let mut content = String::with_capacity((FILLER_LINE_BYTES + 1) * FILLER_LINES);
    for _ in 0..FILLER_LINES {
        content.push_str(line.as_str());
        content.push('\n');
    }
    std::fs::write(path, content).expect("sembrar audit.jsonl sobredimensionado");
}

/// AC-0073-01 — el audit habilitado sobrevive al reopen.
#[test]
// @spec AC-0073-01
fn test_ac_0073_01_audit_survives_reopen() {
    let (_dir, data_path) = temp_paths("ac0073_01");
    let audit_file = audit_path(&data_path);

    let mut database = open_db(&data_path);
    create_items(&mut database);
    database.set_audit(true).expect("set_audit(true)");
    insert_rows(&mut database, PERSISTED_OPS);
    let before = database.audit_trail();
    assert!(
        before.len() >= PERSISTED_OPS,
        "con audit ON deben capturarse al menos {PERSISTED_OPS} entradas"
    );
    database.close().expect("close");

    let reopened = open_db(&data_path);
    let trail = reopened.audit_trail();
    assert!(
        trail.len() >= PERSISTED_OPS,
        "el audit debe recuperarse tras reopen desde {}: {} entradas",
        audit_file.display(),
        trail.len()
    );
    assert!(
        trail.len() <= MAX_AUDIT_ENTRIES,
        "el trail recuperado no debe superar {MAX_AUDIT_ENTRIES} entradas"
    );
    assert!(
        trail.iter().any(|entry| entry.action.contains("insert")),
        "las entradas recuperadas deben contener la acción 'insert': {:?}",
        trail
            .iter()
            .map(|entry| entry.action.clone())
            .collect::<Vec<_>>()
    );
    for entry in &trail {
        assert!(
            !entry.action.is_empty(),
            "toda entrada recuperada debe tener acción no vacía"
        );
    }
}

/// AC-0073-02 — una línea con CRC alterado se descarta sin panic.
#[test]
// @spec AC-0073-02
fn test_ac_0073_02_corrupt_line_is_discarded() {
    let (_dir, data_path) = temp_paths("ac0073_02");
    let audit_file = audit_path(&data_path);

    let mut database = open_db(&data_path);
    create_items(&mut database);
    database.set_audit(true).expect("set_audit(true)");
    insert_rows(&mut database, SEEDED_OPS);
    database.close().expect("close");

    assert!(
        audit_file.exists(),
        "no se creó {}: el audit ON debe persistir en <data>.audit.jsonl",
        audit_file.display()
    );

    corrupt_first_line(&audit_file);

    // Reabrir no debe panicar: la línea corrupta se descarta al cargar.
    let reopened = open_db(&data_path);
    let trail = reopened.audit_trail();
    assert!(
        !trail.is_empty(),
        "debe recuperar al menos una entrada válida y descartar la corrupta: {trail:?}"
    );
    for entry in &trail {
        assert!(
            !entry.action.is_empty(),
            "toda entrada válida recuperada debe tener acción no vacía"
        );
    }
}

/// AC-0073-03 — con audit OFF (por defecto) no se crean entradas ni fichero.
#[test]
// @spec AC-0073-03
fn test_ac_0073_03_audit_off_creates_no_entries() {
    let (_dir, data_path) = temp_paths("ac0073_03");
    let audit_file = audit_path(&data_path);

    let mut database = open_db(&data_path);
    create_items(&mut database);
    // OFF por defecto: no se invoca `set_audit(true)`.
    insert_rows(&mut database, SEEDED_OPS);
    assert!(
        database.audit_trail().is_empty(),
        "sin habilitar el audit el trail debe estar vacío"
    );
    database.close().expect("close");

    assert!(
        !audit_file.exists() || file_len(&audit_file) == 0,
        "con audit OFF {} no debe contener entradas",
        audit_file.display()
    );

    let reopened = open_db(&data_path);
    assert!(
        reopened.audit_trail().is_empty(),
        "tras reopen con audit OFF el trail debe estar vacío"
    );
}

/// AC-0073-04 — el fichero que supera `MAX_AUDIT_BYTES` rota a `.1`.
#[test]
// @spec AC-0073-04
fn test_ac_0073_04_audit_rotates_by_size() {
    let (_dir, data_path) = temp_paths("ac0073_04");
    let audit_file = audit_path(&data_path);
    let rotated = rotated_audit_path(&data_path);

    // Sesión previa: crea la tabla y deja el `data_path` en disco.
    let mut database = open_db(&data_path);
    create_items(&mut database);
    database.close().expect("close");

    // Siembra un audit sobredimensionado ANTES de habilitar la captura.
    write_oversized_audit(&audit_file);
    let seeded = file_len(&audit_file);
    assert!(
        seeded > MAX_AUDIT_BYTES,
        "el sembrado debe superar MAX_AUDIT_BYTES ({seeded} <= {MAX_AUDIT_BYTES})"
    );

    let mut database = open_db(&data_path);
    database.set_audit(true).expect("set_audit(true)");
    insert_rows(&mut database, 1);
    let trail = database.audit_trail();

    assert!(
        rotated.exists(),
        "el fichero activo debe rotar a {}",
        rotated.display()
    );
    let rotated_len = file_len(&rotated);
    assert!(
        rotated_len > MAX_AUDIT_BYTES,
        "el fichero rotado debe conservar el contenido sobredimensionado: {rotated_len}"
    );

    let active_len = file_len(&audit_file);
    assert!(
        active_len < MAX_AUDIT_BYTES,
        "el fichero activo debe ser más pequeño tras rotar: {active_len}"
    );
    assert!(
        !trail.is_empty(),
        "el fichero activo debe continuar con la nueva entrada de audit"
    );
    database.close().expect("close");
}

/// AC-0073-05 — `clear_audit` vacía el trail y el fichero, sin desactivar.
#[test]
// @spec AC-0073-05
fn test_ac_0073_05_clear_audit_wipes_file() {
    let (_dir, data_path) = temp_paths("ac0073_05");
    let audit_file = audit_path(&data_path);

    let mut database = open_db(&data_path);
    create_items(&mut database);
    database.set_audit(true).expect("set_audit(true)");
    insert_rows(&mut database, SEEDED_OPS);

    assert!(
        audit_file.exists() && file_len(&audit_file) > 0,
        "el audit ON debe haber escrito entradas en {}",
        audit_file.display()
    );

    database.clear_audit().expect("clear_audit");
    assert!(
        database.audit_trail().is_empty(),
        "clear_audit debe vaciar el trail en memoria"
    );
    assert!(
        audit_file.exists(),
        "clear_audit no debe borrar el fichero, solo vaciarlo"
    );
    assert_eq!(
        file_len(&audit_file),
        0,
        "clear_audit debe dejar el fichero en 0 bytes"
    );

    // La captura sigue activa tras el clear.
    insert_rows(&mut database, 1);
    assert_eq!(
        database.audit_trail().len(),
        1,
        "la captura de audit debe seguir activa tras clear_audit"
    );
    database.close().expect("close");
}
