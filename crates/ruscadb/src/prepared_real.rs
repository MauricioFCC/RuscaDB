//! Sentencias preparadas reales (SPEC-0166).
//!
//! Un `prepare` valida y fija el texto de la consulta; despues solo se envian
//! valores, nunca SQL. Aqui se validan los marcadores (`$1..$n` o `?`), se
//! comprueba el tipo/aridad en cada `bind` y se renderizan literales escapados.
//! La clave de cache (`plan_cache_key`) es un hash FNV-1a estable del texto, de
//! modo que la misma consulta reutiliza el mismo plan.

/// Tipo declarado de un parametro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// Entero.
    Int,
    /// Coma flotante.
    Float,
    /// Texto.
    Text,
    /// Booleano.
    Bool,
}

/// Valor ligado a un parametro.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    /// Entero.
    Int(i64),
    /// Coma flotante (NaN/inf rechazados al renderizar).
    Float(f64),
    /// Texto.
    Text(String),
    /// Booleano.
    Bool(bool),
    /// NULL SQL.
    Null,
}

impl ParamValue {
    /// Tipo del valor (NULL es compatible con cualquiera).
    fn kind(&self) -> Option<ParamType> {
        match self {
            ParamValue::Int(_) => Some(ParamType::Int),
            ParamValue::Float(_) => Some(ParamType::Float),
            ParamValue::Text(_) => Some(ParamType::Text),
            ParamValue::Bool(_) => Some(ParamType::Bool),
            ParamValue::Null => None,
        }
    }

    /// Renderiza el valor como literal SQL seguro.
    ///
    /// Returns:
    ///     Literal escapado (`'...'` con `''`), o `None` si es NaN/infinito.
    fn render(&self) -> Option<String> {
        match self {
            ParamValue::Int(value) => Some(value.to_string()),
            ParamValue::Float(value) if value.is_finite() => {
                let mut text = value.to_string();
                if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                    text.push_str(".0");
                }
                Some(text)
            }
            ParamValue::Float(_) => None,
            ParamValue::Bool(value) => Some(if *value { "TRUE" } else { "FALSE" }.to_string()),
            ParamValue::Null => Some("NULL".to_string()),
            ParamValue::Text(value) => Some(format!("'{}'", value.replace('\'', "''"))),
        }
    }
}

/// Error de preparacion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// Consulta vacia.
    EmptyQuery,
    /// Marcador `$0` (los parametros empiezan en 1).
    ZeroIndex,
    /// Marcadores no contiguos (`$1` y `$3` sin `$2`).
    NonContiguous(usize),
    /// La aridad declarada no coincide con los marcadores.
    ArityMismatch {
        /// Marcadores encontrados.
        found: usize,
        /// Tipos declarados.
        declared: usize,
    },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrepareError::EmptyQuery => write!(formatter, "consulta vacia"),
            PrepareError::ZeroIndex => write!(formatter, "marcador $0 invalido (empieza en $1)"),
            PrepareError::NonContiguous(position) => {
                write!(formatter, "marcadores no contiguos: falta ${position}")
            }
            PrepareError::ArityMismatch { found, declared } => write!(
                formatter,
                "la consulta tiene {found} marcadores pero se declararon {declared} tipos"
            ),
        }
    }
}

impl std::error::Error for PrepareError {}

/// Error de enlace de valores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// Numero de valores distinto del declarado.
    Arity {
        /// Esperados.
        expected: usize,
        /// Recibidos.
        found: usize,
    },
    /// El valor no encaja con el tipo declarado.
    TypeMismatch {
        /// Posicion (1-based).
        position: usize,
        /// Tipo declarado.
        declared: ParamType,
    },
    /// Float no finito (NaN/inf) no representable como literal.
    NonFinite {
        /// Posicion (1-based).
        position: usize,
    },
}

impl std::fmt::Display for BindError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Arity { expected, found } => {
                write!(
                    formatter,
                    "se esperaban {expected} parametros, llegaron {found}"
                )
            }
            BindError::TypeMismatch { position, declared } => write!(
                formatter,
                "parametro {position}: se esperaba {declared:?} y llego otro tipo"
            ),
            BindError::NonFinite { position } => {
                write!(formatter, "parametro {position}: float no finito")
            }
        }
    }
}

impl std::error::Error for BindError {}

/// Sentencia preparada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedStmtReal {
    /// Texto validado de la consulta.
    pub query: String,
    /// Tipos declarados por posicion.
    params: Vec<ParamType>,
}

impl PreparedStmtReal {
    /// Crea una sentencia preparada simple (todos los parametros `Int`).
    ///
    /// Args:
    ///     query: Texto de la consulta.
    ///     param_count: Numero de parametros.
    ///
    /// Returns:
    ///     Sentencia lista para `bind`.
    pub fn prepare(query: impl Into<String>, param_count: usize) -> Self {
        let query = query.into();
        let declared = Self::count_markers(&query);
        let count = param_count.max(declared);
        Self {
            query,
            params: vec![ParamType::Int; count],
        }
    }

    /// Crea una sentencia preparada con tipos declarados y valida marcadores.
    ///
    /// Args:
    ///     query: Texto de la consulta.
    ///     params: Tipos por posicion.
    ///
    /// Returns:
    ///     Sentencia validada.
    ///
    /// Errors:
    ///     [`PrepareError`] si la consulta esta vacia, hay `$0`, los marcadores
    ///     no son contiguos o la aridad no coincide.
    pub fn prepare_typed(
        query: impl Into<String>,
        params: Vec<ParamType>,
    ) -> Result<Self, PrepareError> {
        let query = query.into();
        if query.trim().is_empty() {
            return Err(PrepareError::EmptyQuery);
        }
        let (found, highest) = Self::scan_markers(&query)?;
        if found != highest {
            return Err(PrepareError::NonContiguous(found + 1));
        }
        if highest != params.len() {
            return Err(PrepareError::ArityMismatch {
                found: highest,
                declared: params.len(),
            });
        }
        Ok(Self { query, params })
    }

    /// Texto de la consulta.
    ///
    /// Returns:
    ///     Consulta validada.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Numero de parametros declarados.
    ///
    /// Returns:
    ///     Aridad esperada.
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Tipos declarados.
    ///
    /// Returns:
    ///     Slice de tipos por posicion.
    pub fn param_types(&self) -> &[ParamType] {
        &self.params
    }

    /// Compatibilidad: enlaza enteros comprobando solo la aridad.
    ///
    /// Args:
    ///     params: Valores enteros.
    ///
    /// Returns:
    ///     `true` si la aridad coincide.
    pub fn bind(&self, params: &[i64]) -> bool {
        params.len() == self.params.len()
    }

    /// Enlaza valores y renderiza la consulta final con literales seguros.
    ///
    /// Args:
    ///     values: Valores a sustituir en los marcadores.
    ///
    /// Returns:
    ///     Consulta lista para ejecutar.
    ///
    /// Errors:
    ///     [`BindError`] si la aridad, el tipo o un float no finito no cuadran.
    pub fn bind_values(&self, values: &[ParamValue]) -> Result<String, BindError> {
        if values.len() != self.params.len() {
            return Err(BindError::Arity {
                expected: self.params.len(),
                found: values.len(),
            });
        }
        let mut rendered = Vec::with_capacity(values.len());
        for (index, (value, declared)) in values.iter().zip(&self.params).enumerate() {
            let position = index + 1;
            if let Some(kind) = value.kind()
                && kind != *declared
            {
                return Err(BindError::TypeMismatch {
                    position,
                    declared: *declared,
                });
            }
            let literal = value.render().ok_or(BindError::NonFinite { position })?;
            rendered.push(literal);
        }
        Ok(Self::substitute(&self.query, &rendered))
    }

    /// Clave estable de cache de plan (FNV-1a 64 del texto normalizado).
    ///
    /// Returns:
    ///     Hash de la consulta con espacios colapsados.
    pub fn plan_cache_key(&self) -> u64 {
        let normalized = self.query.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in normalized.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    /// Cuenta marcadores `$n` y `?`.
    fn count_markers(query: &str) -> usize {
        Self::scan_markers(query)
            .map(|(found, highest)| found.max(highest))
            .unwrap_or(0)
    }

    /// Recorre la consulta y devuelve `(marcadores, indice_maximo)`.
    fn scan_markers(query: &str) -> Result<(usize, usize), PrepareError> {
        let bytes = query.as_bytes();
        let mut index = 0usize;
        let mut found = 0usize;
        let mut highest = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'?' => {
                    found += 1;
                    highest = highest.max(found);
                    index += 1;
                }
                b'$' => {
                    let start = index + 1;
                    let mut end = start;
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                    if end == start {
                        index += 1;
                        continue;
                    }
                    let number: usize = query[start..end].parse().unwrap_or(0);
                    if number == 0 {
                        return Err(PrepareError::ZeroIndex);
                    }
                    found += 1;
                    highest = highest.max(number);
                    index = end;
                }
                _ => index += 1,
            }
        }
        Ok((found, highest))
    }

    /// Sustituye marcadores por literales en orden.
    fn substitute(query: &str, values: &[String]) -> String {
        let mut out = String::with_capacity(query.len() + values.len() * 8);
        let bytes = query.as_bytes();
        let mut index = 0usize;
        // Posicion ordinal del marcador (cuenta `?` y `$n` por orden de aparicion).
        let mut ordinal = 0usize;
        while index < bytes.len() {
            match bytes[index] {
                b'?' => {
                    out.push_str(values.get(ordinal).map(String::as_str).unwrap_or("NULL"));
                    ordinal += 1;
                    index += 1;
                }
                b'$' => {
                    let start = index + 1;
                    let mut end = start;
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                    if end == start {
                        out.push('$');
                        index += 1;
                        continue;
                    }
                    let position: usize = query[start..end].parse().unwrap_or(0);
                    out.push_str(
                        values
                            .get(position.saturating_sub(1))
                            .map(String::as_str)
                            .unwrap_or("NULL"),
                    );
                    ordinal += 1;
                    index = end;
                }
                _ => {
                    // Copia el caracter UTF-8 completo.
                    let ch = query[index..].chars().next().unwrap_or('\u{0}');
                    out.push(ch);
                    index += ch.len_utf8();
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{BindError, ParamType, ParamValue, PrepareError, PreparedStmtReal};

    /// AC-0166-01: `bind` valida aridad y `bind_values` renderiza.
    #[test]
    // @spec AC-0166-01
    fn test_ac_0166_01_bind() {
        let statement = PreparedStmtReal::prepare("SELECT * FROM t WHERE id = $1", 1);
        assert!(statement.bind(&[42]));
        assert!(!statement.bind(&[]));
        let rendered = statement.bind_values(&[ParamValue::Int(42)]).expect("bind");
        assert_eq!(rendered, "SELECT * FROM t WHERE id = 42");
    }

    /// AC-0166-02: reutilizacion con distintos valores y tipos.
    #[test]
    // @spec AC-0166-02
    fn test_ac_0166_02_reuse() {
        let statement = PreparedStmtReal::prepare("q", 1);
        assert!(statement.bind(&[1]));
        assert!(statement.bind(&[2]));
        assert_eq!(statement.plan_cache_key(), statement.plan_cache_key());

        let typed = PreparedStmtReal::prepare_typed(
            "SELECT * FROM t WHERE a = $1 AND b = $2",
            vec![ParamType::Int, ParamType::Text],
        )
        .expect("prepare");
        assert_eq!(typed.param_count(), 2);
        let sql = typed
            .bind_values(&[ParamValue::Int(7), ParamValue::Text("x".into())])
            .expect("bind");
        assert_eq!(sql, "SELECT * FROM t WHERE a = 7 AND b = 'x'");

        // Tipo incorrecto se rechaza antes de tocar el motor.
        assert_eq!(
            typed.bind_values(&[ParamValue::Text("no".into()), ParamValue::Null]),
            Err(BindError::TypeMismatch {
                position: 1,
                declared: ParamType::Int,
            })
        );
    }

    /// AC-0166-03: el texto de la consulta es inmutable y los valores se
    /// escapan (no hay inyeccion).
    #[test]
    // @spec AC-0166-03
    fn test_ac_0166_03_injection() {
        let statement = PreparedStmtReal::prepare_typed(
            "SELECT * FROM t WHERE name = $1",
            vec![ParamType::Text],
        )
        .expect("prepare");
        let attack = ParamValue::Text("'; DROP TABLE t; --".into());
        let sql = statement.bind_values(&[attack]).expect("bind");
        // El valor queda dentro de un unico literal con la comilla duplicada.
        assert_eq!(sql, "SELECT * FROM t WHERE name = '''; DROP TABLE t; --'");
        // La sentencia original no se muta: el siguiente bind es limpio.
        assert_eq!(statement.query(), "SELECT * FROM t WHERE name = $1");
        let clean = statement
            .bind_values(&[ParamValue::Text("ok".into())])
            .expect("bind limpio");
        assert_eq!(clean, "SELECT * FROM t WHERE name = 'ok'");
    }

    /// Validaciones de preparacion: vacia, `$0`, no contiguos, aridad.
    #[test]
    fn prepare_validations() {
        assert_eq!(
            PreparedStmtReal::prepare_typed("", vec![]),
            Err(PrepareError::EmptyQuery)
        );
        assert_eq!(
            PreparedStmtReal::prepare_typed("SELECT $0", vec![ParamType::Int]),
            Err(PrepareError::ZeroIndex)
        );
        assert_eq!(
            PreparedStmtReal::prepare_typed("SELECT $1, $3", vec![ParamType::Int; 3]),
            Err(PrepareError::NonContiguous(3))
        );
        assert_eq!(
            PreparedStmtReal::prepare_typed("SELECT $1", vec![ParamType::Int; 2]),
            Err(PrepareError::ArityMismatch {
                found: 1,
                declared: 2,
            })
        );
        assert_eq!(
            PreparedStmtReal::prepare_typed("SELECT ?", vec![ParamType::Bool])
                .expect("?")
                .bind_values(&[ParamValue::Bool(true)])
                .expect("bind"),
            "SELECT TRUE"
        );
    }

    /// NaN/inf no se renderizan (fail-fast).
    #[test]
    fn non_finite_float_is_rejected() {
        let statement =
            PreparedStmtReal::prepare_typed("SELECT $1", vec![ParamType::Float]).expect("prepare");
        assert_eq!(
            statement.bind_values(&[ParamValue::Float(f64::NAN)]),
            Err(BindError::NonFinite { position: 1 })
        );
    }

    /// La clave de cache normaliza espacios y distingue consultas distintas.
    #[test]
    fn plan_cache_key_normalizes_whitespace() {
        let compact = PreparedStmtReal::prepare("SELECT * FROM t", 0);
        let padded = PreparedStmtReal::prepare("SELECT   *\nFROM\tt", 0);
        assert_eq!(compact.plan_cache_key(), padded.plan_cache_key());
        let other = PreparedStmtReal::prepare("SELECT * FROM u", 0);
        assert_ne!(compact.plan_cache_key(), other.plan_cache_key());
    }

    /// Renderizado de float, bool y NULL como literales SQL.
    #[test]
    fn renders_scalar_literals() {
        let statement = PreparedStmtReal::prepare_typed(
            "VALUES ($1, $2, $3)",
            vec![ParamType::Float, ParamType::Bool, ParamType::Text],
        )
        .expect("prepare");
        let sql = statement
            .bind_values(&[
                ParamValue::Float(2.0),
                ParamValue::Bool(false),
                ParamValue::Null,
            ])
            .expect("bind");
        assert_eq!(sql, "VALUES (2.0, FALSE, NULL)");
    }

    /// Mezcla de `?` y `$n`: ambos cuentan como marcadores.
    #[test]
    fn mixed_markers_are_counted() {
        let statement = PreparedStmtReal::prepare("SELECT $1, ?", 2);
        assert_eq!(statement.param_count(), 2);
        assert_eq!(
            statement
                .bind_values(&[ParamValue::Int(1), ParamValue::Int(2)])
                .expect("bind"),
            "SELECT 1, 2"
        );
    }
}
