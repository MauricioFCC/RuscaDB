//! Tests de aceptación de SPEC-0100 (cliente LLM agnóstico de proveedor).
//!
//! Verifican la resolución de configuración de `scripts/llm_provider.py` por
//! `--show-config` (default `local`/llama.cpp, override `openai` por entorno y
//! `--provider`, `ollama` legacy y la key obligatoria de `openai`) y que
//! `scripts/llm_judge.py --show-prompt` construye el prompt T2 sin tocar la
//! red. Solo `std`.
//!
//! Intérprete: `python` con fallback a `python3`; sin intérprete el test falla
//! ruidosamente (nunca verde silencioso).

#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Variables de entorno que se limpian para aislar cada caso (determinismo).
const SCRUB_ENV: &[&str] = &[
    "RUSCADB_LLM_PROVIDER",
    "RUSCADB_LLM_MODEL",
    "RUSCADB_LLM_BASE_URL",
    "RUSCADB_LLM_API_KEY",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
];

/// Raíz del workspace: dos niveles sobre `crates/xtask`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Devuelve el intérprete de Python disponible (`python` o `python3`).
fn python_interpreter() -> String {
    for candidate in ["python", "python3"] {
        if let Ok(output) = Command::new(candidate).arg("--version").output()
            && output.status.success()
        {
            return candidate.to_string();
        }
    }
    panic!(
        "WHAT: falta un intérprete de Python para los tests de SPEC-0100. \
         WHY: scripts/llm_provider.py y llm_judge.py son Python y no se pueden \
         verificar sin él. WHERE: instala Python 3 en PATH ('python' o \
         'python3'); test en crates/xtask/tests/llm_provider.rs."
    );
}

/// Ejecuta un script de `scripts/` con entorno controlado y argumentos dados.
///
/// Limpia primero las claves de `remove` y luego fija las de `envs`, de modo
/// que las variables del caso mandan sobre las heredadas.
///
/// Args:
///     script: Nombre del script bajo `scripts/`.
///     envs: Pares `(clave, valor)` a fijar.
///     remove: Claves a eliminar del entorno (aislamiento determinista).
///     args: Argumentos de línea de comandos del script.
///
/// Returns:
///     La salida del proceso (stdout, stderr, exit code).
fn run_script(script: &str, envs: &[(&str, &str)], remove: &[&str], args: &[&str]) -> Output {
    let path = workspace_root().join("scripts").join(script);
    let mut command = Command::new(python_interpreter());
    command.arg(path);
    for key in remove {
        command.env_remove(key);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    for arg in args {
        command.arg(arg);
    }
    command
        .output()
        .unwrap_or_else(|e| panic!("no se pudo ejecutar {script}: {e}"))
}

/// Concatena stdout y stderr (para asertar mensajes y códigos).
fn combined(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

/// AC-0100-01: por defecto el proveedor es `local` (llama.cpp) con protocolo
/// OpenAI-compatible en 8080/v1, el modelo por defecto y sin exigir API key.
#[test]
// @spec AC-0100-01
fn test_ac_0100_01_show_config_defaults_local() {
    let output = run_script("llm_provider.py", &[], SCRUB_ENV, &["--show-config"]);
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "el default local debe salir 0; salida:\n{text}"
    );
    assert!(
        text.contains("\"provider\": \"local\""),
        "el default debe ser local; salida:\n{text}"
    );
    assert!(
        text.contains("\"base_url\": \"http://127.0.0.1:8080/v1\""),
        "local debe apuntar a llama.cpp en 8080/v1; salida:\n{text}"
    );
    assert!(
        text.contains("\"model\": \"qwen3-4b-thinking-2507-q4-k-m\""),
        "modelo local por defecto; salida:\n{text}"
    );
    assert!(
        text.contains("\"api_key_present\": false"),
        "local no exige key (api_key_present=false); salida:\n{text}"
    );
}

/// AC-0100-02: `RUSCADB_LLM_PROVIDER=openai` (producción) cambia base y modelo
/// y reporta la key presente; `--provider local` sobreescribe el entorno.
#[test]
// @spec AC-0100-02
fn test_ac_0100_02_show_config_openai_env_override() {
    let envs = [
        ("RUSCADB_LLM_PROVIDER", "openai"),
        ("OPENAI_API_KEY", "test-key"),
    ];
    let scrub = [
        "RUSCADB_LLM_MODEL",
        "RUSCADB_LLM_BASE_URL",
        "OPENAI_BASE_URL",
    ];

    let output = run_script("llm_provider.py", &envs, &scrub, &["--show-config"]);
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "openai con key debe salir 0; salida:\n{text}"
    );
    assert!(
        text.contains("\"provider\": \"openai\""),
        "el entorno debe forzar openai; salida:\n{text}"
    );
    assert!(
        text.contains("\"base_url\": \"https://api.openai.com/v1\""),
        "openai debe usar su base por defecto; salida:\n{text}"
    );
    assert!(
        text.contains("\"model\": \"gpt-4o-mini\""),
        "modelo openai por defecto; salida:\n{text}"
    );
    assert!(
        text.contains("\"api_key_present\": true"),
        "con OPENAI_API_KEY la key está presente; salida:\n{text}"
    );

    let overridden = run_script(
        "llm_provider.py",
        &envs,
        &scrub,
        &["--provider", "local", "--show-config"],
    );
    let text = combined(&overridden);
    assert_eq!(
        overridden.status.code(),
        Some(0),
        "el override CLI debe salir 0; salida:\n{text}"
    );
    assert!(
        text.contains("\"provider\": \"local\""),
        "--provider local gana al entorno; salida:\n{text}"
    );
    assert!(
        text.contains("\"base_url\": \"http://127.0.0.1:8080/v1\""),
        "al volver a local apunta a llama.cpp; salida:\n{text}"
    );
}

/// AC-0100-03: `openai` sin `OPENAI_API_KEY` falla ruidoso (exit 2) con mensaje
/// accionable; `local` (default) nunca exige key.
#[test]
// @spec AC-0100-03
fn test_ac_0100_03_openai_missing_key_exits_two() {
    let output = run_script(
        "llm_provider.py",
        &[("RUSCADB_LLM_PROVIDER", "openai")],
        SCRUB_ENV,
        &["--show-config"],
    );
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(2),
        "openai sin key debe salir 2; salida:\n{text}"
    );
    assert!(
        text.contains("OPENAI_API_KEY"),
        "el mensaje debe nombrar OPENAI_API_KEY; salida:\n{text}"
    );

    let local = run_script("llm_provider.py", &[], SCRUB_ENV, &["--show-config"]);
    assert_eq!(
        local.status.code(),
        Some(0),
        "local no exige key; salida:\n{}",
        combined(&local)
    );
}

/// AC-0100-04: `ollama` (legacy) usa el puerto nativo 11434 y no exige key.
#[test]
// @spec AC-0100-04
fn test_ac_0100_04_show_config_ollama_legacy() {
    let output = run_script(
        "llm_provider.py",
        &[("RUSCADB_LLM_PROVIDER", "ollama")],
        SCRUB_ENV,
        &["--show-config"],
    );
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "ollama debe salir 0; salida:\n{text}"
    );
    assert!(
        text.contains("\"provider\": \"ollama\""),
        "el entorno debe seleccionar ollama; salida:\n{text}"
    );
    assert!(
        text.contains("\"base_url\": \"http://127.0.0.1:11434\""),
        "ollama usa su puerto nativo; salida:\n{text}"
    );
    assert!(
        text.contains("\"api_key_present\": false"),
        "ollama no usa key; salida:\n{text}"
    );
}

/// AC-0100-05: `llm_judge.py --show-prompt` imprime el prompt T2 (Spec
/// Fidelity >= 0.90 + adversarial, 0 hallazgos HIGH) sin tocar la red ni
/// resolver el proveedor (se fuerza openai sin key y aun así sale 0).
#[test]
// @spec AC-0100-05
fn test_ac_0100_05_judge_show_prompt_offline() {
    let dir = std::env::temp_dir().join(format!("ruscadb_llm_judge_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("directorio temporal");
    let diff = dir.join("change.diff");
    std::fs::write(
        &diff,
        "+++ b/crates/ruscadb-core/src/record.rs\n@@ -1 +1 @@\n-pub fn f() {}\n+pub fn f() { /* cambio */ }\n",
    )
    .expect("escribir el diff");
    let path = diff.to_str().expect("ruta utf-8");

    let output = run_script(
        "llm_judge.py",
        &[("RUSCADB_LLM_PROVIDER", "openai")],
        SCRUB_ENV,
        &["--show-prompt", path],
    );
    let text = combined(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "--show-prompt no debe tocar la red ni exigir config; salida:\n{text}"
    );
    assert!(
        text.contains("Spec Fidelity"),
        "el prompt debe citar la fidelidad a la spec; salida:\n{text}"
    );
    assert!(
        text.contains("0.90"),
        "el prompt debe citar el umbral 0.90; salida:\n{text}"
    );
    assert!(
        text.contains("HIGH"),
        "el prompt debe exigir 0 hallazgos HIGH; salida:\n{text}"
    );
    assert!(
        text.contains("cambio"),
        "el prompt debe incluir el diff; salida:\n{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
