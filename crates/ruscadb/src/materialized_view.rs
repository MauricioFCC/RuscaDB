//! Vistas materializadas (SPEC-0149): resultado precomputado de una consulta.
//!
//! La vista guarda las filas del ultimo `refresh`; el `query` asociado permite
//! recalcularlas con el executor. No se refresca sola (el disparador es del
//! consumidor), lo que evita coste oculto.

/// Delta de un refresh (filas insertadas/borradas/sin cambios).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefreshDelta {
    /// Filas nuevas respecto al snapshot anterior.
    pub inserted: usize,
    /// Filas desaparecidas.
    pub deleted: usize,
    /// Filas identicas (mismo fingerprint).
    pub unchanged: usize,
}

impl RefreshDelta {
    /// Numero de filas cambiadas (`inserted + deleted`).
    ///
    /// Returns:
    ///     Filas que difieren.
    pub fn changed(&self) -> usize {
        self.inserted + self.deleted
    }

    /// Indica si el refresh no cambio nada.
    ///
    /// Returns:
    ///     `true` si no hay inserciones ni borrados.
    pub fn is_noop(&self) -> bool {
        self.inserted == 0 && self.deleted == 0
    }
}

/// Vista materializada.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MaterializedView {
    /// Nombre de la vista.
    pub name: String,
    /// Consulta que la alimenta.
    pub query: String,
    /// Filas precomputadas.
    pub rows: Vec<Vec<String>>,
    /// Fingerprints ordenados de las filas (delta incremental).
    #[serde(default)]
    pub fingerprints: Vec<u64>,
}

impl MaterializedView {
    /// Crea la vista (sin filas).
    ///
    /// Args:
    ///     name: Nombre.
    ///     query: Consulta RQL.
    ///
    /// Returns:
    ///     Vista vacia.
    pub fn new(name: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            query: query.into(),
            rows: Vec::new(),
            fingerprints: Vec::new(),
        }
    }

    /// Carga el resultado inicial.
    ///
    /// Args:
    ///     rows: Filas precomputadas.
    pub fn create(&mut self, rows: Vec<Vec<String>>) {
        self.fingerprints = Self::fingerprints_of(&rows);
        self.rows = rows;
    }

    /// Reemplaza el resultado (refresh).
    ///
    /// Args:
    ///     rows: Filas nuevas.
    pub fn refresh(&mut self, rows: Vec<Vec<String>>) {
        self.fingerprints = Self::fingerprints_of(&rows);
        self.rows = rows;
    }

    /// Filas precomputadas.
    ///
    /// Returns:
    ///     Slice de filas.
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// Nombre de la vista.
    ///
    /// Returns:
    ///     Nombre declarado.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Consulta asociada.
    ///
    /// Returns:
    ///     Texto RQL.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Fingerprints ordenados de un conjunto de filas.
    ///
    /// Args:
    ///     rows: Filas en texto.
    ///
    /// Returns:
    ///     Un hash FNV-1a de 64 bits por fila, ordenado.
    pub fn fingerprints_of(rows: &[Vec<String>]) -> Vec<u64> {
        let mut fingerprints: Vec<u64> = rows.iter().map(|row| fingerprint(row)).collect();
        fingerprints.sort_unstable();
        fingerprints
    }

    /// Reemplaza las filas y calcula el delta contra el snapshot anterior.
    ///
    /// El calculo es una **merge de dos listas ordenadas** (`O(n log n)` por
    /// los fingerprints), no una comparacion cuadratica.
    ///
    /// Args:
    ///     rows: Filas nuevas del refresh.
    ///
    /// Returns:
    ///     [`RefreshDelta`] con insertadas/borradas/sin cambios.
    pub fn diff(&mut self, rows: Vec<Vec<String>>) -> RefreshDelta {
        // Ambos vectores se mantienen ordenados por `fingerprints_of`.
        let old = std::mem::take(&mut self.fingerprints);
        let new = Self::fingerprints_of(&rows);
        let (mut left, mut right) = (0usize, 0usize);
        let mut delta = RefreshDelta::default();
        while left < old.len() && right < new.len() {
            match old[left].cmp(&new[right]) {
                std::cmp::Ordering::Equal => {
                    delta.unchanged += 1;
                    left += 1;
                    right += 1;
                }
                std::cmp::Ordering::Less => {
                    delta.deleted += 1;
                    left += 1;
                }
                std::cmp::Ordering::Greater => {
                    delta.inserted += 1;
                    right += 1;
                }
            }
        }
        delta.deleted += old.len() - left;
        delta.inserted += new.len() - right;
        self.fingerprints = new;
        self.rows = rows;
        delta
    }
}

/// FNV-1a de 64 bits sobre una fila (separadores para evitar ambiguedad).
fn fingerprint(row: &[String]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for field in row {
        for byte in field.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xFF;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::MaterializedView;

    #[test] // @spec AC-0149-01
    fn test_ac_0149_01_materialized_view_create() {
        let mut view = MaterializedView::new("v1", "SELECT * FROM t");
        assert!(view.rows().is_empty());
        view.create(vec![vec!["1".to_string(), "a".to_string()]]);
        assert_eq!(view.rows().len(), 1);
        assert_eq!(view.name(), "v1");
        assert_eq!(view.query(), "SELECT * FROM t");
        view.refresh(vec![vec!["2".to_string()]]);
        assert_eq!(view.rows()[0][0], "2");
    }
}
