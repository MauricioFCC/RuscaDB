//! Grafo ligero sobre tablas: aristas en los registros + `TRAVERSE`.
//!
//! Ejecutar con `cargo run --example graph_traverse`. Contrato SDD:
//! SPEC-0087 (AC-0087-03).

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, Edge, EdgeSet, Record, RecordId, RecordMeta, Row,
    ScalarMap, ScalarValue,
};

/// Resumen del recorrido de grafo a dos profundidades.
#[derive(Debug)]
pub struct GraphReport {
    /// Nombres alcanzados con `TRAVERSE edges DEPTH 2`.
    pub depth2_names: Vec<String>,
    /// Nombres alcanzados con `TRAVERSE edges DEPTH 1`.
    pub depth1_names: Vec<String>,
}

/// Crea la cadena a → b → c más un nodo aislado y la recorre.
///
/// Returns:
///     El resumen con los nombres vistos a cada profundidad.
///
/// Raises:
///     Los errores de apertura, DDL, inserción o consulta se propagan.
pub fn run() -> Result<GraphReport, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("graph_traverse.db");
    let mut db = Database::open(DbConfig::new(&path, 64))?;

    db.create_table(
        "nodes",
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
    )?;

    let a = RecordId::new();
    let b = RecordId::new();
    let c = RecordId::new();
    let aislado = RecordId::new();
    for record in [
        node(a, 1, "a", vec![edge(b)]),
        node(b, 2, "b", vec![edge(c)]),
        node(c, 3, "c", Vec::new()),
        node(aislado, 4, "d", Vec::new()),
    ] {
        db.insert_record("nodes", record)?;
    }

    let deep = db.execute("SELECT * FROM nodes TRAVERSE edges DEPTH 2")?;
    let depth2_names = column_texts(&deep, "name")?;
    println!("TRAVERSE DEPTH 2: {depth2_names:?}");

    let shallow = db.execute("SELECT * FROM nodes TRAVERSE edges DEPTH 1")?;
    let depth1_names = column_texts(&shallow, "name")?;
    println!("TRAVERSE DEPTH 1: {depth1_names:?}");

    db.close()?;
    Ok(GraphReport {
        depth2_names,
        depth1_names,
    })
}

/// Construye un nodo con aristas salientes.
///
/// Args:
///     id: Identificador estable del nodo.
///     business_id: Valor de la columna `id`.
///     name: Nombre del nodo (columna `name`).
///     outgoing: Aristas salientes del nodo.
///
/// Returns:
///     El registro listo para `Database::insert_record`.
fn node(id: RecordId, business_id: i64, name: &str, outgoing: Vec<Edge>) -> Record {
    Record {
        id,
        scalars: ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(business_id)),
            ("name".to_string(), ScalarValue::Text(name.to_string())),
        ]),
        doc: None,
        edges: EdgeSet {
            out: outgoing,
            incoming: Vec::new(),
        },
        vector: None,
        blob: None,
        meta: RecordMeta::default(),
    }
}

/// Crea una arista `link` hacia otro nodo.
///
/// Args:
///     target: Nodo destino de la arista.
///
/// Returns:
///     La arista saliente.
fn edge(target: RecordId) -> Edge {
    Edge {
        label: "link".to_string(),
        node: target,
    }
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
        eprintln!("graph_traverse: no se pudo completar el flujo: {err}");
        std::process::exit(1);
    }
}
