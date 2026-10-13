# Operación de RuscaDB

Runbook de operación de una base **embebida** RuscaDB: abrirla con seguridad
(lock), migrar el esquema, hacer backup y restore, rotar claves y resolver los
fallos más comunes. Todos los comandos y mensajes de error de este documento
están copiados del estado real del repositorio (`0.1.x`) y son ejecutables tal
cual.

Documentos relacionados:

- Reporte de vulnerabilidades y modelo de amenazas: [`SECURITY.md`](../SECURITY.md).
- Gates de verificación (T1/T2/T3) y *golden path*: [`docs/verification.md`](verification.md).

## Ficheros de una base

Una base se identifica por la ruta del fichero de datos (p. ej. `app.db`). A
partir de esa ruta, la fachada deriva el resto de ficheros **en el mismo
directorio** (`crates/ruscadb/src/database.rs` y `crates/ruscadb/src/lock.rs`):

| Ruta derivada | Contenido |
|---|---|
| `app.db` | Archivo de páginas (heap). |
| `app.wal` | Global write-ahead log (frames con CRC32C). |
| `app.lock` | Sidecar del lock cross-proceso (vacío; el lock vive en el SO). |
| `app.manifest.json` | Manifiesto versionado (`schema_version`, `epoch`, `checkpoint_lsn`). |
| `app.models.json` | Registro de modelos, si se usa. |
| `app.audit.jsonl` | Trail de auditoría, si está activado. |

Un backup correcto copia el `.data`, el `.wal` y el `.manifest.json` **como un
conjunto**; el manifiesto fija el `checkpoint_lsn` desde el que se reproduce el
WAL.

## Abrir una base (lock y DatabaseInUse)

`Database::open` adquiere un **lock exclusivo no-bloqueante** sobre el sidecar
`<data>.lock` **antes** de tocar el fichero de datos (SPEC-0076). Si otra
instancia —otro proceso u otra apertura en el mismo proceso— ya lo tiene, la
apertura falla sin bloquear ni abrir el `.data`:

```text
base en uso: app.db está bloqueada por otra instancia de RuscaDB; cierra ese proceso o usa otra ruta
```

Reglas de operación:

- **Una sola `Database` por ruta de datos a la vez.** El lock es exclusivo.
- El lock se libera al dropear la `Database` o al llamar a `close()`; el sistema
  operativo también lo libera si el proceso muere (no hay locks huérfanos).
- **No borres `app.lock` a mano** mientras un proceso lo tiene: el lock real
  vive en el descriptor abierto; borrar el fichero no lo libera.
- El lock se toma **sobre el sidecar**, nunca sobre el `.data`, para evitar el
  `LockFileEx` obligatorio de Windows.

Reproduce el comportamiento con el test real (un segundo proceso es rechazado):

```powershell
cargo test -p ruscadb --test process_lock -- --nocapture
```

## Migración de esquema (forward-only)

El manifiesto (`app.manifest.json`) declara `schema_version`. Al abrir, la
fachada valida y migra con `ruscadb-txn` (`Manifest::load_with_migrations`,
SPEC-0077). La política es **solo hacia adelante** (como el `user_version` de
SQLite): nunca hay downgrade.

| Situación | Error (`RuscaError`) | Mensaje real |
|---|---|---|
| El fichero es más nuevo que el binario | `SchemaVersionTooNew` | `esquema no soportado: el manifiesto declara schema_version 2 y este binario solo soporta 1; actualiza RuscaDB y no retrocedas el binario` |
| Versión antigua sin migración registrada | `MigrationFailed` | `migración de esquema no disponible: de 0 a 1: no hay migración registrada desde schema_version 0 hacia 1; registra el salto 0 -> ? o actualiza el binario` |
| JSON inválido o campos faltantes | `CorruptManifest` | `manifiesto corrupto: manifiesto inválido en app.manifest.json: <detalle>` |

Garantías:

- **Atómico**: el manifiesto migrado se persiste con `tmp` + `fsync` + `rename`.
  Si algo falla, el fichero original queda **intacto**.
- **Dry-run**: `Manifest::migrate_plan` calcula los pasos `from -> to` sin tocar
  el fichero.
- **Sin downgrade**: bajar de versión es un error explícito
  (`MigrationFailed`), no una operación silenciosa.

Verifica la lógica de migración con los tests reales:

```powershell
cargo test -p ruscadb-txn --lib manifest
cargo test -p ruscadb-txn --lib migration
```

Si el manifiesto declara una versión futura, **actualiza el binario**; no
retrocedas el binario ni edites `schema_version` a mano.

## Backup y restore

**Backup en frío (recomendado).** Cierra la base (libera el lock) y copia el
conjunto de ficheros derivados. La ausencia del lock garantiza que no hay
escritor durante la copia:

```powershell
# 1) Cierra la base (close()/drop). Sin el lock, nadie escribe.
# 2) Copia .data, .wal y .manifest.json JUNTOS (mismo directorio).
#    .models.json y .audit.jsonl son opcionales.
New-Item -ItemType Directory -Force -Path .\backup | Out-Null
Copy-Item app.db, app.wal, app.manifest.json -Destination .\backup\ -Force
Copy-Item app.models.json, app.audit.jsonl -Destination .\backup\ -Force -ErrorAction SilentlyContinue
```

**Backup en caliente (limitado en 0.1.x).** No existe API de *snapshot* online.
Copiar los ficheros con la base abierta por un escritor **no** está garantizado
consistente. Para una copia en caliente, haz un *dump* lógico: abre en lectura,
`SELECT *` de cada tabla y vuelca a otro directorio (con la clave si está
cifrado).

**Restore.**

1. Detén el proceso que tenga la base abierta.
2. Restaura el **conjunto** `.data` + `.wal` + `.manifest.json` de un mismo
   backup (un WAL que no casa con su manifiesto es un backup rasgado).
3. Reabre la base; el recovery del WAL se apoya en el CRC32C por frame y trunca
   en el último frame válido.

Verifica el manifiesto tras el restore:

```powershell
cargo test -p ruscadb-txn --lib manifest
```

## Rotación de claves

El cifrado en reposo es **opcional** y se configura en la apertura vía
`EncryptionConfig`, con clave directa de 32 B (`EncryptionConfig::new`) o
derivada de una *passphrase* con Argon2id (`EncryptionConfig::from_passphrase`).
El material de clave vive en `zeroize::Zeroizing` y su `Debug` está redactado.

> **Estado real (0.1.x):** RuscaDB **no** soporta rotación de claves en caliente
> ni KMS/HSM; está declarado fuera de alcance en SPEC-0013
> (`crates/ruscadb/src/encryption.rs`). Rotar una clave exige una **re-clave
> lógica** (dump con la clave vieja, carga con la nueva):

1. Haz un backup en frío con la clave actual (ver arriba).
2. Abre la base con la **clave vieja** y vuelca todas las tablas (`SELECT *`) a
   un directorio nuevo.
3. Crea la base nueva con la **clave nueva**
   (`EncryptionConfig::new(new_key)` o `from_passphrase(new_passphrase, new_salt)`)
   y reinserta los datos.
4. Verifica que la base nueva abre con la clave nueva y que la clave vieja ya
   **no** la abre.
5. Retira la clave vieja solo cuando la base nueva esté verificada.

Abrir un WAL con una clave errónea, o sin clave uno cifrado, **falla en modo
estricto** con `WalCorrupt` y nunca expone texto en claro. Comprueba el
comportamiento real:

```powershell
cargo test -p ruscadb --lib encryption
```

## Troubleshooting

| Síntoma (mensaje real) | Causa | Acción |
|---|---|---|
| `base en uso: <ruta> está bloqueada por otra instancia de RuscaDB; cierra ese proceso o usa otra ruta` | Otro proceso o apertura tiene el lock `<data>.lock`. | Cierra el otro proceso; no borres `app.lock` a mano. Verifica con `cargo test -p ruscadb --test process_lock`. |
| `esquema no soportado: el manifiesto declara schema_version N ...` | El fichero lo escribió un binario más nuevo. | Actualiza RuscaDB; **no** retrocedas el binario ni edites `schema_version`. |
| `migración de esquema no disponible: de X a Y: ...` | Falta la migración `X -> ?` en este binario. | Usa el binario que registra el salto; si ya avanzaste el fichero, restaura el backup. |
| `manifiesto corrupto: ...` | JSON inválido o campos obligatorios ausentes. | Restaura `app.manifest.json` desde el backup. |
| `WAL corrupto: ...` | Frame rasgado, manipulado o clave incorrecta. | Comprueba la clave; el recovery trunca en el último frame con CRC32C válido. |
| `buffer pool lleno: capacidad N, sin páginas desalojables` | `pool_capacity` insuficiente (habitual en modo cifrado). | Aumenta `pool_capacity` al dimensionar el conjunto de trabajo. |

Para el detalle de cada gate de verificación y cómo correrlo localmente, consulta
[`docs/verification.md`](verification.md). Para reportar una vulnerabilidad,
lee [`SECURITY.md`](../SECURITY.md).
