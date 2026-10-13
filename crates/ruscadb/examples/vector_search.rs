//! Búsqueda vectorial (`KNN`) y full-text (`MATCH`) sobre la misma tabla.
//!
//! Ejecutar con `cargo run --example vector_search`. Contrato SDD:
//! SPEC-0087 (AC-0087-02).

use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, Row, ScalarMap, ScalarValue,
};

/// Resumen de las dos búsquedas ejecutadas por el ejemplo.
#[derive(Debug)]
pub struct VectorSearchReport {
    /// Títulos devueltos por `KNN` (ordenados por distancia).
    pub knn_titles: Vec<String>,
    /// Títulos devueltos por `MATCH` (ordenados por BM25).
    pub match_titles: Vec<String>,
}

/// Inserta tres documentos con embeddings y ejecuta `KNN` + `MATCH`.
///
/// Returns:
///     El resumen con los títulos de ambas búsquedas.
///
/// Raises:
///     Los errores de apertura, DDL, inserción o consulta se propagan.
pub fn run() -> Result<VectorSearchReport, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vector_search.db");
    let mut db = Database::open(DbConfig::new(&path, 64))?;

    db.create_table(
        "docs",
        vec![
            ColumnDef {
                name: "id".to_string(),
                col_type: ColumnType::Int,
            },
            ColumnDef {
                name: "titulo".to_string(),
                col_type: ColumnType::Text,
            },
        ],
    )?;

    let docs: [(i64, &str, [f64; 2]); 3] = [
        (1, "gato casero", [0.0, 0.0]),
        (2, "perro grande", [5.0, 5.0]),
        (3, "gato montes", [9.0, 9.0]),
    ];
    for (id, titulo, vector) in docs {
        db.insert_record("docs", document(id, titulo, vector))?;
    }

    let knn = db.execute("SELECT * FROM docs KNN embedding <|2|> [0.0, 0.0]")?;
    let knn_titles = column_texts(&knn, "titulo")?;
    println!("KNN top-2: {knn_titles:?}");

    let matches = db.execute("SELECT * FROM docs WHERE MATCH(titulo, 'gato')")?;
    let match_titles = column_texts(&matches, "titulo")?;
    println!("MATCH 'gato': {match_titles:?}");

    db.close()?;
    Ok(VectorSearchReport {
        knn_titles,
        match_titles,
    })
}

/// Construye un `Record` con escalares y embedding L2.
///
/// Args:
///     id: Identificador de negocio del documento.
///     titulo: Título textual del documento.
///     vector: Componentes del embedding en `f64`.
///
/// Returns:
///     El registro listo para `Database::insert_record`.
fn document(id: i64, titulo: &str, vector: [f64; 2]) -> Record {
    let floats: Vec<f32> = vector.iter().map(|value| *value as f32).collect();
    let embedding = Embedding::new(
        floats,
        EmbeddingMeta {
            model_id: "example".to_string(),
            dim: vector.len(),
            metric: Metric::L2,
        },
    );
    let vector = match embedding {
        Ok(value) => value,
        Err(err) => panic!("embedding de ejemplo inválido: {err}"),
    };
    Record {
        id: RecordId::new(),
        scalars: ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("titulo".to_string(), ScalarValue::Text(titulo.to_string())),
        ]),
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(vector),
        blob: None,
        meta: RecordMeta::default(),
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
        eprintln!("vector_search: no se pudo completar el flujo: {err}");
        std::process::exit(1);
    }
}
