//! Tests de aceptación de hardening operativo + shadow reads (SPEC-0062).
//!
//! Cubre los criterios AC-0062-01/02/03/05: `integrity_check` sobre una base
//! sana y sobre un WAL corrupto, y la paridad/nulidad de coste de shadow reads.
//! Estos tests son RED por diseño: la API de SPEC-0062 aún no existe en la
//! fachada `ruscadb`, por lo que la compilación falla al resolver los símbolos
//! `Database::integrity_check`, `IntegrityReport`, `Database::set_shadow_reads`,
//! `Database::shadow_reads_enabled`, `Database::shadow_report` y `ShadowReport`.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, IntegrityReport,
    Metric, Record, RecordId, RecordMeta, ScalarMap, ScalarValue, ShadowReport,
};

/// Capacidad del buffer pool en los tests (sin backpressure).
const POOL_CAPACITY: usize = 128;

/// Dimensión de los vectores de prueba (coincide con la query KNN).
const VECTOR_DIM: usize = 3;

/// Valor de `k` de la consulta KNN de prueba.
const KNN_K: usize = 2;

/// Vector de consulta de las pruebas KNN.
const QUERY_VECTOR: [f64; VECTOR_DIM] = [0.1, 0.2, 0.3];

/// Offset del inicio del `body` dentro del PRIMER frame del WAL.
///
/// El frame es `[ len: u32 LE (4 B) | body | crc32c: u32 LE ]`; alterar un byte
/// del `body` del primer frame rompe su CRC sin confundirlo con la cola rasgada.
const WAL_FIRST_FRAME_BODY_OFFSET: u64 = 4;

/// Máscara XOR aplicada al byte alterado (garantiza un cambio distinto).
const CORRUPT_XOR_MASK: u8 = 0xFF;

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.db"));
    let database =
        Database::open(DbConfig::new(&path, POOL_CAPACITY)).expect("apertura de la base");
    (dir, database)
}

/// Deriva la ruta del WAL a partir del directorio temporal y el tag.
///
/// Args:
///     dir: Directorio temporal de la base.
///     tag: Sufijo del nombre de archivo de datos.
///
/// Returns:
///     La ruta del fichero `.wal` asociado (misma raíz, extensión `.wal`).
fn wal_path(dir: &tempfile::TempDir, tag: &str) -> PathBuf {
    dir.path().join(format!("{tag}.db")).with_extension("wal")
}

/// Crea `items(id INT, name TEXT)`.
///
/// Args:
///     database: Base destino.
///     table: Nombre de la tabla a crear.
fn create_items(database: &mut Database, table: &str) {
    database
        .create_table(
            table,
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

/// Crea un embedding L2 de la dimensión del vector.
///
/// Args:
///     values: Componentes del vector en `f64`.
///
/// Returns:
///     El embedding L2 construido.
fn embedding(values: &[f64]) -> Embedding {
    let floats: Vec<f32> = values.iter().map(|value| *value as f32).collect();
    Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "test".to_string(),
            dim: values.len(),
            metric: Metric::L2,
        },
    )
    .expect("embedding válido")
}

/// Construye un `Record` con escalares y vector (sin aristas).
///
/// Args:
///     id: Identificador del registro.
///     scalars: Escalares del registro.
///     vector: Componentes del embedding.
///
/// Returns:
///     El registro listo para `insert_record`.
fn vector_record(id: RecordId, scalars: ScalarMap, vector: &[f64]) -> Record {
    Record {
        id,
        scalars,
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(embedding(vector)),
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Construye la consulta KNN de prueba sobre `items`.
///
/// Returns:
///     El texto RQL `SELECT * FROM items KNN embedding <|k|> [...]`.
fn knn_query() -> String {
    format!(
        "SELECT * FROM items KNN embedding <|{KNN_K}|> [{}, {}, {}]",
        QUERY_VECTOR[0], QUERY_VECTOR[1], QUERY_VECTOR[2]
    )
}

/// Siembra `items` con filas vectoriales de ejemplo.
///
/// Args:
///     database: Base destino (debe tener la tabla `items`).
fn seed_vector_rows(database: &mut Database) {
    let data = [
        (1_i64, "origen", [0.0, 0.0, 0.0]),
        (2, "cerca", [0.1, 0.1, 0.1]),
        (3, "lejos", [9.0, 9.0, 9.0]),
        (4, "medio", [3.0, 3.0, 3.0]),
    ];
    for (id, name, vector) in data {
        let record = vector_record(RecordId::new(), scalars(id, name), &vector);
        database
            .insert_record("items", record)
            .expect("insert_record");
    }
}

/// Altera un byte del `body` del PRIMER frame del WAL en disco.
///
/// Rompe el CRC32C del frame sin tocar el final del fichero: al no ser la cola,
/// no puede interpretarse como una escritura rasgada legítima.
///
/// Args:
///     path: Ruta del fichero `.wal`.
fn corrupt_first_wal_frame(path: &Path) {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("abre el WAL para corromper");
    file.seek(SeekFrom::Start(WAL_FIRST_FRAME_BODY_OFFSET))
        .expect("seek al body del primer frame");
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).expect("lee el byte objetivo");
    byte[0] ^= CORRUPT_XOR_MASK;
    file.seek(SeekFrom::Start(WAL_FIRST_FRAME_BODY_OFFSET))
        .expect("vuelve al byte objetivo");
    file.write_all(&byte).expect("escribe el byte alterado");
    file.sync_all().expect("fsync del WAL alterado");
}

/// Indica si un detalle del reporte denuncia una anomalía de integridad.
///
/// Args:
///     detail: Línea de detalle del `IntegrityReport`.
///
/// Returns:
///     `true` si el detalle menciona un error o corrupción (case-insensitive).
fn is_error_detail(detail: &str) -> bool {
    let lowered = detail.to_ascii_lowercase();
    lowered.contains("error") || lowered.contains("corrupt")
}

/// AC-0062-01 — `integrity_check` sobre una base sana marca todo en `true`.
#[test]
// @spec AC-0062-01
fn test_ac_0062_01_integrity_ok_on_healthy_db() {
    let (_dir, mut database) = open_test_db("ac0062_01");
    create_items(&mut database, "items");
    seed_vector_rows(&mut database);

    let report: IntegrityReport = database.integrity_check().expect("integrity_check");

    assert!(report.wal_ok, "el WAL sano debe reportarse íntegro");
    assert!(report.heap_ok, "el heap sano debe reportarse coherente");
    assert!(
        report.vector_ok,
        "la cobertura del índice vectorial debe ser completa"
    );
    assert!(
        !report.details.iter().any(|detail| is_error_detail(detail)),
        "una base sana no debe producir detalles de error: {:?}",
        report.details
    );
}

/// AC-0062-02 — un byte alterado en el primer frame del WAL se detecta.
#[test]
// @spec AC-0062-02
fn test_ac_0062_02_integrity_detects_corrupt_wal() {
    let (dir, mut database) = open_test_db("ac0062_02");
    create_items(&mut database, "items");
    seed_vector_rows(&mut database);

    // Las inserciones auto-confirman: el WAL ya tiene frames en disco.
    let path = wal_path(&dir, "ac0062_02");
    assert!(path.exists(), "el WAL debe existir tras confirmar");
    corrupt_first_wal_frame(&path);

    let report: IntegrityReport = database
        .integrity_check()
        .expect("integrity_check no debe entrar en panic");

    assert!(
        !report.wal_ok,
        "un frame corrupto debe marcar el WAL como no íntegro"
    );
    assert!(
        !report.details.is_empty(),
        "la detección de corrupción debe ofrecer un detalle accionable"
    );
}

/// AC-0062-03 — con shadow reads el resultado KNN no cambia y no hay divergencia.
#[test]
// @spec AC-0062-03
fn test_ac_0062_03_shadow_parity_without_divergence() {
    let (_dir, mut database) = open_test_db("ac0062_03");
    create_items(&mut database, "items");
    seed_vector_rows(&mut database);
    let query = knn_query();

    let off = database.execute(&query).expect("KNN en modo OFF");

    database.set_shadow_reads(true);
    assert!(
        database.shadow_reads_enabled(),
        "set_shadow_reads(true) debe habilitar el modo"
    );
    let on = database.execute(&query).expect("KNN en modo ON");

    assert_eq!(on, off, "shadow reads no debe alterar el resultado servido");

    let report: ShadowReport = database.shadow_report();
    assert!(
        report.checks >= 1,
        "con shadow ON se debe verificar al menos una consulta"
    );
    assert!(
        report.divergences.is_empty(),
        "un índice consistente no debe producir divergencias: {:?}",
        report.divergences
    );
}

/// AC-0062-05 — shadow reads está OFF por defecto (coste nulo).
#[test]
// @spec AC-0062-05
fn test_ac_0062_05_shadow_off_by_default() {
    let (_dir, mut database) = open_test_db("ac0062_05");
    assert!(
        !database.shadow_reads_enabled(),
        "una base recién abierta debe tener shadow reads deshabilitado"
    );

    create_items(&mut database, "items");
    seed_vector_rows(&mut database);
    let query = knn_query();
    let _rows = database.execute(&query).expect("KNN sin shadow");

    let report: ShadowReport = database.shadow_report();
    assert_eq!(
        report.checks, 0,
        "sin habilitar shadow no debe haber verificaciones"
    );
}
