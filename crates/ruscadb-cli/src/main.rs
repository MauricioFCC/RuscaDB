//! CLI de RuscaDB: ejecutable unico multiplataforma.
//!
//! Comandos:
//!
//! - `ruscadb version` — version y capacidades del binario.
//! - `ruscadb query --db <ruta> "<RQL>"` — ejecuta una sentencia RQL.
//! - `ruscadb script --db <ruta> <archivo.rql>` — ejecuta un script RQL.
//! - `ruscadb mcp --db <ruta> [--write]` — servidor MCP JSON-RPC por stdio.
//!
//! El binario no abre red: opera sobre el directorio de la base (WAL + manifiesto
//! locales). Sin dependencias externas: solo `std` + las crates del workspace,
//! lo que mantiene el cross-compile a Windows/Linux/macOS (x64 y arm64) simple.

#![forbid(unsafe_code)]

use std::fmt;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use ruscadb::{ColumnDef, ColumnType, Database, DbConfig, Row, ScalarValue};
use ruscadb_mcp::McpServer;

/// Tamano del pool de paginas por defecto (suficiente para CLI).
const DEFAULT_POOL: usize = 64;

/// Error de la CLI (uso incorrecto o fallo de runtime).
#[derive(Debug)]
enum CliError {
    /// Uso incorrecto: falta un argumento o la combinacion no existe.
    Usage(String),
    /// Fallo de la biblioteca (base de datos, RQL, E/S).
    Runtime(String),
}

impl fmt::Display for CliError {
    /// Formatea el error con prefijo accionable.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "uso: {message}"),
            Self::Runtime(message) => write!(formatter, "{message}"),
        }
    }
}

/// Opciones parseadas de un subcomando.
#[derive(Debug)]
struct Options {
    /// Ruta del directorio de la base de datos.
    db: PathBuf,
    /// Salida en JSON en lugar de tabla.
    json: bool,
    /// Escribe habilitada (solo MCP).
    write: bool,
    /// Argumentos posicionales.
    positional: Vec<String>,
}

/// Punto de entrada: despacha el subcomando y traduce el error a codigo de salida.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ruscadb: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Despacha el subcomando solicitado.
///
/// Args:
///     args: Argumentos de la linea de comandos (sin el nombre del binario).
///
/// Returns:
///     `Ok(())` si el comando termina correctamente.
///
/// Errors:
///     [`CliError::Usage`] si el subcomando no existe o faltan argumentos;
///     [`CliError::Runtime`] si la base de datos o RQL fallan.
fn run(args: &[String]) -> Result<(), CliError> {
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    match command {
        "version" | "-V" | "--version" => {
            print_version();
            Ok(())
        }
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "query" => run_query(&args[1..]),
        "script" => run_script(&args[1..]),
        "table" => run_table(&args[1..]),
        "mcp" => run_mcp(&args[1..]),
        other => Err(CliError::Usage(format!(
            "subcomando desconocido '{other}'. Usa 'ruscadb help' para ver la lista"
        ))),
    }
}

/// Ejecuta una sentencia RQL suelta.
///
/// Args:
///     args: Argumentos del subcomando `query`.
///
/// Returns:
///     `Ok(())` tras imprimir el resultado.
///
/// Errors:
///     [`CliError::Usage`] si falta `--db` o la sentencia;
///     [`CliError::Runtime`] si la sentencia falla.
fn run_query(args: &[String]) -> Result<(), CliError> {
    let options = parse_options(args, true)?;
    let statement = options
        .positional
        .first()
        .ok_or_else(|| CliError::Usage("falta la sentencia RQL".to_string()))?;
    let mut database = open_database(&options.db)?;
    let rows = database
        .execute(statement)
        .map_err(|error| CliError::Runtime(format!("fallo la sentencia: {error}")))?;
    print_rows(&rows, options.json);
    Ok(())
}

/// Ejecuta un script RQL (sentencias separadas por `;`).
///
/// Args:
///     args: Argumentos del subcomando `script`.
///
/// Returns:
///     `Ok(())` tras ejecutar todas las sentencias.
///
/// Errors:
///     [`CliError::Usage`] si falta `--db` o la ruta del script;
///     [`CliError::Runtime`] si el script no se puede leer o una sentencia falla.
fn run_script(args: &[String]) -> Result<(), CliError> {
    let options = parse_options(args, true)?;
    let path = options
        .positional
        .first()
        .ok_or_else(|| CliError::Usage("falta la ruta del script .rql".to_string()))?;
    let source = fs::read_to_string(path)
        .map_err(|error| CliError::Runtime(format!("no se pudo leer '{path}': {error}")))?;
    let mut database = open_database(&options.db)?;
    for statement in split_statements(&source) {
        let rows = database
            .execute(&statement)
            .map_err(|error| CliError::Runtime(format!("fallo la sentencia: {error}")))?;
        print_rows(&rows, options.json);
    }
    Ok(())
}

/// Ejecuta el subcomando `table` (DDL: `create` y `list`).
///
/// Args:
///     args: Argumentos del subcomando `table`.
///
/// Returns:
///     `Ok(())` tras crear o listar las tablas.
///
/// Errors:
///     [`CliError::Usage`] si la accion no existe o faltan argumentos;
///     [`CliError::Runtime`] si el catalogo falla.
fn run_table(args: &[String]) -> Result<(), CliError> {
    let action = args
        .first()
        .ok_or_else(|| CliError::Usage("falta la accion (create|list)".to_string()))?;
    let options = parse_options(&args[1..], false)?;
    match action.as_str() {
        "create" => create_table(&options),
        "list" => list_tables(&options),
        other => Err(CliError::Usage(format!(
            "accion de tabla desconocida '{other}' (create|list)"
        ))),
    }
}

/// Crea una tabla con columnas `nombre:TIPO`.
///
/// Args:
///     options: Opciones parseadas (ruta de la base y posicionales).
///
/// Returns:
///     `Ok(())` tras crear la tabla.
///
/// Errors:
///     [`CliError::Usage`] si faltan el nombre o las columnas;
///     [`CliError::Runtime`] si la tabla no se puede crear.
fn create_table(options: &Options) -> Result<(), CliError> {
    let name = options
        .positional
        .first()
        .ok_or_else(|| CliError::Usage("falta el nombre de la tabla".to_string()))?;
    if options.positional.len() < 2 {
        return Err(CliError::Usage(
            "faltan las columnas (formato: nombre:TIPO, TIPO en Int|Float|Text|Bool)".to_string(),
        ));
    }
    let columns = options.positional[1..]
        .iter()
        .map(|spec| parse_column(spec))
        .collect::<Result<Vec<_>, _>>()?;
    let mut database = open_database(&options.db)?;
    database
        .create_table(name, columns)
        .map_err(|error| CliError::Runtime(format!("no se pudo crear la tabla: {error}")))?;
    println!("tabla '{name}' creada");
    Ok(())
}

/// Lista las tablas del catalogo con sus columnas.
///
/// Args:
///     options: Opciones parseadas (ruta de la base).
///
/// Returns:
///     `Ok(())` tras imprimir el catalogo.
///
/// Errors:
///     [`CliError::Runtime`] si el catalogo no se puede leer.
fn list_tables(options: &Options) -> Result<(), CliError> {
    let mut database = open_database(&options.db)?;
    let catalog = database
        .catalog()
        .map_err(|error| CliError::Runtime(format!("no se pudo leer el catalogo: {error}")))?;
    for table in catalog.table_names() {
        let definition = catalog
            .get(&table)
            .map_err(|error| CliError::Runtime(format!("no se pudo leer '{table}': {error}")))?;
        let columns: Vec<String> = definition
            .columns
            .iter()
            .map(|column| format!("{}:{}", column.name, column_type_name(column.col_type)))
            .collect();
        println!("{table} ({})", columns.join(", "));
    }
    Ok(())
}

/// Parsea una definicion de columna `nombre:TIPO`.
///
/// Args:
///     spec: Especificacion de la columna.
///
/// Returns:
///     La definicion de columna.
///
/// Errors:
///     [`CliError::Usage`] si el formato o el tipo no son validos.
fn parse_column(spec: &str) -> Result<ColumnDef, CliError> {
    let (name, kind) = spec.split_once(':').ok_or_else(|| {
        CliError::Usage(format!("columna invalida '{spec}' (esperado nombre:TIPO)"))
    })?;
    let col_type = match kind.to_ascii_lowercase().as_str() {
        "int" | "integer" | "bigint" => ColumnType::Int,
        "float" | "double" | "real" => ColumnType::Float,
        "text" | "string" | "varchar" => ColumnType::Text,
        "bool" | "boolean" => ColumnType::Bool,
        other => {
            return Err(CliError::Usage(format!(
                "tipo '{other}' desconocido (Int|Float|Text|Bool)"
            )));
        }
    };
    Ok(ColumnDef {
        name: name.to_string(),
        col_type,
    })
}

/// Devuelve el nombre textual de un tipo de columna.
///
/// Args:
///     col_type: Tipo declarado.
///
/// Returns:
///     El nombre canonico del tipo.
fn column_type_name(col_type: ColumnType) -> &'static str {
    match col_type {
        ColumnType::Bool => "Bool",
        ColumnType::Int => "Int",
        ColumnType::Float => "Float",
        ColumnType::Text => "Text",
    }
}

/// Sirve el protocolo MCP (JSON-RPC 2.0) por stdio.
///
/// Args:
///     args: Argumentos del subcomando `mcp`.
///
/// Returns:
///     `Ok(())` al terminar el bucle de stdio.
///
/// Errors:
///     [`CliError::Usage`] si falta `--db`;
///     [`CliError::Runtime`] si la base de datos o el servidor MCP fallan.
fn run_mcp(args: &[String]) -> Result<(), CliError> {
    let options = parse_options(args, true)?;
    let database = open_database(&options.db)?;
    let mut server = if options.write {
        McpServer::read_write(database)
    } else {
        McpServer::new(database)
    }
    .map_err(|error| CliError::Runtime(format!("no se pudo iniciar el servidor MCP: {error}")))?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| CliError::Runtime(format!("stdin: {error}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = server.handle_message(&line);
        writeln!(stdout, "{response}")
            .map_err(|error| CliError::Runtime(format!("stdout: {error}")))?;
        stdout
            .flush()
            .map_err(|error| CliError::Runtime(format!("stdout: {error}")))?;
    }
    Ok(())
}

/// Abre la base de datos en la ruta indicada.
///
/// Args:
///     path: Ruta del directorio de la base.
///
/// Returns:
///     La base abierta.
///
/// Errors:
///     [`CliError::Runtime`] si la base no se puede abrir.
fn open_database(path: &PathBuf) -> Result<Database, CliError> {
    Database::open(DbConfig::new(path, DEFAULT_POOL))
        .map_err(|error| CliError::Runtime(format!("no se pudo abrir la base: {error}")))
}

/// Parsea las opciones comunes (`--db`, `--json`, `--write`).
///
/// Args:
///     args: Argumentos del subcomando.
///     allow_json: Si admite `--json`.
///
/// Returns:
///     Las opciones parseadas.
///
/// Errors:
///     [`CliError::Usage`] si `--db` falta o el valor de una opcion es invalido.
fn parse_options(args: &[String], allow_json: bool) -> Result<Options, CliError> {
    let mut db: Option<PathBuf> = None;
    let mut json = false;
    let mut write = false;
    let mut positional: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--db" | "-d" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| CliError::Usage("'--db' requiere una ruta".to_string()))?;
                db = Some(PathBuf::from(value));
                index += 2;
            }
            "--json" => {
                if !allow_json {
                    return Err(CliError::Usage(
                        "'--json' no aplica a este comando".to_string(),
                    ));
                }
                json = true;
                index += 1;
            }
            "--write" => {
                write = true;
                index += 1;
            }
            other if other.starts_with("--db=") => {
                db = Some(PathBuf::from(&other[5..]));
                index += 1;
            }
            other if other.starts_with('-') => {
                return Err(CliError::Usage(format!("opcion desconocida '{other}'")));
            }
            other => {
                positional.push(other.to_string());
                index += 1;
            }
        }
    }
    let db = db.ok_or_else(|| CliError::Usage("falta '--db <ruta>'".to_string()))?;
    Ok(Options {
        db,
        json,
        write,
        positional,
    })
}

/// Divide un script RQL en sentencias (separador `;`, ignorando vacias).
///
/// Args:
///     source: Texto completo del script.
///
/// Returns:
///     Las sentencias en orden de aparicion.
fn split_statements(source: &str) -> Vec<String> {
    source
        .split(';')
        .map(|statement| statement.trim().to_string())
        .filter(|statement| !statement.is_empty())
        .collect()
}

/// Imprime las filas resultantes (tabla alineada o JSON).
///
/// Args:
///     rows: Filas del resultado.
///     json: Si emite JSON en lugar de tabla.
fn print_rows(rows: &[Row], json: bool) {
    if json {
        println!("{}", rows_to_json(rows));
        return;
    }
    print_table(rows);
}

/// Imprime las filas como tabla alineada.
///
/// Args:
///     rows: Filas del resultado.
fn print_table(rows: &[Row]) {
    if rows.is_empty() {
        println!("(0 filas)");
        return;
    }
    let headers: Vec<String> = rows
        .iter()
        .flat_map(|row| row.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            headers
                .iter()
                .map(|header| scalar_to_text(row.get(header)))
                .collect()
        })
        .collect();
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in &cells {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.len());
        }
    }
    print_line(&headers, &widths);
    for row in &cells {
        print_line(row, &widths);
    }
    println!("({} filas)", rows.len());
}

/// Imprime una linea de la tabla con padding.
///
/// Args:
///     cells: Celdas de la linea.
///     widths: Anchos por columna.
fn print_line(cells: &[String], widths: &[usize]) {
    let mut line = String::new();
    for (index, cell) in cells.iter().enumerate() {
        let width = widths.get(index).copied().unwrap_or(cell.len());
        line.push_str(cell);
        if index + 1 < cells.len() {
            line.push_str(&" ".repeat(width - cell.len() + 2));
        }
    }
    println!("{line}");
}

/// Convierte un escalar a texto plano para la tabla.
///
/// Args:
///     value: Valor de la celda (ausente = `NULL`).
///
/// Returns:
///     La representacion textual del valor.
fn scalar_to_text(value: Option<&ScalarValue>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(ScalarValue::Null) => "NULL".to_string(),
        Some(ScalarValue::Bool(flag)) => flag.to_string(),
        Some(ScalarValue::Int(number)) => number.to_string(),
        Some(ScalarValue::UInt(number)) => number.to_string(),
        Some(ScalarValue::Float(number)) => number.to_string(),
        Some(ScalarValue::Text(text)) => text.clone(),
        Some(ScalarValue::Bytes(bytes)) => format!(
            "0x{}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
        Some(ScalarValue::TimestampMillis(millis)) => millis.to_string(),
    }
}

/// Convierte las filas a un array JSON (sin dependencias externas).
///
/// Args:
///     rows: Filas del resultado.
///
/// Returns:
///     El JSON serializado (objetos con las claves de cada fila).
fn rows_to_json(rows: &[Row]) -> String {
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('{');
        for (position, (key, value)) in row.iter().enumerate() {
            if position > 0 {
                out.push(',');
            }
            out.push_str(&json_string(key));
            out.push(':');
            out.push_str(&json_scalar(value));
        }
        out.push('}');
    }
    out.push(']');
    out
}

/// Escapa una cadena como literal JSON.
///
/// Args:
///     text: Texto a escapar.
///
/// Returns:
///     El literal JSON entrecomillado.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Serializa un escalar como valor JSON.
///
/// Args:
///     value: Valor a serializar.
///
/// Returns:
///     El literal JSON del valor.
fn json_scalar(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Null => "null".to_string(),
        ScalarValue::Bool(flag) => flag.to_string(),
        ScalarValue::Int(number) => number.to_string(),
        ScalarValue::UInt(number) => number.to_string(),
        ScalarValue::Float(number) => number.to_string(),
        ScalarValue::Text(text) => json_string(text),
        ScalarValue::Bytes(bytes) => json_string(&format!(
            "0x{}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )),
        ScalarValue::TimestampMillis(millis) => millis.to_string(),
    }
}

/// Imprime la version y las capacidades del binario.
fn print_version() {
    println!("ruscadb {}", env!("CARGO_PKG_VERSION"));
    println!("edition: {}", env!("CARGO_PKG_RUST_VERSION"));
    println!("repositorio: {}", env!("CARGO_PKG_REPOSITORY"));
    println!("canales: query, script, mcp");
}

/// Imprime la ayuda de uso.
fn print_help() {
    println!(
        "ruscadb {} — base de datos embebida, multi-modelo y multimodal",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USO:");
    println!("  ruscadb query --db <ruta> [--json] \"<RQL>\"");
    println!("  ruscadb script --db <ruta> [--json] <archivo.rql>");
    println!("  ruscadb table create --db <ruta> <tabla> <col:TIPO>...");
    println!("  ruscadb table list --db <ruta>");
    println!("  ruscadb mcp --db <ruta> [--write]");
    println!("  ruscadb version");
    println!("  ruscadb help");
    println!();
    println!("EJEMPLOS:");
    println!("  ruscadb table create --db ./app.db docs title:Text score:Float");
    println!("  ruscadb query --db ./app.db \"SELECT * FROM docs\"");
    println!("  ruscadb mcp --db ./app.db   # servidor MCP por stdio");
}

/// Pruebas unitarias del CLI (parsers y formateadores).
#[cfg(test)]
mod tests {
    use super::*;

    /// El splitter de sentencias ignora vacios y recorta espacios.
    #[test]
    fn test_split_statements_ignora_vacios() {
        let statements = split_statements("SELECT 1; ;  SELECT 2 ;");
        assert_eq!(
            statements,
            vec!["SELECT 1".to_string(), "SELECT 2".to_string()]
        );
    }

    /// Las columnas se parsean con sus alias de tipo aceptados.
    #[test]
    fn test_parse_column_acepta_alias() {
        let column = parse_column("score:FLOAT").expect("columna valida");
        assert_eq!(column.name, "score");
        assert_eq!(column.col_type, ColumnType::Float);
    }

    /// Un tipo desconocido produce error de uso accionable.
    #[test]
    fn test_parse_column_rechaza_tipo() {
        let error = parse_column("score:decimal").expect_err("tipo invalido");
        assert!(error.to_string().contains("decimal"), "mensaje: {error}");
    }

    /// El escapado JSON neutraliza comillas y saltos de linea.
    #[test]
    fn test_json_string_escapa() {
        assert_eq!(json_string("a\"b\nc"), "\"a\\\"b\\nc\"");
    }

    /// Los escalares nulos se imprimen como NULL en la tabla.
    #[test]
    fn test_scalar_to_text_null() {
        assert_eq!(scalar_to_text(None), "NULL");
        assert_eq!(scalar_to_text(Some(&ScalarValue::Null)), "NULL");
    }
}
