//! Spike `TableProvider` de DataFusion sobre filas nativas (SPEC-0060).
//!
//! Solo se compila con la feature `datafusion` (OFF por defecto). Convierte
//! las filas relacionales de RuscaDB (`ScalarValue`) a `RecordBatch` de Arrow
//! y las sirve a DataFusion mediante `MemorySourceConfig` + `DataSourceExec`
//! (el mismo camino que `MemTable` upstream), sin tocar el executor
//! propio (NF-0060-02). El registro y el planning son responsabilidad del
//! caller (tests del spike).
//!
//! Subset soportado: `Bool`, `Int`, `Float`, `Text` y `Null` (el null hereda
//! el tipo de la primera fila no-nula de su columna; columnas todas-null no
//! son inferibles y rechazan con error accionable).

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, BooleanBuilder, Float64Array, Int64Array, StringBuilder};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::catalog::{Session, TableProvider};
use datafusion::error::{DataFusionError, Result as DfResult};
use datafusion::logical_expr::{Expr, TableType};
use datafusion::physical_plan::ExecutionPlan;
use datafusion_datasource::memory::MemorySourceConfig;
use datafusion_datasource::source::DataSourceExec;
use ruscadb_core::ScalarValue;

/// Filas del spike: una fila es una lista de escalares alineada con `columns`.
pub type SpikeRow = Vec<ScalarValue>;

/// `TableProvider` en memoria sobre filas nativas de RuscaDB.
///
/// Diseño: la conversión `ScalarValue -> Arrow` ocurre en la construcción
/// (`try_new`); `scan` devuelve un `DataSourceExec` sobre un lote en memoria
/// con la proyección aplicada por el propio DataFusion.
#[derive(Debug)]
pub struct RuscaTableProvider {
    /// Esquema Arrow inferido de las columnas y las filas.
    schema: SchemaRef,
    /// Lote Arrow con todas las filas (una partición).
    batch: RecordBatch,
}

/// Inferencia de tipo por columna a partir de las filas no-nulas.
fn infer_data_type(values: &[&ScalarValue]) -> DfResult<DataType> {
    let mut kind: Option<DataType> = None;
    for value in values {
        let candidate = match value {
            ScalarValue::Null => continue,
            ScalarValue::Bool(_) => DataType::Boolean,
            ScalarValue::Int(_) | ScalarValue::UInt(_) | ScalarValue::TimestampMillis(_) => {
                DataType::Int64
            }
            ScalarValue::Float(_) => DataType::Float64,
            ScalarValue::Text(_) => DataType::Utf8,
            other => {
                return Err(DataFusionError::NotImplemented(format!(
                    "tipo no soportado en el spike SPEC-0060: {other:?}"
                )));
            }
        };
        if let Some(previous) = &kind {
            if previous != &candidate {
                return Err(DataFusionError::Plan(format!(
                    "tipos mixtos en una columna del spike: {previous:?} vs {candidate:?}"
                )));
            }
        }
        kind = Some(candidate);
    }
    kind.ok_or_else(|| {
        DataFusionError::Plan(
            "columna todas-null: el esquema no es inferible (especifica datos)".to_string(),
        )
    })
}

/// Construye la columna Arrow de un nombre dado su tipo y sus valores.
fn build_column(data_type: &DataType, values: &[&ScalarValue]) -> DfResult<ArrayRef> {
    match data_type {
        DataType::Int64 => {
            let mut builder = Int64Array::builder(values.len());
            for value in values {
                match value {
                    ScalarValue::Null => builder.append_null(),
                    ScalarValue::Int(inner) => builder.append_value(*inner),
                    ScalarValue::UInt(inner) => builder.append_value(*inner as i64),
                    ScalarValue::TimestampMillis(inner) => builder.append_value(*inner),
                    other => {
                        return Err(DataFusionError::Plan(format!(
                            "valor {other:?} no compatible con Int64"
                        )));
                    }
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Float64 => {
            let mut builder = Float64Array::builder(values.len());
            for value in values {
                match value {
                    ScalarValue::Null => builder.append_null(),
                    ScalarValue::Float(inner) => builder.append_value(*inner),
                    other => {
                        return Err(DataFusionError::Plan(format!(
                            "valor {other:?} no compatible con Float64"
                        )));
                    }
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Utf8 => {
            let mut builder = StringBuilder::with_capacity(values.len(), 0);
            for value in values {
                match value {
                    ScalarValue::Null => builder.append_null(),
                    ScalarValue::Text(inner) => builder.append_value(inner),
                    other => {
                        return Err(DataFusionError::Plan(format!(
                            "valor {other:?} no compatible con Utf8"
                        )));
                    }
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Boolean => {
            let mut builder = BooleanBuilder::with_capacity(values.len());
            for value in values {
                match value {
                    ScalarValue::Null => builder.append_null(),
                    ScalarValue::Bool(inner) => builder.append_value(*inner),
                    other => {
                        return Err(DataFusionError::Plan(format!(
                            "valor {other:?} no compatible con Boolean"
                        )));
                    }
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        other => Err(DataFusionError::NotImplemented(format!(
            "tipo del esquema no soportado en el spike: {other:?}"
        ))),
    }
}

impl RuscaTableProvider {
    /// Construye el provider infiriendo el esquema de los datos.
    ///
    /// Args:
    ///     columns: Nombres de columna en orden.
    ///     rows: Filas de escalares (`Bool`/`Int`/`Float`/`Text`/`Null`).
    ///
    /// Returns:
    ///     El provider listo para `SessionContext::register_table`.
    ///
    /// Errors:
    ///     [`DataFusionError::Plan`] si una fila no sigue el esquema, hay
    ///     tipos mixtos o una columna es toda-null (no inferible).
    pub fn try_new(columns: Vec<String>, rows: Vec<SpikeRow>) -> DfResult<Self> {
        let width = columns.len();
        for (index, row) in rows.iter().enumerate() {
            if row.len() != width {
                return Err(DataFusionError::Plan(format!(
                    "fila {index} tiene {} valores, se esperaban {width}",
                    row.len()
                )));
            }
        }
        // Transponer filas -> columnas para inferir y construir.
        let mut fields = Vec::with_capacity(width);
        let mut arrays: Vec<ArrayRef> = Vec::with_capacity(width);
        for (column_index, name) in columns.iter().enumerate() {
            let cells: Vec<&ScalarValue> = rows.iter().map(|row| &row[column_index]).collect();
            let data_type = infer_data_type(&cells)?;
            let array = build_column(&data_type, &cells)?;
            fields.push(Field::new(name, data_type, true));
            arrays.push(array);
        }
        let schema = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(schema.clone(), arrays)?;
        Ok(Self { schema, batch })
    }
}

#[async_trait::async_trait]
impl TableProvider for RuscaTableProvider {
    /// Esquema Arrow inferido en la construcción.
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }

    /// Tipo de tabla base (no vista ni gherkin).
    fn table_type(&self) -> TableType {
        TableType::Base
    }

    /// Escaneo: un `DataSourceExec` de una partición con proyección.
    ///
    /// Como `MemTable` upstream, `filters`/`limit` se ignoran: el planner de
    /// DataFusion añade los nodos de filtro/límite por encima del plan.
    async fn scan(
        &self,
        _state: &dyn Session,
        projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        _limit: Option<usize>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        let source = MemorySourceConfig::try_new(
            &[vec![self.batch.clone()]],
            self.schema.clone(),
            projection.cloned(),
        )?;
        // `from_data_source` ya devuelve `Arc<DataSourceExec>`.
        Ok(DataSourceExec::from_data_source(source))
    }
}

/// Tests unitarios de la inferencia de esquema (SPEC-0060).
#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::infer_data_type;
    use datafusion::arrow::datatypes::DataType;
    use ruscadb_core::ScalarValue;

    /// Un Int no-nulo infiere Int64; Null antes del valor no contamina.
    #[test]
    fn infer_int_with_leading_null() {
        let values = [&ScalarValue::Null, &ScalarValue::Int(7)];
        assert_eq!(infer_data_type(&values).expect("infer"), DataType::Int64);
    }

    /// Tipos mixtos rechazan con error accionable.
    #[test]
    fn infer_mixed_types_errors() {
        let values = [&ScalarValue::Int(1), &ScalarValue::Text("x".into())];
        let error = infer_data_type(&values).expect_err("tipos mixtos deben fallar");
        assert!(
            error.to_string().contains("mixtos"),
            "mensaje accionable: {error}"
        );
    }

    /// Columna toda-null no es inferible y rechaza con error accionable.
    #[test]
    fn infer_all_null_errors() {
        let values = [&ScalarValue::Null, &ScalarValue::Null];
        let error = infer_data_type(&values).expect_err("all-null debe fallar");
        assert!(
            error.to_string().contains("inferible"),
            "mensaje accionable: {error}"
        );
    }
}
