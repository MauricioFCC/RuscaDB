//! Expresiones de tabla comunes (CTE), incluidas recursivas (SPEC-0163).
//!
//! Una CTE recursiva se evalua como punto fijo: `semilla` -> aplicar `paso` a
//! la frontera hasta que no produzca filas nuevas. Se distingue `UNION ALL`
//! (multiplicidades conservadas) de `UNION` (deduplicado) y se detecta el
//! ciclo (una fila generada que ya estaba en el resultado) para no colgar en
//! datos ciclicos como un grafo con bucles.

use std::collections::BTreeSet;

/// Resultado de evaluar una CTE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CteOutcome {
    /// Filas producidas, en orden de generacion.
    pub rows: Vec<i64>,
    /// Iteraciones ejecutadas (sin contar la semilla).
    pub iterations: usize,
    /// `true` si se genero una fila ya vista (posible ciclo).
    pub hit_cycle: bool,
    /// `true` si se alcanzo `max_iterations`.
    pub truncated: bool,
}

/// Definicion de una CTE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RqlCte {
    /// Nombre de la CTE.
    pub name: String,
    /// `true` si es recursiva (`WITH RECURSIVE`).
    pub recursive: bool,
    /// Limite duro de iteraciones (defensa ante recursion infinita).
    pub max_iterations: usize,
}

impl RqlCte {
    /// Crea la CTE.
    ///
    /// Args:
    ///     name: Nombre visible en la consulta.
    ///     recursive: `true` para `WITH RECURSIVE`.
    ///
    /// Returns:
    ///     CTE con limite por defecto de 1000 iteraciones.
    pub fn new(name: impl Into<String>, recursive: bool) -> Self {
        Self {
            name: name.into(),
            recursive,
            max_iterations: 1000,
        }
    }

    /// Indica si la CTE es recursiva.
    ///
    /// Returns:
    ///     `true` para `WITH RECURSIVE`.
    pub fn is_recursive(&self) -> bool {
        self.recursive
    }

    /// Nombre de la CTE.
    ///
    /// Returns:
    ///     Nombre declarado.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Punto fijo simple (`UNION ALL`) compatible con la version anterior.
    ///
    /// Args:
    ///     seed: Filas iniciales.
    ///     step: Funcion que recibe la frontera y devuelve filas nuevas.
    ///     max_depth: Limite de iteraciones.
    ///
    /// Returns:
    ///     Filas acumuladas.
    pub fn fixed_point(
        &self,
        seed: Vec<i64>,
        step: impl Fn(&[i64]) -> Vec<i64>,
        max_depth: usize,
    ) -> Vec<i64> {
        let mut accumulator = seed;
        for _ in 0..max_depth {
            let frontier = step(&accumulator);
            if frontier.is_empty() {
                break;
            }
            accumulator.extend(frontier);
        }
        accumulator
    }

    /// Evalua la CTE con semantica `UNION ALL` y deteccion de ciclo.
    ///
    /// Args:
    ///     seed: Filas iniciales.
    ///     step: Funcion que recibe la frontera y devuelve filas nuevas.
    ///
    /// Returns:
    ///     Resultado con metricas y bandera de ciclo.
    pub fn eval(&self, seed: Vec<i64>, step: impl Fn(&[i64]) -> Vec<i64>) -> CteOutcome {
        self.run(seed, step, false)
    }

    /// Evalua la CTE con semantica `UNION` (deduplicado) y deteccion de ciclo.
    ///
    /// Args:
    ///     seed: Filas iniciales.
    ///     step: Funcion que recibe la frontera y devuelve filas nuevas.
    ///
    /// Returns:
    ///     Resultado sin filas duplicadas.
    pub fn eval_distinct(&self, seed: Vec<i64>, step: impl Fn(&[i64]) -> Vec<i64>) -> CteOutcome {
        self.run(seed, step, true)
    }

    /// Nucleo comun de ambos modos.
    fn run(&self, seed: Vec<i64>, step: impl Fn(&[i64]) -> Vec<i64>, distinct: bool) -> CteOutcome {
        if !self.recursive {
            return CteOutcome {
                rows: seed,
                iterations: 0,
                hit_cycle: false,
                truncated: false,
            };
        }
        let mut rows: Vec<i64> = Vec::new();
        let mut seen: BTreeSet<i64> = BTreeSet::new();
        for value in seed {
            if !distinct || seen.insert(value) {
                rows.push(value);
            }
        }
        let mut frontier = rows.clone();
        let mut iterations = 0;
        let mut hit_cycle = false;
        let mut truncated = false;

        while !frontier.is_empty() {
            if iterations >= self.max_iterations {
                truncated = true;
                break;
            }
            iterations += 1;
            let produced = step(&frontier);
            let mut next = Vec::new();
            for value in produced {
                if seen.contains(&value) {
                    hit_cycle = true;
                    if distinct {
                        continue;
                    }
                }
                seen.insert(value);
                next.push(value);
                rows.push(value);
            }
            frontier = next;
        }

        CteOutcome {
            rows,
            iterations,
            hit_cycle,
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RqlCte;

    /// AC-0163-01: nombre y bandera recursiva.
    #[test]
    // @spec AC-0163-01
    fn test_ac_0163_01_cte() {
        let cte = RqlCte::new("t", false);
        assert_eq!(cte.name(), "t");
        assert!(!cte.is_recursive());
        let recursive = RqlCte::new("g", true);
        assert!(recursive.is_recursive());
        // Una CTE no recursiva no itera: devuelve la semilla tal cual.
        let outcome = cte.eval(vec![1, 2], |_| vec![3]);
        assert_eq!(outcome.rows, vec![1, 2]);
        assert_eq!(outcome.iterations, 0);
    }

    /// AC-0163-02: recursion que termina, deduplicado y deteccion de ciclo.
    #[test]
    // @spec AC-0163-02
    fn test_ac_0163_02_recursive() {
        let cte = RqlCte::new("g", true);
        let outcome = cte.eval(vec![1], |rows| {
            let max = rows.iter().copied().max().unwrap_or(0);
            if max < 4 { vec![max + 1] } else { vec![] }
        });
        assert_eq!(outcome.rows, vec![1, 2, 3, 4]);
        assert_eq!(outcome.iterations, 4);
        assert!(!outcome.truncated);

        // UNION: un paso que reemite lo ya visto no duplica filas.
        let distinct = cte.eval_distinct(vec![1], |_| vec![1, 2]);
        assert_eq!(distinct.rows, vec![1, 2]);
        assert!(distinct.hit_cycle, "reemitir una fila vista es ciclo");

        // Limite de iteraciones: no cuelga con un paso que siempre produce.
        let mut bounded = RqlCte::new("inf", true);
        bounded.max_iterations = 5;
        let outcome = bounded.eval(vec![0], |rows| {
            vec![rows.iter().copied().max().unwrap_or(0) + 1]
        });
        assert!(outcome.truncated);
        assert_eq!(outcome.iterations, 5);
    }

    /// `fixed_point` mantiene la semantica historica.
    #[test]
    fn fixed_point_matches_legacy() {
        let cte = RqlCte::new("g", true);
        let rows = cte.fixed_point(
            vec![1],
            |rows| {
                if rows.len() < 4 {
                    vec![rows.len() as i64 + 1]
                } else {
                    vec![]
                }
            },
            10,
        );
        assert_eq!(rows, vec![1, 2, 3, 4]);
    }
}
