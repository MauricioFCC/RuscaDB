//! Integración end-to-end de FVS y el índice primario B+tree (SPEC-0024).
//!
//! Cubre los criterios AC-0024-01..05 y una propiedad PBT: el `KNN` con filtro
//! `WHERE` coincide siempre con la fuerza bruta restringida al filtro.

// El oráculo puede fallar ruidosamente (equivalente a `allow-expect-in-tests`
// del workspace, que no alcanza a los targets de integración).
#![allow(clippy::expect_used)]

use proptest::prelude::*;
use ruscadb::{
    ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Embedding, EmbeddingMeta, Metric, Record,
    RecordId, RecordMeta, ScalarMap, ScalarValue,
};

/// Abre una base temporal de pruebas con pool amplio (sin backpressure).
///
/// Args:
///     tag: Sufijo del nombre de archivo (aísla cada test).
///
/// Returns:
///     El directorio temporal (keep-alive) y la base abierta.
fn open_test_db(tag: &str) -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join(format!("{tag}.data"));
    let database = Database::open(DbConfig::new(&path, 128)).expect("apertura de la base");
    (dir, database)
}

/// Crea la tabla `items(id INT, name TEXT)`.
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

/// Inserta una fila `(id, name)` en `items` (auto-commit) y devuelve su id.
fn insert_row(database: &mut Database, id: i64, name: &str) -> RecordId {
    let scalars = ScalarMap::from([
        ("id".to_string(), ScalarValue::Int(id)),
        ("name".to_string(), ScalarValue::Text(name.to_string())),
    ]);
    database.insert("items", scalars).expect("insert")
}

/// Inserta una fila con vector en `items` (auto-commit) y devuelve su id.
fn insert_vector(database: &mut Database, id: i64, vector: &[f64]) -> RecordId {
    let floats: Vec<f32> = vector.iter().map(|value| *value as f32).collect();
    let record = Record {
        id: RecordId::new(),
        scalars: ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(id)),
            ("name".to_string(), ScalarValue::Text(format!("v{id}"))),
        ]),
        doc: None,
        edges: EdgeSet::default(),
        vector: Some(
            Embedding::new(
                floats,
                EmbeddingMeta {
                    model_id: "test".to_string(),
                    dim: vector.len(),
                    metric: Metric::L2,
                },
            )
            .expect("embedding válido"),
        ),
        blob: None,
        meta: RecordMeta::default(),
    };
    database
        .insert_record("items", record)
        .expect("insert_record")
}

/// Extrae el valor entero de la columna `id`.
fn int_of(row: &ruscadb::Row) -> i64 {
    match row.get("id") {
        Some(ScalarValue::Int(value)) => *value,
        other => panic!("se esperaba Int en 'id', se obtuvo {other:?}"),
    }
}

/// Distancia euclídea en `f32` (oráculo de fuerza bruta).
fn l2(left: &[f64], right: &[f64]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| (*a as f32 - *b as f32).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// Oráculo: ids del top-k exacto restringido al filtro `id >= cutoff`.
fn brute_force_filtered(
    entries: &[(i64, Vec<f64>)],
    query: &[f64],
    cutoff: i64,
    k: usize,
) -> Vec<i64> {
    let mut scored: Vec<(i64, f32)> = entries
        .iter()
        .filter(|(id, _)| *id >= cutoff)
        .map(|(id, vector)| (*id, l2(vector, query)))
        .collect();
    // Orden por (distancia, id) — mismo desempate que FVS.
    scored.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    scored.into_iter().take(k).map(|(id, _)| id).collect()
}

/// AC-0024-01 — un `KNN` con filtro `WHERE` usa FVS y devuelve el top-k exacto
/// restringido al filtro para selectividades baja, media y alta.
#[test]
fn test_ac_0024_01_knn_with_filter_uses_fvs() {
    let (_dir, mut database) = open_test_db("ac24_01");
    create_items(&mut database, "items");

    let count = 60_i64;
    let entries: Vec<(i64, Vec<f64>)> = (0..count)
        .map(|id| (id, vec![id as f64, ((id * 7) % 13) as f64]))
        .collect();
    for (id, vector) in &entries {
        insert_vector(&mut database, *id, vector);
    }

    let query = vec![3.0, 4.0];
    let query_clause = format!("[{}, {}]", query[0], query[1]);

    // Selectividad: < 0.05 => PreFilter; ~0.5 => InFilter; ~0.98 => PostFilter.
    for (cutoff, k) in [(58_i64, 2_usize), (30, 3), (1, 4)] {
        let sql = format!(
            "SELECT * FROM items WHERE id >= {cutoff} KNN embedding <|{k}|> {query_clause}"
        );
        let rows = database.execute(&sql).expect("KNN con filtro");
        let got: Vec<i64> = rows.iter().map(int_of).collect();
        let expected = brute_force_filtered(&entries, &query, cutoff, k);
        assert_eq!(
            got, expected,
            "cutoff={cutoff} k={k}: el KNN filtrado debe ser exacto"
        );
    }
}

/// FVS — si `PostFilter` no alcanza `k` vecinos permitidos (los más cercanos
/// están filtrados fuera), se recalcula con `PreFilter` y devuelve el top-k
/// exacto. Fuerza la rama de *fallback* de `fvs_top_k`.
#[test]
fn test_fvs_post_filter_under_return_falls_back_exact() {
    let (_dir, mut database) = open_test_db("ac24_post");
    create_items(&mut database, "items");

    // Cuatro vectores muy cercanos al query quedan fuera del filtro; el resto
    // (6/10 = 0.6) entra en `PostFilter`, cuyo sobre-muestreo top-4 no contiene
    // ningún permitido => `PostFilter` devuelve vacío y se cae a `PreFilter`.
    let entries: Vec<(i64, Vec<f64>)> = (0..10)
        .map(|id| {
            let vector = if id < 4 {
                vec![id as f64, 0.0]
            } else {
                vec![100.0, 0.0]
            };
            (id, vector)
        })
        .collect();
    for (id, vector) in &entries {
        insert_vector(&mut database, *id, vector);
    }

    let rows = database
        .execute("SELECT * FROM items WHERE id >= 4 KNN embedding <|1|> [0.0, 0.0]")
        .expect("KNN postfilter con fallback");
    let got: Vec<i64> = rows.iter().map(int_of).collect();
    assert_eq!(got, brute_force_filtered(&entries, &[0.0, 0.0], 4, 1));
    assert_eq!(got, vec![4], "el vecino permitido más cercano es el id 4");
}

/// AC-0024-02 — el borrado localiza la fila por el índice primario y mantiene
/// coherente la estructura `RecordId -> locator`.
#[test]
fn test_ac_0024_02_primary_index_point_delete() {
    let (_dir, mut database) = open_test_db("ac24_02");
    create_items(&mut database, "items");
    let ids: Vec<RecordId> = (0..5)
        .map(|index| insert_row(&mut database, index, "x"))
        .collect();

    assert_eq!(
        database.primary_index_len("items"),
        ids.len(),
        "el índice primario registra cada fila insertada"
    );
    let target = ids[3];
    assert!(
        database
            .get_record("items", &target)
            .expect("lookup")
            .is_some(),
        "el point lookup encuentra la fila antes del borrado"
    );

    assert!(database.delete("items", &target).expect("delete"));
    assert!(
        database
            .get_record("items", &target)
            .expect("lookup borrado")
            .is_none(),
        "el point lookup ya no encuentra la fila borrada"
    );
    assert_eq!(
        database.primary_index_len("items"),
        ids.len() - 1,
        "el borrado retira la entrada del índice primario"
    );
    assert!(
        !database
            .delete("items", &RecordId::new())
            .expect("delete ausente"),
        "borrar un id ausente es no-op"
    );
    assert_eq!(
        database
            .execute("SELECT * FROM items")
            .expect("select")
            .len(),
        ids.len() - 1
    );
}

/// AC-0024-03 — el índice primario se reconstruye al reabrir y el point lookup
/// sigue funcionando.
#[test]
fn test_ac_0024_03_primary_index_survives_reopen() {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let path = dir.path().join("ac24_03.data");
    let target;
    {
        let mut database = Database::open(DbConfig::new(&path, 128)).expect("open");
        create_items(&mut database, "items");
        let _first = insert_row(&mut database, 1, "x");
        let _second = insert_row(&mut database, 2, "y");
        target = insert_row(&mut database, 3, "z");
        assert_ne!(database.primary_index_len("items"), 0);
        database.close().expect("cierre");
    }

    let mut database = Database::open(DbConfig::new(&path, 128)).expect("reapertura");
    assert_eq!(
        database.primary_index_len("items"),
        3,
        "el índice primario se reconstruye desde el heap al abrir"
    );
    let record = database
        .get_record("items", &target)
        .expect("lookup tras reopen")
        .expect("la fila vive tras reabrir");
    assert_eq!(
        record.scalars.get("id"),
        Some(&ScalarValue::Int(3)),
        "el point lookup devuelve la fila correcta"
    );
    assert!(
        database
            .delete("items", &target)
            .expect("delete tras reopen")
    );
    assert!(
        database
            .get_record("items", &target)
            .expect("lookup borrado")
            .is_none()
    );
}

/// AC-0024-04 — tras el borrado el point lookup no ve la fila; reinsertar el
/// mismo id la vuelve a registrar.
#[test]
fn test_ac_0024_04_delete_then_reinsert_same_id() {
    let (_dir, mut database) = open_test_db("ac24_04");
    create_items(&mut database, "items");
    let id = insert_row(&mut database, 7, "x");

    assert!(database.delete("items", &id).expect("delete"));
    assert!(
        database
            .get_record("items", &id)
            .expect("lookup borrado")
            .is_none()
    );
    assert!(
        database
            .execute("SELECT * FROM items")
            .expect("select")
            .is_empty()
    );

    // Reinsertar el mismo id vuelve a registrarlo en el índice primario.
    let revived = Record {
        id,
        scalars: ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(7)),
            ("name".to_string(), ScalarValue::Text("x".to_string())),
        ]),
        doc: None,
        edges: EdgeSet::default(),
        vector: None,
        blob: None,
        meta: RecordMeta::default(),
    };
    database.insert_record("items", revived).expect("reinsert");

    assert!(
        database
            .get_record("items", &id)
            .expect("lookup revivido")
            .is_some(),
        "reinsertar el mismo id lo registra de nuevo"
    );
    assert_eq!(database.primary_index_len("items"), 1);
    assert_eq!(
        database
            .execute("SELECT * FROM items")
            .expect("select")
            .len(),
        1,
        "la versión borrada no reaparece; solo vive la nueva"
    );
}

/// AC-0024-05 — fronteras: índice vacío, filtro vacío, filtro total, `k = 0` e
/// id ausente se manejan sin pánicos.
#[test]
fn test_ac_0024_05_index_edge_cases() {
    let (_dir, mut database) = open_test_db("ac24_05");
    create_items(&mut database, "items");

    // Índice primario vacío.
    assert_eq!(database.primary_index_len("items"), 0);
    assert!(
        database
            .get_record("items", &RecordId::new())
            .expect("lookup vacío")
            .is_none()
    );
    assert!(
        !database
            .delete("items", &RecordId::new())
            .expect("delete vacío"),
        "borrar en un índice vacío es no-op"
    );

    let count = 6_i64;
    let entries: Vec<(i64, Vec<f64>)> = (0..count)
        .map(|id| (id, vec![id as f64, (id % 3) as f64]))
        .collect();
    let mut inserted = Vec::new();
    for (id, vector) in &entries {
        inserted.push(insert_vector(&mut database, *id, vector));
    }
    assert_eq!(database.primary_index_len("items"), count as usize);

    // Filtro que no selecciona nada => vacío.
    let none = database
        .execute("SELECT * FROM items WHERE id > 100 KNN embedding <|2|> [0.0, 0.0]")
        .expect("filtro vacío");
    assert!(none.is_empty());

    // k = 0 con filtro total => vacío.
    let zero = database
        .execute("SELECT * FROM items WHERE id >= 0 KNN embedding <|0|> [0.0, 0.0]")
        .expect("k=0");
    assert!(zero.is_empty());

    // Filtro total => top-k exacto.
    let query = vec![0.0, 0.0];
    let total = database
        .execute("SELECT * FROM items WHERE id >= 0 KNN embedding <|6|> [0.0, 0.0]")
        .expect("filtro total");
    let got: Vec<i64> = total.iter().map(int_of).collect();
    assert_eq!(
        got,
        brute_force_filtered(&entries, &query, 0, count as usize)
    );

    // Borrar todas deja el índice primario vacío y el heap consultable.
    for id in &inserted {
        assert!(
            database.delete("items", id).expect("delete"),
            "cada fila viva se borra por su id"
        );
    }
    assert_eq!(database.primary_index_len("items"), 0);
    assert!(
        database
            .execute("SELECT * FROM items")
            .expect("select final")
            .is_empty()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// PBT: el `KNN` con filtro coincide con la fuerza bruta filtrada para
    /// cualquier umbral, consulta y `k` (incluye filtro vacío y `k = 0`).
    #[test]
    fn prop_knn_with_filter_matches_bruteforce(
        raw_vectors in prop::collection::vec(prop::collection::vec(0u32..40, 2), 1..7),
        raw_query in prop::collection::vec(0u32..40, 2),
        cutoff_raw in 0u32..8,
        k in 0usize..6,
    ) {
        let vectors: Vec<Vec<f64>> = raw_vectors
            .iter()
            .map(|vector| vector.iter().map(|value| f64::from(*value) / 2.0).collect())
            .collect();
        let query: Vec<f64> = raw_query.iter().map(|value| f64::from(*value) / 2.0).collect();
        let cutoff = i64::from(cutoff_raw); // 0..8: cubre filtro total y filtro vacío

        let (_dir, mut database) = open_test_db("ac24_prop");
        create_items(&mut database, "items");
        let entries: Vec<(i64, Vec<f64>)> = vectors
            .iter()
            .enumerate()
            .map(|(index, vector)| (index as i64, vector.clone()))
            .collect();
        for (id, vector) in &entries {
            insert_vector(&mut database, *id, vector);
        }

        let sql = format!(
            "SELECT * FROM items WHERE id >= {cutoff} KNN embedding <|{k}|> [{}, {}]",
            query[0], query[1]
        );
        let rows = database.execute(&sql).expect("KNN filtrado");
        let got: Vec<i64> = rows.iter().map(int_of).collect();
        let expected = brute_force_filtered(&entries, &query, cutoff, k);
        prop_assert_eq!(got, expected);
    }
}
