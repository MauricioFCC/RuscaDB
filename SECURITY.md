# Política de seguridad

RuscaDB es una base de datos **embebida, in-process, multi-modelo y multimodal**
escrita en Rust. Esta política describe cómo reportar vulnerabilidades, qué
versiones se soportan, el alcance del modelo de amenazas y la política de
`unsafe`.

El canon de referencia es **OWASP** (Top 10, ASVS, SAMM), **MITRE CWE** y las
prácticas de seguridad de GitHub (security advisories privados, Dependabot,
secret scanning, push protection). Las clases de debilidad relevantes para una
DB in-process se citan como `CWE-XXXX`.

---

## Versiones soportadas

Solo se aplican parches de seguridad a la última versión de la serie `0.1.x`
(estado pre-1.0, API inestable).

| Versión | Soportada | Notas |
|---|---|---|
| `0.2.x` | ❌ (futuro) | No publicada. |
| `0.1.x` | ✅ | Serie actual. Se parchea la última `0.1.z`. |
| `< 0.1` / `main` | ⚠️ | Sin garantía; se corrige si es trivial y ya está resuelto en `0.1.x`. |

Antes de 1.0 el proyecto puede introducir cambios incompatibles en versiones
`MINOR`; los avisos de seguridad se publican siempre para la última `0.1.z`.

---

## Cómo reportar una vulnerabilidad

**NO abras un issue público.** Un issue público expone la vulnerabilidad antes
de que exista un parche (coordinación de divulgación responsable / `CWE-1059`
deficiencias de documentación de seguridad).

Usa uno de estos canales privados:

1. **GitHub Security Advisory (preferido)** — pestaña *Security → Advisories →
   Report a vulnerability* del repositorio
   (<https://github.com/ruscadb/ruscadb/security/advisories/new>). Permite
   discutir el reporte de forma privada y coordinar el CVE.
2. **Email** — `security@ruscadb.dev`. Si quieres cifrar el reporte, solicita
   primero la clave pública PGP en ese mismo buzón.

Incluye, en lo posible:

- versión de RuscaDB (`0.1.z`) y plataforma/arquitectura;
- el crate o capa afectada (`ruscadb-query`, `ruscadb-wal`, `ruscadb-ffi`,
  `ruscadb-vector`, `ruscadb-multimodal`, `ruscadb-crypto`, …);
- una descripción del impacto y un PoC mínimo (input, RQL, blob o llamada al
  C-ABI) que reproduzca el fallo;
- clasificación sugerida `CWE-XXXX` y, si aplica, el mapeo OWASP;
- si lo deseas, tu nombre/afiliación y si quieres crédito público.

### Tiempos de respuesta objetivo

| Etapa | Objetivo |
|---|---|
| Acuse de recibo | ≤ 3 días hábiles |
| Triage inicial (severidad CVSS + `CWE`) | ≤ 7 días hábiles |
| Plan de mitigación / parche | Crítico/Alto ≤ 30 días; Medio/Bajo ≤ 90 días |
| Divulgación coordinada | Tras publicar el parche, o ≤ 90 días desde el reporte |

Estos plazos son objetivos de buena fe (proyecto pre-1.0, mantenido por
voluntarios). Si el reporte está activamente explotado, se prioriza la
mitigación inmediata.

---

## Alcance del modelo de amenazas

RuscaDB corre **en el mismo proceso que la aplicación anfitriona**: no hay
frontera de proceso ni de red. Por tanto, **un fallo de corrupción de memoria en
el parser, la deserialización o la FFI es un fallo de la app anfitriona** (RCE o
corrupción de datos, `CWE-119` memoria fuera de límites, `CWE-416`
use-after-free, `CWE-787` escritura fuera de límites). «Es seguro por estar en
Rust» es falso: el `unsafe` acotado, la deserialización y el parser son la
superficie real.

### Superficie en alcance

| Superficie | Ejemplos de debilidad | Referencia |
|---|---|---|
| **Parser RQL** | Inyección por interpolación, DoS por complejidad (regex catastrófica, `OR` anidado), bypass del sandbox de `file()`/`net` | `CWE-89`, `CWE-1333`, `CWE-400` |
| **Deserialización WAL / blob** | `parquet`/Arrow malformado, offsets/lengths no validados, zip-bomb, torn writes | `CWE-119`, `CWE-787`, `CWE-20` |
| **FFI C-ABI (`ruscadb-ffi`)** | `(ptr, len)` inconsistente, doble `free`, use-after-free, panic que cruza el ABI | `CWE-119`, `CWE-416`, `CWE-787`, `CWE-617` |
| **Índice vectorial HNSW / grafo CSR** | OOB por lista de vecinos corrupta, `get_unchecked` sin invariantes | `CWE-119`, `CWE-787` |
| **Cifrado en reposo (`ruscadb-crypto`)** | Nonce reutilizado, AEAD sin verificación de tag, KDF débil, material sensible sin `zeroize` | `CWE-323`, `CWE-347`, `CWE-330`, `CWE-316` |
| **Supply chain** | Crate `build.rs` malicioso, dependencia comprometida, modelo `.onnx` malicioso | `CWE-1104`, `CWE-506` |

### Fuera de alcance

- **Ataques que requieren acceso físico o root** al host: cifrado en reposo
  protege el archivo en disco, no un adversario con control total de la máquina
  (keylogging, volcado de memoria del proceso vivo).
- **La aplicación anfitriona**: validación de identidad, autenticación,
  autorización de negocio, TLS/servidor y manejo de las claves que RuscaDB
  recibe. RuscaDB es una librería embebida: no expone red ni usuarios.
- **Configuración insegura deliberada**: capacidades `file`/`net`/`script`
  habilitadas, presupuestos de recursos elevados o desactivar la verificación
  de integridad (CRC32C/BLAKE3) por parte del integrador.
- **DoS de recursos** cuando el anfitrión amplía los límites por encima de los
  defaults seguros documentados (`docs/RuscaDB-roadmap.md` §6.4).
- **Dependencias de terceros**: se enrutan a su upstream (RustSec), aunque el
  proyecto las rastrea con `cargo-audit`/`cargo-deny`/`cargo-vet` y SBOM.

---

## Política de `unsafe`

El `unsafe` es el riesgo número uno de una DB in-process. La política es
**release-blocking**:

- **Solo `ruscadb-ffi` habilita `unsafe`** (`#![allow(unsafe_code)]`) porque su
  contrato es el C-ABI. Todos los demás crates lo prohíben con
  `#![forbid(unsafe_code)]`.
- El **presupuesto de bloques `unsafe` por crate** vive versionado en
  [`unsafe-allowlist.toml`](unsafe-allowlist.toml) y lo verifica
  `scripts/check_core_unsafe.py` en el gate T1. Ningún `unsafe` nuevo entra sin
  ampliar el presupuesto en un PR revisado.
- **Cada bloque `unsafe` lleva un comentario `// SAFETY:`** que justifica por qué
  las precondiciones se cumplen. Los lints
  `undocumented_unsafe_blocks`, `missing_safety_doc` y
  `multiple_unsafe_ops_per_block` están en `deny`; `unsafe_op_in_unsafe_fn` en
  `deny`.
- La frontera C valida `(ptr, len)` antes de `from_raw_parts`/dereferenciar, usa
  una **tabla de handles con marca mágica + generación** (anti doble-free /
  use-after-free) y captura panics con `catch_unwind` para que ninguno cruce el
  ABI.
- Cobertura adicional: `miri` (UB), ASan/UBSan, `cargo-geiger`, y `cargo-fuzz`
  sobre parser, WAL/CRC, blob, FFI y modelos.

---

## Hardening y controles

- **Integridad**: CRC32C por frame de WAL y BLAKE3 en páginas/manifiesto;
  frames manipulados se truncan o rechazan.
- **Datos en reposo** (opcional): AEAD XChaCha20-Poly1305 / AES-256-GCM,
  passphrase → Argon2id → DEK, `secrecy` + `zeroize` para el material sensible.
- **Límites de recursos** por defecto: profundidad AST 64, query budget
  `100 000`, timeout 5 000 ms, `LIMIT` implícito 1 000 filas, blob 256 MiB,
  dimensión de vector ≤ 4 096, traversal depth 16.
- **Path safety**: `file()` restringido al *sandbox root* (canonicalización +
  prefijo; anti `..`/symlink).
- **Supply chain**: `Cargo.lock --locked`, `cargo-audit`, `cargo-deny`,
  `cargo-vet`, toolchain pinneada, SBOM CycloneDX por release y `gitleaks` en
  pre-commit/CI.

Gracias por ayudar a mantener RuscaDB seguro.
