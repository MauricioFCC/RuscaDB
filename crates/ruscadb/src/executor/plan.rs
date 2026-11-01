//! Planificacion de acceso (`Plan`, `plan_for`, `EXPLAIN`).

use super::*;

/// Aviso de full-scan sobre el umbral para `EXPLAIN` (R9, SPEC-0054).
///
/// Args:
///     plan: Plan elegido por [`plan_for`].
///     estimated_rows: Filas estimadas de la tabla.
///
/// Returns:
///     El aviso si hay `FullScan` con filas >= [`FULL_SCAN_WARN_THRESHOLD`].
pub(crate) fn full_scan_warning(plan: &Plan, estimated_rows: u64) -> Option<String> {
    if matches!(plan, Plan::FullScan) && estimated_rows >= FULL_SCAN_WARN_THRESHOLD {
        Some(format!(
            "WARNING full-scan over ~{estimated_rows} rows (threshold {FULL_SCAN_WARN_THRESHOLD}): considera un índice o un LIMIT"
        ))
    } else {
        None
    }
}
/// Plan de acceso a una tabla.
///
/// `KnnScan`/`TraverseScan` señalan que la consulta incluye las cláusulas
/// `KNN`/`TRAVERSE`: la base se obtiene por recorrido del heap (o por índice de
/// igualdad) y la cláusula se resuelve después contra los índices en memoria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Recorrido completo del heap.
    FullScan,
    /// Búsqueda de igualdad en el índice de la columna.
    IndexScan {
        /// Columna indexada usada.
        column: String,
    },
    /// La consulta pide vecinos `KNN` (resueltos por el índice HNSW).
    KnnScan {
        /// Columna de embedding pedida.
        column: String,
    },
    /// La consulta pide un recorrido `TRAVERSE` (resuelto por el CSR).
    TraverseScan {
        /// Columna de aristas pedida.
        column: String,
    },
}
/// Elige el plan de acceso para un `SELECT`.
///
/// Prioridad: `KNN` > `TRAVERSE` > `IndexScan` (filtro `columna = literal`
/// sobre la columna indexada) > `FullScan`.
///
/// Args:
///     select: Consulta analizada.
///     catalog: Catálogo con las tablas e índices.
///
/// Returns:
///     El [`Plan`] de acceso elegido.
///
/// Errors:
///     [`RuscaError::TableNotFound`] si la tabla no existe.
pub(crate) fn plan_for(select: &Select, catalog: &Catalog) -> Result<Plan, RuscaError> {
    let table = catalog.get(&select.from)?;
    if let Some(knn) = &select.knn {
        return Ok(Plan::KnnScan {
            column: knn.column.clone(),
        });
    }
    if let Some(traverse) = &select.traverse {
        return Ok(Plan::TraverseScan {
            column: traverse.column.clone(),
        });
    }
    // SPEC-0104: varios índices por tabla; se usa el primero cuya columna
    // aparezca en una igualdad del `WHERE`.
    for column in table.index_columns() {
        if find_eq_literal(select.filter.as_ref(), &column).is_some() {
            return Ok(Plan::IndexScan { column });
        }
    }
    Ok(Plan::FullScan)
}
#[cfg(test)]
mod planner_tests {
    //! Tests del planner (SPEC-0054): timeout cooperativo y aviso de full-scan.

    use super::{Plan, QueryBudget, execute_with_plan_at, full_scan_warning};
    use crate::catalog::{ColumnDef, ColumnType};
    use crate::database::{Database, DbConfig};
    use ruscadb_core::{RuscaError, ScalarMap, ScalarValue};
    use ruscadb_query::parse;

    /// Abre una base temporal con `t(a INT)` y 10 filas.
    pub(crate) fn open_seeded_db(tag: &str) -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join(format!("{tag}.db"));
        let mut database = Database::open(DbConfig::new(&path, 128)).expect("apertura");
        database
            .create_table(
                "t",
                vec![ColumnDef {
                    name: "a".to_string(),
                    col_type: ColumnType::Int,
                }],
            )
            .expect("create_table");
        for index in 0..10 {
            database
                .insert(
                    "t",
                    ScalarMap::from([("a".to_string(), ScalarValue::Int(index))]),
                )
                .expect("insert");
        }
        (dir, database)
    }

    /// AC-0054-04 — deadline vencido aborta el scan con `QueryTimeout`.
    #[test]
    // @spec AC-0054-04
    pub(crate) fn test_ac_0054_04_timeout_cancels_scan() {
        let (_dir, mut database) = open_seeded_db("ac0054_04");
        let select = parse("SELECT * FROM t").expect("parse");
        let snapshot = database.snapshot();
        let error = execute_with_plan_at(
            &mut database,
            &select,
            Plan::FullScan,
            &snapshot,
            &QueryBudget::expired(),
        )
        .expect_err("deadline vencido debe abortar");
        assert!(
            matches!(error, RuscaError::QueryTimeout { .. }),
            "se esperaba QueryTimeout, se obtuvo {error:?}"
        );

        // Con budget fresco el mismo scan funciona (sin falsos timeouts).
        let rows = execute_with_plan_at(
            &mut database,
            &select,
            Plan::FullScan,
            &snapshot,
            &QueryBudget::default_budget(),
        )
        .expect("budget fresco");
        assert_eq!(rows.len(), 10);
    }

    /// BVA del aviso puro: umbral exacto y planes no-FullScan.
    #[test]
    pub(crate) fn full_scan_warning_fires_over_threshold() {
        assert!(
            full_scan_warning(&Plan::FullScan, 1000).is_some(),
            "en el umbral avisa"
        );
        assert!(
            full_scan_warning(&Plan::FullScan, 999).is_none(),
            "bajo el umbral calla"
        );
        assert!(
            full_scan_warning(
                &Plan::IndexScan {
                    column: "a".to_string()
                },
                999_999
            )
            .is_none(),
            "con índice nunca avisa"
        );
    }
}
