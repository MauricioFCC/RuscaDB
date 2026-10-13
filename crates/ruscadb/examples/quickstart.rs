//! Quickstart de RuscaDB: crear una base, una tabla y DML completo.
//!
//! Ejecutar con `cargo run --example quickstart`. La base vive en un
//! directorio temporal que se limpia al salir. Contrato SDD: SPEC-0087
//! (AC-0087-01).

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarValue};

/// Resumen del flujo quickstart, devuelto al caller para poder verificarlo.
#[derive(Debug)]
pub struct QuickstartReport {
    /// Nombres que cumplen `WHERE edad >= 18` tras los INSERT.
    pub mayores: Vec<String>,
    /// Nombres resultantes tras el `UPDATE ... WHERE id = 2`.
    pub tras_update: Vec<String>,
    /// Registros que quedan tras el `DELETE ... WHERE id = 3`.
    pub tras_delete: usize,
}

/// Ejecuta el flujo completo (DDL, INSERT, SELECT, UPDATE, DELETE).
///
/// Returns:
///     El resumen con las filas observadas en cada etapa del flujo.
///
/// Raises:
///     Los errores de apertura, DDL o DML se propagan en el `Result`.
pub fn run() -> Result<QuickstartReport, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("quickstart.db");
    let mut db = Database::open(DbConfig::new(&path, 64))?;

    db.create_table(
        "personas",
        vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            },
            ColumnDef {
                name: "nombre".to_string(),
                col_type: ColumnType::Text,
            },
            ColumnDef {
                name: "edad".to_string(),
                col_type: ColumnType::Int,
            },
        ],
    )?;

    db.execute(
        "INSERT INTO personas (id, nombre, edad) VALUES \
         (1, 'Ana', 34), (2, 'Luis', 17), (3, 'Mar', 22)",
    )?;

    let mayores = column_texts(
        &db.execute("SELECT * FROM personas WHERE edad >= 18")?,
        "nombre",
    )?;
    println!("mayores de edad: {mayores:?}");

    db.execute("UPDATE personas SET nombre = 'Luis (menor)' WHERE id = 2")?;
    let tras_update = column_texts(&db.execute("SELECT * FROM personas")?, "nombre")?;
    println!("tras UPDATE: {tras_update:?}");

    db.execute("DELETE FROM personas WHERE id = 3")?;
    let restantes = db.execute("SELECT * FROM personas")?;
    println!("tras DELETE quedan {} registros", restantes.len());
    let tras_delete = restantes.len();

    db.close()?;
    Ok(QuickstartReport {
        mayores,
        tras_update,
        tras_delete,
    })
}

/// Extrae una columna de texto de cada fila.
///
/// Args:
///     rows: Filas devueltas por `Database::execute`.
///     column: Nombre de la columna `Text` a extraer.
///
/// Returns:
///     Los valores de la columna en orden.
///
/// Raises:
///     El `Result` falla si la columna no existe o no es `Text`.
fn column_texts(rows: &[Row], column: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    rows.iter()
        .map(|row| match row.get(column) {
            Some(ScalarValue::Text(value)) => Ok(value.clone()),
            other => Err(format!("columna '{column}': se esperaba Text, llegó {other:?}").into()),
        })
        .collect()
}

/// Punto de entrada: ejecuta el ejemplo y sale con código 1 si falla.
fn main() {
    if let Err(err) = run() {
        eprintln!("quickstart: no se pudo completar el flujo: {err}");
        std::process::exit(1);
    }
}
