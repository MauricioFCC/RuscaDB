//! Planificador y ejecutor `SELECT` (SPEC-0012, FR-0012-04/05).
//!
//! [`plan_for`] elige `IndexScan` cuando el filtro contiene `columna =
//! literal` sobre la columna indexada; en otro caso `FullScan`.
//! [`execute_select`] evalúa filtro (con coerción numérica `Int↔Float`),
//! proyección y `LIMIT`. `NULL` excluye la fila y los tipos incompatibles
//! devuelven [`RuscaError::TypeMismatch`].

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ruscadb_core::{Metric, Record, RecordId, RuscaError, ScalarMap, ScalarValue};
use ruscadb_query::limits::{
    DEFAULT_TIMEOUT_MILLIS, FULL_SCAN_WARN_THRESHOLD, IMPLICIT_LIMIT, check_budget,
};
use ruscadb_query::{
    AggFunc, Aggregate, CompareOp, Expr, HavingCondition, KnnClause, KnnMode, OrderBy, Projection,
    Select, TraverseClause,
};
use ruscadb_txn::{Snapshot, Version};

use crate::catalog::{Catalog, ColumnType, TableDef};
use crate::database::Database;
use crate::document::{doc_contains, extract_doc_scalar};
use crate::heap::{heap_read, heap_scan};
use crate::index::{canonical_key, index_lookup_eq};

mod aggregate;
mod budget;
mod eval;
mod execute;
mod knn;
mod plan;
mod rows;
mod subquery;
mod vtable;

pub(crate) use aggregate::*;
pub(crate) use budget::*;
pub(crate) use eval::*;
pub(crate) use execute::*;
pub(crate) use knn::*;
pub use plan::Plan;
pub(crate) use plan::full_scan_warning;
pub(crate) use plan::plan_for;
pub(crate) use rows::*;
pub(crate) use subquery::*;

/// Fila proyectada: nombre de columna → valor escalar.
pub type Row = BTreeMap<String, ScalarValue>;
