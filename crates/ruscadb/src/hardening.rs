//! Hardening operativo: integridad y shadow reads (SPEC-0062).
//!
//! Una base embebida no tiene un DBA que la vigile, así que se autoverifica:
//!
//! - [`Database::integrity_check`]: valida el CRC32C de cada frame del WAL, la
//!   coherencia del heap (slots) y la cobertura del índice vectorial, sin
//!   entrar en panic.
//! - Shadow reads (`Database::set_shadow_reads`): cada KNN se re-ejecuta por el
//!   camino exacto (brute-force con `ruscadb_vector::distance`) y se comparan
//!   los ids contra la ruta indexada. La divergencia se registra
//!   ([`ShadowReport`]) y **nunca** altera el resultado servido.

use std::collections::BTreeSet;

use ruscadb_core::{RecordId, RuscaError};

use crate::catalog::{Catalog, TableDef};
use crate::database::Database;
use crate::heap::heap_scan;

/// Reporte de integridad de una base abierta (SPEC-0062).
///
/// Cada bandera en `false` viene acompañada de una línea en `details` con el
/// componente afectado y la causa (WHAT+WHY+WHERE).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    /// `true` si todos los frames del WAL decodifican con CRC32C válido.
    pub wal_ok: bool,
    /// `true` si el heap de todas las tablas escanea con slots coherentes.
    pub heap_ok: bool,
    /// `true` si el índice vectorial cubre exactamente las filas vivas vectoriales.
    pub vector_ok: bool,
    /// Detalles accionables de los componentes en fallo (vacío si todo va bien).
    pub details: Vec<String>,
}

/// Divergencia detectada por shadow reads (SPEC-0062).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowDivergence {
    /// Etiqueta de la consulta verificada (columna y `k`).
    pub query: String,
    /// Top-k calculado por el camino exacto (oráculo brute-force).
    pub expected: Vec<RecordId>,
    /// Top-k devuelto por la ruta indexada (el que se sirvió).
    pub got: Vec<RecordId>,
}

/// Acumulador del modo shadow: verificaciones y divergencias (SPEC-0062).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShadowReport {
    /// Número de consultas KNN verificadas desde que se habilitó el modo.
    pub checks: usize,
    /// Divergencias acumuladas (una por consulta discrepante).
    pub divergences: Vec<ShadowDivergence>,
}

impl ShadowReport {
    /// Registra una verificación y, si procede, su divergencia.
    ///
    /// La comparación es por conjuntos de ids (el orden del índice puede
    /// diferir en empates sin ser una divergencia real).
    ///
    /// Args:
    ///     query: Etiqueta de la consulta verificada.
    ///     expected: Top-k del camino exacto.
    ///     got: Top-k servido por la ruta indexada.
    pub(crate) fn record_check(
        &mut self,
        query: String,
        expected: Vec<RecordId>,
        got: Vec<RecordId>,
    ) {
        self.checks += 1;
        if !same_ids(&expected, &got) {
            self.divergences.push(ShadowDivergence {
                query,
                expected,
                got,
            });
        }
    }
}

/// Compara dos listas de ids como conjuntos.
///
/// Args:
///     left: Primera lista.
///     right: Segunda lista.
///
/// Returns:
///     `true` si contienen exactamente los mismos ids (sin importar el orden).
fn same_ids(left: &[RecordId], right: &[RecordId]) -> bool {
    left.len() == right.len()
        && left.iter().collect::<BTreeSet<_>>() == right.iter().collect::<BTreeSet<_>>()
}

impl Database {
    /// Verifica la integridad del WAL, del heap y del índice vectorial.
    ///
    /// No modifica el estado servido: solo lee y reporta. Pensado para
    /// operación (chequeo periódico) y diagnóstico post-incidente.
    ///
    /// Returns:
    ///     El [`IntegrityReport`] con una bandera por componente y detalles
    ///     accionables de los que fallen.
    ///
    /// Errors:
    ///     [`RuscaError::CorruptManifest`] si el catálogo persistido es ilegible
    ///     (no se puede enumerar tablas); la corrupción de WAL/heap/índice se
    ///     reporta en `details`, nunca como panic.
    pub fn integrity_check(&mut self) -> Result<IntegrityReport, RuscaError> {
        let mut report = IntegrityReport {
            wal_ok: true,
            heap_ok: true,
            vector_ok: true,
            details: Vec::new(),
        };
        self.check_wal(&mut report);
        let tables = load_tables(self)?;
        for table in &tables {
            self.check_table_heap(table, &mut report);
            self.check_table_vectors(table, &mut report);
        }
        Ok(report)
    }

    /// Habilita o deshabilita la verificación por shadow reads.
    ///
    /// Args:
    ///     enabled: `true` activa la re-ejecución exacta de cada KNN.
    pub fn set_shadow_reads(&mut self, enabled: bool) {
        self.shadow_enabled = enabled;
    }

    /// Indica si el modo shadow está activo.
    ///
    /// Returns:
    ///     `true` si cada KNN se re-verifica contra el camino exacto.
    pub fn shadow_reads_enabled(&self) -> bool {
        self.shadow_enabled
    }

    /// Devuelve una copia del acumulador de shadow reads.
    ///
    /// Returns:
    ///     El [`ShadowReport`] con verificaciones y divergencias acumuladas.
    pub fn shadow_report(&self) -> ShadowReport {
        self.shadow.clone()
    }

    /// Registra una verificación de shadow (uso interno del executor).
    ///
    /// Args:
    ///     query: Etiqueta de la consulta verificada.
    ///     expected: Top-k del camino exacto.
    ///     got: Top-k servido por la ruta indexada.
    pub(crate) fn record_shadow_check(
        &mut self,
        query: String,
        expected: Vec<RecordId>,
        got: Vec<RecordId>,
    ) {
        self.shadow.record_check(query, expected, got);
    }

    /// Verifica el CRC32C de todos los frames del WAL.
    ///
    /// Args:
    ///     report: Reporte en construcción (se anota el fallo si lo hay).
    fn check_wal(&self, report: &mut IntegrityReport) {
        let key = self.encryption_key();
        match ruscadb_wal::verify(self.wal_path(), key.as_ref()) {
            Ok(verification) if !verification.corrupted => {}
            Ok(verification) => {
                report.wal_ok = false;
                report.details.push(format!(
                    "WAL corrupto en '{}': frame inválido en el offset {} ({} bytes de cola)",
                    self.wal_path().display(),
                    verification
                        .corrupt_offset
                        .unwrap_or(verification.valid_offset),
                    verification.trailing_bytes
                ));
            }
            Err(error) => {
                report.wal_ok = false;
                report.details.push(format!(
                    "WAL no verificable en '{}': {error}",
                    self.wal_path().display()
                ));
            }
        }
    }

    /// Escanea el heap de una tabla para detectar slots corruptos.
    ///
    /// Args:
    ///     table: Definición de la tabla a escanear.
    ///     report: Reporte en construcción (se anota el fallo si lo hay).
    fn check_table_heap(&mut self, table: &TableDef, report: &mut IntegrityReport) {
        if let Err(error) = heap_scan(self, table) {
            report.heap_ok = false;
            report
                .details
                .push(format!("heap no íntegro en '{}': {error}", table.name));
        }
    }

    /// Comprueba que el índice vectorial cubre las filas vivas vectoriales.
    ///
    /// Args:
    ///     table: Definición de la tabla a comprobar.
    ///     report: Reporte en construcción (se anota el fallo si lo hay).
    fn check_table_vectors(&mut self, table: &TableDef, report: &mut IntegrityReport) {
        let Ok(rows) = heap_scan(self, table) else {
            return; // El fallo de heap ya se reportó; evita duplicar detalles.
        };
        let live_vectors = rows
            .iter()
            .filter(|(_, record)| record.meta.deleted_tx.is_none() && record.vector.is_some())
            .count();
        let indexed = self
            .table_indexes(&table.name)
            .map_or(0, |index| index.vector_len());
        if indexed != live_vectors {
            report.vector_ok = false;
            report.details.push(format!(
                "cobertura del índice vectorial en '{}': {indexed} indexados vs {live_vectors} vivos",
                table.name
            ));
        }
    }
}

/// Carga las definiciones de todas las tablas del catálogo persistido.
///
/// Args:
///     database: Base de la que leer el catálogo.
///
/// Returns:
///     Las definiciones de tabla en orden de catálogo.
///
/// Errors:
///     [`RuscaError::CorruptManifest`] si el catálogo es ilegible.
fn load_tables(database: &mut Database) -> Result<Vec<TableDef>, RuscaError> {
    let catalog = Catalog::load(database)?;
    let mut tables = Vec::new();
    for name in catalog.table_names() {
        tables.push(catalog.get(&name)?.clone());
    }
    Ok(tables)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    use ruscadb_core::{
        EdgeSet, Embedding, EmbeddingMeta, Metric, Record, RecordMeta, ScalarMap, ScalarValue,
    };

    use crate::catalog::{ColumnDef, ColumnType};

    /// Capacidad del buffer pool en los tests unitarios.
    const POOL_CAPACITY: usize = 128;
    /// Dimensión de los vectores del test de divergencia.
    const VECTOR_DIM: usize = 3;
    /// Consulta KNN usada para provocar (o no) divergencia (`k = 2`).
    const KNN_SQL: &str = "SELECT * FROM items KNN embedding <|2|> [0.1, 0.2, 0.3]";

    /// Abre una base temporal con una tabla `items(id INT, name TEXT)`.
    ///
    /// Args:
    ///     tag: Sufijo del nombre de archivo (aísla el test).
    ///
    /// Returns:
    ///     El directorio temporal (keep-alive) y la base lista.
    fn open_items(tag: &str) -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().expect("directorio temporal");
        let path = dir.path().join(format!("{tag}.db"));
        let mut database =
            Database::open(crate::database::DbConfig::new(&path, POOL_CAPACITY)).expect("apertura");
        database
            .create_table(
                "items",
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
        (dir, database)
    }

    /// Construye un registro vectorial L2 `(number, name, [f32; DIM])`.
    ///
    /// Args:
    ///     id: Identificador del registro.
    ///     number: Valor de la columna `id`.
    ///     name: Valor de la columna `name`.
    ///     vector: Componentes del embedding.
    ///
    /// Returns:
    ///     El registro listo para `insert_record`.
    fn vector_record(id: RecordId, number: i64, name: &str, vector: [f32; VECTOR_DIM]) -> Record {
        let embedding = Embedding::new(
            vector.to_vec(),
            EmbeddingMeta {
                model_id: "test".to_string(),
                dim: VECTOR_DIM,
                metric: Metric::L2,
            },
        )
        .expect("embedding válido");
        let scalars = ScalarMap::from([
            ("id".to_string(), ScalarValue::Int(number)),
            ("name".to_string(), ScalarValue::Text(name.to_string())),
        ]);
        Record {
            id,
            scalars,
            doc: None,
            edges: EdgeSet::default(),
            vector: Some(embedding),
            blob: None,
            meta: RecordMeta::default(),
        }
    }

    /// AC-0062-04 — un índice desincronizado se registra como divergencia.
    ///
    /// Given: `items` con 4 filas vectoriales y shadow reads habilitado.
    /// When: se retira del índice el vector del vecino más cercano y se repite
    ///       la query (una vez con shadow ON y otra OFF).
    /// Then: la divergencia queda registrada (esperado vs obtenido) y el
    ///       resultado servido es idéntico con shadow ON y OFF.
    #[test]
    // @spec AC-0062-04
    fn test_ac_0062_04_shadow_records_divergence() {
        let (_dir, mut database) = open_items("ac0062_04_unit");
        let data = [
            ("origen", [0.0_f32, 0.0, 0.0]),
            ("cerca", [0.1, 0.1, 0.1]),
            ("lejos", [9.0, 9.0, 9.0]),
            ("medio", [3.0, 3.0, 3.0]),
        ];
        let mut nearest: Option<Record> = None;
        for (number, (name, vector)) in data.into_iter().enumerate() {
            let record = vector_record(RecordId::new(), number as i64, name, vector);
            if name == "cerca" {
                nearest = Some(record.clone());
            }
            database.insert_record("items", record).expect("insert");
        }
        let target = nearest.expect("registro del vecino más cercano");

        database.set_shadow_reads(true);
        let _consistent = database.execute(KNN_SQL).expect("KNN consistente");
        assert!(
            database.shadow_report().divergences.is_empty(),
            "un índice consistente no debe divergir"
        );

        database
            .indexes
            .get_mut("items")
            .expect("índice presente")
            .remove_record(&target);

        let served_on = database.execute(KNN_SQL).expect("KNN con shadow ON");
        database.set_shadow_reads(false);
        let served_off = database.execute(KNN_SQL).expect("KNN con shadow OFF");
        database.set_shadow_reads(true);

        assert_eq!(
            served_on, served_off,
            "shadow reads no debe alterar el resultado servido"
        );
        let report = database.shadow_report();
        assert_eq!(report.checks, 2, "deben registrarse dos verificaciones");
        assert_eq!(
            report.divergences.len(),
            1,
            "el índice desincronizado debe registrar una divergencia: {report:?}"
        );
        let divergence = &report.divergences[0];
        assert!(
            divergence.expected.contains(&target.id),
            "el oráculo exacto debe incluir el id desincronizado: {divergence:?}"
        );
        assert_ne!(
            divergence.got, divergence.expected,
            "una divergencia debe tener ids distintos a los esperados"
        );
    }
}
