//! Test de humo de los ejemplos ejecutables (SPEC-0087).
//!
//! Cada ejemplo se ejecuta **como binario real** (el mismo que el usuario
//! lanza con `cargo run --example ...`) y se validan sus salidas: no basta
//! con que compilen, tienen que producir los resultados documentados.

use std::path::PathBuf;
use std::process::Command;

/// Localiza el binario de un ejemplo compilado para este profile.
///
/// Args:
///     name: Nombre del ejemplo (`quickstart`, `vector_search`, ...).
///
/// Returns:
///     Ruta `<target>/<profile>/examples/<name><exe>`.
fn example_exe(name: &str) -> PathBuf {
    let current = std::env::current_exe()
        .unwrap_or_else(|error| panic!("no se pudo resolver la ruta del test: {error}"));
    // `<target>/<profile>/deps/<test>.exe` -> `<target>/<profile>`.
    let profile = current
        .parent()
        .and_then(|deps| deps.parent())
        .unwrap_or_else(|| panic!("perfil de compilación inesperado en {}", current.display()));
    let exe = profile
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        exe.exists(),
        "ejemplo '{name}' no compilado en {} (ejecuta `cargo build --examples -p ruscadb` o `cargo test -p ruscadb` sin --test)",
        exe.display()
    );
    exe
}

/// Ejecuta un ejemplo y devuelve su salida estándar.
///
/// Args:
///     name: Nombre del ejemplo a ejecutar.
///
/// Returns:
///     El stdout completo (UTF-8) del proceso hijo.
fn run_example(name: &str) -> String {
    let exe = example_exe(name);
    let output = Command::new(&exe)
        .output()
        .unwrap_or_else(|error| panic!("no se pudo ejecutar {}: {error}", exe.display()));
    assert!(
        output.status.success(),
        "el ejemplo '{name}' terminó con {:?}. stderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("stdout de '{name}' no es UTF-8: {error}"))
}

/// AC-0087-01 — el quickstart ejecuta DML completo con resultados esperados.
#[test]
// @spec AC-0087-01
fn test_ac_0087_01_quickstart_runs_full_dml_flow() {
    let stdout = run_example("quickstart");
    assert!(
        stdout.contains(r#"mayores de edad: ["Ana", "Mar"]"#),
        "SELECT con WHERE debe devolver Ana y Mar:\n{stdout}"
    );
    assert!(
        stdout.contains(r#"tras UPDATE: ["Ana", "Luis (menor)", "Mar"]"#),
        "UPDATE ... WHERE debe reescribir una sola fila:\n{stdout}"
    );
    assert!(
        stdout.contains("tras DELETE quedan 2 registros"),
        "DELETE ... WHERE debe quitar una fila:\n{stdout}"
    );
}

/// AC-0087-02 — KNN devuelve el top-k ordenado y MATCH el BM25 esperado.
#[test]
// @spec AC-0087-02
fn test_ac_0087_02_vector_search_returns_ranked_results() {
    let stdout = run_example("vector_search");
    assert!(
        stdout.contains(r#"KNN top-2: ["gato casero", "perro grande"]"#),
        "KNN debe ordenar por distancia L2 desde [0, 0]:\n{stdout}"
    );
    assert!(
        stdout.contains(r#"MATCH 'gato': ["gato casero", "gato montes"]"#),
        "MATCH debe recuperar ambos títulos con 'gato':\n{stdout}"
    );
}

/// AC-0087-03 — TRAVERSE respeta DEPTH y excluye el nodo aislado.
#[test]
// @spec AC-0087-03
fn test_ac_0087_03_graph_traverse_respects_depth() {
    let stdout = run_example("graph_traverse");
    assert!(
        stdout.contains(r#"TRAVERSE DEPTH 2: ["a", "b", "c"]"#),
        "DEPTH 2 debe alcanzar a-b-c:\n{stdout}"
    );
    assert!(
        stdout.contains(r#"TRAVERSE DEPTH 1: ["a", "b"]"#),
        "DEPTH 1 se detiene antes de 'c' y 'd' queda fuera:\n{stdout}"
    );
}

/// AC-0087-04 — la guía de API documenta cada ejemplo con su comando.
#[test]
// @spec AC-0087-04
fn test_ac_0087_04_api_guide_documents_each_example() {
    let guide_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/api_guide.md");
    let guide = std::fs::read_to_string(guide_path)
        .unwrap_or_else(|err| panic!("docs/api_guide.md ilegible: {err}"));
    for example in ["quickstart", "vector_search", "graph_traverse"] {
        let command = format!("cargo run --example {example}");
        assert!(guide.contains(&command), "la guía no documenta `{command}`");
        let file = format!("examples/{example}.rs");
        assert!(guide.contains(&file), "la guía no referencia {file}");
    }
    for api in [
        "Database::open",
        "DbConfig",
        "create_table",
        "execute",
        "insert_record",
    ] {
        assert!(guide.contains(api), "la guía no explica {api}");
    }
}
