//! Tests de aceptación de SPEC-0083 (embeddings estáticos `model2vec-rs`).
//!
//! Generan un **fixture safetensors mínimo** en directorio temporal (sin red,
//! sin modelo real de ~30 MB: NF-0083-01) y asientan carga local, gating de
//! la feature y determinismo byte a byte.
//!
//! Los tests de `StaticEmbedder` solo existen con la feature activa:
//! `cargo test -p ruscadb-ai --features static-embeddings`.
//! El AC-0083-02 corre SIEMPRE (verifica el build por defecto sin la feature).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Contador para directorios temporales únicos por proceso/test.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Dimensión del fixture safetensors (columnas de la matriz `embeddings`).
const FIXTURE_DIM: usize = 4;

/// Vocabulario WordPiece del fixture: `[UNK]`=0, `hello`=1, `world`=2,
/// `static`=3, `embeddings`=4 (ids = fila de la matriz de pesos).
const FIXTURE_VOCAB: &str = r#"{"[UNK]":0,"hello":1,"world":2,"static":3,"embeddings":4}"#;

/// Serializa la matriz de pesos del fixture como safetensors con tensor
/// `embeddings` (2-D, F32, little-endian), el formato que lee
/// `model2vec-rs::StaticModel::from_bytes` vía `SafeTensors::deserialize`.
///
/// Args:
///     rows: Filas de la matriz (tamaño del vocabulario).
///     cols: Columnas (dimensión de los vectores).
///     data: Pesos en orden fila-major (`rows * cols` valores).
///
/// Returns:
///     Bytes completos del fichero `.safetensors` (header JSON alineado a
///     8 bytes como exige el formato).
fn safetensors_bytes(rows: usize, cols: usize, data: &[f32]) -> Vec<u8> {
    let header = format!(
        "{{\"embeddings\":{{\"dtype\":\"F32\",\"shape\":[{rows},{cols}],\
         \"data_offsets\":[0,{}]}}}}",
        data.len() * 4
    );
    let mut padded = header;
    while padded.len() % 8 != 0 {
        padded.push(' ');
    }
    let mut out = Vec::with_capacity(8 + padded.len() + data.len() * 4);
    out.extend_from_slice(&(padded.len() as u64).to_le_bytes());
    out.extend_from_slice(padded.as_bytes());
    for value in data {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Directorio temporal con un modelo model2vec mínimo; se borra al soltar.
struct TempModel {
    /// Ruta del directorio con `config.json` + `tokenizer.json` + `model.safetensors`.
    path: PathBuf,
}

impl TempModel {
    /// Crea el directorio temporal y escribe los tres ficheros del fixture.
    ///
    /// Returns:
    ///     El fixture listo para `StaticEmbedder::from_local_dir`.
    ///
    /// Panics:
    ///     Si no se puede crear el directorio o escribir los ficheros (los
    ///     tests de aceptación no tienen dónde cargar si falla el disco).
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ruscadb_m2v_fixture_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap_or_else(|err| {
            panic!(
                "WHAT: no se pudo crear el dir del fixture '{}'. WHY: {err}. \
                 WHERE: TempModel::new (SPEC-0083).",
                path.display()
            )
        });
        // tokenizador WordPiece + WhitespaceSplit mínimo (sin red).
        let tokenizer = format!(
            r###"{{"version":"1.0","truncation":null,"padding":null,"added_tokens":[],"normalizer":null,"pre_tokenizer":{{"type":"WhitespaceSplit"}},"post_processor":null,"decoder":null,"model":{{"type":"WordPiece","vocab":{FIXTURE_VOCAB},"unk_token":"[UNK]","continuing_subword_prefix":"##","max_input_chars_per_word":100}}}}"###
        );
        std::fs::write(path.join("tokenizer.json"), tokenizer).unwrap_or_else(|err| {
            panic!(
                "WHAT: no se pudo escribir tokenizer.json. WHY: {err}. \
                 WHERE: TempModel::new (SPEC-0083)."
            )
        });
        // normalize=true: el modelo ya L2-normaliza; el embedder lo repite
        // por contrato del puerto Embedder.
        std::fs::write(path.join("config.json"), r#"{"normalize":true,"dim":4}"#).unwrap_or_else(
            |err| {
                panic!(
                    "WHAT: no se pudo escribir config.json. WHY: {err}. \
                     WHERE: TempModel::new (SPEC-0083)."
                )
            },
        );
        // Pesos: fila 0 = ceros ([UNK]), filas 1..4 = vectores unitarios.
        // Tamaño total = 5 filas (vocab) × FIXTURE_DIM columnas.
        let mut data = vec![0.0_f32; 5 * FIXTURE_DIM];
        for row in 1..5 {
            data[row * FIXTURE_DIM + (row - 1)] = 1.0;
        }
        std::fs::write(
            path.join("model.safetensors"),
            safetensors_bytes(5, FIXTURE_DIM, &data),
        )
        .unwrap_or_else(|err| {
            panic!(
                "WHAT: no se pudo escribir model.safetensors. WHY: {err}. \
                 WHERE: TempModel::new (SPEC-0083)."
            )
        });
        Self { path }
    }

    /// Ruta del directorio del fixture.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempModel {
    /// Elimina el directorio temporal (mejora esfuerzo: tolera fallos).
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// AC-0083-01: `StaticEmbedder` carga el fixture safetensors local y
/// devuelve el vector esperado (mean-pool de `hello`+`world` con L2) con la
/// dim declarada, sin red.
#[cfg(feature = "static-embeddings")]
#[test]
// @spec AC-0083-01
fn test_ac_0083_01_static_embedder_loads_local_fixture() {
    use ruscadb_ai::Embedder;

    let fixture = TempModel::new();
    let embedder =
        ruscadb_ai::StaticEmbedder::from_local_dir(fixture.path()).expect("fixture válido");
    assert_eq!(
        embedder.dim(),
        FIXTURE_DIM,
        "la dim debe venir de los pesos"
    );

    // "hello world" -> ids [1,2] -> filas [1,0,0,0] y [0,1,0,0] ->
    // media [0.5,0.5,0,0] -> L2 => [s,s,0,0] con s = 0.5 / sqrt(0.5).
    let vector = embedder.embed("hello world");
    assert_eq!(vector.len(), FIXTURE_DIM, "longitud igual a la dim");
    let s = 0.5_f32 / (0.5_f32 * 0.5_f32 + 0.5_f32 * 0.5_f32).sqrt();
    for (index, esperado) in [s, s, 0.0, 0.0].into_iter().enumerate() {
        assert!(
            (vector[index] - esperado).abs() < 1e-6,
            "componente {index}: esperado {esperado}, obtenido {}",
            vector[index]
        );
    }
}

/// AC-0083-02: el build por defecto no arrastra `model2vec-rs` — la
/// dependencia es opcional y toda mención al código vive detrás de
/// `#[cfg(feature = "static-embeddings")]`, con los embedders puros
/// operativos. Corre con y sin la feature.
#[test]
// @spec AC-0083-02
fn test_ac_0083_02_default_build_stays_dependency_free() {
    use ruscadb_ai::Embedder;

    // (a) Los embedders puros del build por defecto siguen funcionando.
    let puro = ruscadb_ai::HashingEmbedder::new(8).expect("embedder puro");
    assert_eq!(puro.embed("hola mundo").len(), 8);

    // (b) Cargo.toml: opcional + mapeado a la feature + fuera de default.
    let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    let linea_dep = manifest
        .lines()
        .find(|line| line.contains("model2vec-rs ="))
        .expect("el manifest debe declarar model2vec-rs");
    assert!(
        linea_dep.contains("optional = true"),
        "model2vec-rs debe ser opcional: {linea_dep}"
    );
    assert!(
        manifest.contains("static-embeddings = [\"dep:model2vec-rs\"]"),
        "la feature debe mapear dep:model2vec-rs"
    );
    assert!(
        manifest.contains("default = []"),
        "default debe seguir vacío (sin static-embeddings)"
    );

    // (c) Ninguna línea de CÓDIGO (sin comentarios) menciona model2vec sin
    // estar bajo #[cfg(feature = "static-embeddings")].
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"));
    let cfg_esperado = "#[cfg(feature = \"static-embeddings\")]";
    let mut ultimo_cfg: Option<&str> = None;
    let mut violaciones: Vec<String> = Vec::new();
    for (indice, linea) in src.lines().enumerate() {
        let texto = linea.trim_start();
        if texto.starts_with("//") {
            continue; // docs y comentarios: no son código compilable
        }
        if texto.starts_with("#[cfg") {
            ultimo_cfg = Some(texto);
            continue;
        }
        if texto.contains("model2vec") && ultimo_cfg != Some(cfg_esperado) {
            violaciones.push(format!("linea {}: {texto}", indice + 1));
        }
    }
    assert!(
        violaciones.is_empty(),
        "model2vec en código sin cfg static-embeddings: {violaciones:?}"
    );
}

/// AC-0083-03: dos instancias construidas desde el mismo fixture producen
/// vectores idénticos byte a byte y la misma dim (determinismo puro).
#[cfg(feature = "static-embeddings")]
#[test]
// @spec AC-0083-03
fn test_ac_0083_03_static_embedder_is_deterministic() {
    use ruscadb_ai::Embedder;

    let fixture = TempModel::new();
    let primero = ruscadb_ai::StaticEmbedder::from_local_dir(fixture.path()).expect("fixture 1");
    let segundo = ruscadb_ai::StaticEmbedder::from_local_dir(fixture.path()).expect("fixture 2");

    let a = primero.embed("hello static embeddings");
    let b = segundo.embed("hello static embeddings");
    assert_eq!(
        primero.dim(),
        segundo.dim(),
        "dim constante entre instancias"
    );
    assert_eq!(a.len(), FIXTURE_DIM, "dim constante entre textos");
    // Igualdad byte a byte (f32 por bits) entre instancias y entre llamadas.
    let en_bytes = |vector: &[f32]| -> Vec<u8> {
        vector
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    };
    assert_eq!(
        en_bytes(&a),
        en_bytes(&b),
        "mismo texto + mismo fixture => mismos bytes"
    );
    let repetido = primero.embed("hello static embeddings");
    assert_eq!(
        en_bytes(&a),
        en_bytes(&repetido),
        "la misma instancia es determinista"
    );
}

/// Complemento de NF-0083-03 (sin marcador AC): un directorio inexistente
/// produce error WHAT+WHY+WHERE accionable, nunca un panic ni un vector
/// silencioso.
#[cfg(feature = "static-embeddings")]
#[test]
fn static_embedder_reports_actionable_error_for_missing_model() {
    let faltante = std::env::temp_dir().join("ruscadb_m2v_no_existe_nunca");
    let error = ruscadb_ai::StaticEmbedder::from_local_dir(&faltante)
        .expect_err("un directorio inexistente debe fallar");
    let texto = error.to_string();
    for marca in ["WHAT:", "WHY:", "WHERE:"] {
        assert!(
            texto.contains(marca),
            "el error debe llevar {marca}: {texto}"
        );
    }
}
