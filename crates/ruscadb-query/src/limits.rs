//! Límites de recursos del planner (SPEC-0054, roadmap §6.4).
//!
//! Coste estático pre-ejecución (defensa DoS de la superficie RQL): profundidad
//! máxima del AST, budget de complejidad en unidades abstractas, `LIMIT`
//! implícito, timeout cooperativo por defecto y umbral de aviso de full-scan.
//! Todo el modelo es puro (sin E/S) y usa aritmética saturante: el coste nunca
//! puede desbordar ni entrar en pánico.

use ruscadb_core::RuscaError;

use crate::ast::{Expr, Projection, Select};

/// Profundidad máxima del AST de un `SELECT` (guard clause del parseo).
pub const MAX_AST_DEPTH: usize = 64;
/// Budget de complejidad en unidades abstractas (rechazo pre-ejecución).
pub const COMPLEXITY_BUDGET: u64 = 100_000;
/// Límite inyectado cuando el `SELECT` no trae `LIMIT` (§6.4).
pub const IMPLICIT_LIMIT: u64 = 1000;
/// Timeout cooperativo por defecto de una query (ms).
pub const DEFAULT_TIMEOUT_MILLIS: u64 = 5000;
/// Filas estimadas a partir de las cuales el full-scan avisa en `EXPLAIN` (R9).
pub const FULL_SCAN_WARN_THRESHOLD: u64 = 1000;

/// Coste de una comparación escalar del `WHERE`.
const COST_COMPARISON: u64 = 100;
/// Coste de un `MATCH` full-text (tokenizar + rank BM25).
const COST_MATCH: u64 = 500;
/// Coste de una extracción documental `->`.
const COST_DOC_EXTRACT: u64 = 10;
/// Coste de una contención documental `@>` (parsea JSON al ejecutar).
const COST_DOC_CONTAINS: u64 = 100;
/// Coste por dimensión del vector de consulta `KNN`.
const COST_KNN_DIM: u64 = 10;
/// Coste por nivel de profundidad de `TRAVERSE`.
const COST_TRAVERSE_DEPTH: u64 = 100;
/// Coste por clave de `GROUP BY`.
const COST_GROUP_KEY: u64 = 10;
/// Coste por agregado del `SELECT`.
const COST_AGGREGATE: u64 = 50;
/// Coste por condición del `HAVING`.
const COST_HAVING: u64 = 50;
/// Coste fijo de un `JOIN` (nested-loop).
const COST_JOIN: u64 = 1000;
/// Coste de `ORDER BY` (ordenación).
const COST_ORDER_BY: u64 = 10;
/// Coste por columna proyectada.
const COST_PROJECTION_COL: u64 = 1;
/// Coste de `SELECT *` (columnas desconocidas en el planner).
const COST_PROJECTION_ALL: u64 = 10;

/// Profundidad de una expresión (hojas = 1; `AND`/comparaciones anidan).
///
/// Args:
///     expression: Expresión a medir.
///
/// Returns:
///     Niveles del árbol (una cadena AND de N predicados mide N+1).
pub fn expr_depth(expression: &Expr) -> usize {
    match expression {
        Expr::Column(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Text(_)
        | Expr::Match { .. }
        | Expr::DocExtract { .. }
        | Expr::DocContains { .. } => 1,
        Expr::Compare { left, right, .. } => 1 + expr_depth(left).max(expr_depth(right)),
        Expr::And(left, right) => 1 + expr_depth(left).max(expr_depth(right)),
    }
}

/// Profundidad máxima del `SELECT` (su filtro; el resto de cláusulas es plano).
///
/// Args:
///     select: Consulta analizada.
///
/// Returns:
///     Profundidad (≥ 1).
pub fn select_depth(select: &Select) -> usize {
    select.filter.as_ref().map(expr_depth).unwrap_or(1)
}

/// Verifica la profundidad contra [`MAX_AST_DEPTH`].
///
/// Args:
///     select: Consulta analizada.
///
/// Returns:
///     `Ok(())` si cabe; `Err(mensaje)` accionable en caso contrario.
pub fn check_depth(select: &Select) -> Result<(), String> {
    let depth = select_depth(select);
    if depth > MAX_AST_DEPTH {
        return Err(format!(
            "profundidad del AST {depth} excede el máximo {MAX_AST_DEPTH} (simplifica la consulta)"
        ));
    }
    Ok(())
}

/// Coste de una expresión en unidades abstractas (saturante).
fn expr_cost(expression: &Expr) -> u64 {
    match expression {
        Expr::Column(_) | Expr::Int(_) | Expr::Float(_) | Expr::Text(_) => 1,
        Expr::Compare { left, right, .. } => COST_COMPARISON
            .saturating_add(expr_cost(left))
            .saturating_add(expr_cost(right)),
        Expr::And(left, right) => 1u64
            .saturating_add(expr_cost(left))
            .saturating_add(expr_cost(right)),
        Expr::Match { .. } => COST_MATCH,
        Expr::DocExtract { .. } => COST_DOC_EXTRACT,
        Expr::DocContains { .. } => COST_DOC_CONTAINS,
    }
}

/// Complejidad de un `SELECT` en unidades abstractas (saturante).
///
/// Modelo documentado: base 1 + proyección + 100 por comparación + 500 por
/// `MATCH` + 10 por dimensión KNN + 100 por nivel de `TRAVERSE` + 10 por clave
/// de grupo + 50 por agregado/condición HAVING + 1000 por JOIN + 10 por
/// `ORDER BY`.
///
/// Args:
///     select: Consulta analizada.
///
/// Returns:
///     Coste total (nunca desborda: aritmética saturante).
pub fn complexity(select: &Select) -> u64 {
    let mut total = 1u64;
    total = total.saturating_add(match &select.projection {
        Projection::All => COST_PROJECTION_ALL,
        Projection::Columns(columns) => COST_PROJECTION_COL.saturating_mul(columns.len() as u64),
    });
    total = total.saturating_add(COST_AGGREGATE.saturating_mul(select.aggregates.len() as u64));
    if let Some(filter) = &select.filter {
        total = total.saturating_add(expr_cost(filter));
    }
    if let Some(knn) = &select.knn {
        total = total.saturating_add(COST_KNN_DIM.saturating_mul(knn.query.len() as u64));
    }
    if let Some(traverse) = &select.traverse {
        total = total.saturating_add(COST_TRAVERSE_DEPTH.saturating_mul(traverse.depth as u64));
    }
    total = total.saturating_add(COST_GROUP_KEY.saturating_mul(select.group_by.len() as u64));
    total = total.saturating_add(COST_HAVING.saturating_mul(select.having.len() as u64));
    if select.join.is_some() {
        total = total.saturating_add(COST_JOIN);
    }
    if select.order_by.is_some() {
        total = total.saturating_add(COST_ORDER_BY);
    }
    total
}

/// Verifica el budget contra [`COMPLEXITY_BUDGET`] (rechazo pre-ejecución).
///
/// Args:
///     select: Consulta analizada.
///
/// Returns:
///     `Ok(())` si cabe en el budget.
///
/// Errors:
///     [`RuscaError::ResourceLimit`] con recurso `query_complexity` si excede.
pub fn check_budget(select: &Select) -> Result<(), RuscaError> {
    let cost = complexity(select);
    if cost > COMPLEXITY_BUDGET {
        return Err(RuscaError::ResourceLimit {
            resource: "query_complexity".to_string(),
            limit: COMPLEXITY_BUDGET,
            actual: cost,
        });
    }
    Ok(())
}
