//! # ruscadb-ffi
//!
//! C ABI estable de RuscaDB: contrato único para todos los drivers. Valida
//! `(ptr, len)` antes de `from_raw_parts`, usa una tabla global de handles
//! (anti use-after-free/doble-free) y `catch_unwind` en la frontera para que
//! ningún panic cruce el ABI.
//!
//! Diseño: ADR-008/ADR-011 y `docs/RuscaDB-roadmap.md` §6.2. Fase: F5.

#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
// La frontera C recibe punteros crudos por contrato; la validación de
// `(ptr, len)` y el registro de handles ocurren antes de cualquier desreferencia.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::{c_char, c_int};
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use ruscadb::{Database, DbConfig, PAGE_SIZE, Page, PageId};

/// Código de retorno: operación exitosa.
pub const RC_OK: c_int = 0;
/// Código de retorno: puntero nulo.
pub const RC_NULL_POINTER: c_int = 1;
/// Código de retorno: handle no registrado (inválido o ya liberado).
pub const RC_INVALID_HANDLE: c_int = 2;
/// Código de retorno: error de dominio, E/S o longitud inconsistente.
pub const RC_DOMAIN_ERROR: c_int = 3;
/// Código de retorno: panic capturado en la frontera FFI.
pub const RC_PANIC: c_int = 4;
/// Código de retorno: buffer de salida insuficiente (no se escribió nada);
/// el tamaño requerido está en `ruscadb_last_error`.
pub const RC_BUFFER_TOO_SMALL: c_int = 5;

/// Marca de handle vivo (`"RUSCADB1"` en ASCII). Detecta punteros corruptos.
const HANDLE_MAGIC: u64 = 0x5255_5343_4144_4231;

/// Handle opaco del C-ABI: envuelve el motor con exclusión mutua.
///
/// El llamador solo maneja `*mut RuscadbHandle`; nunca inspecciona su contenido.
pub struct RuscadbHandle {
    /// Marca que acredita que el puntero apunta a un handle construido por
    /// [`ruscadb_open`] (defensa ante punteros corruptos).
    magic: u64,
    /// Motor RuscaDB protegido para acceso concurrente desde hilos C.
    database: Mutex<Database>,
}

/// Registro global de handles vivos (direcciones base).
///
/// Impide doble-free y use-after-free: `close` solo libera punteros que siguen
/// registrados, y cualquier operación sobre un puntero ausente falla sin
/// desreferenciar.
fn registry() -> &'static Mutex<HashSet<usize>> {
    static REGISTRY: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Registra la dirección de un handle vivo; devuelve `true` si era nueva.
fn register(pointer: usize) -> bool {
    match registry().lock() {
        Ok(mut set) => set.insert(pointer),
        Err(poisoned) => poisoned.into_inner().insert(pointer),
    }
}

/// Retira la dirección del registro; devuelve `true` si estaba registrada.
fn unregister(pointer: usize) -> bool {
    match registry().lock() {
        Ok(mut set) => set.remove(&pointer),
        Err(poisoned) => poisoned.into_inner().remove(&pointer),
    }
}

/// Indica si la dirección corresponde a un handle vivo registrado.
fn is_registered(pointer: usize) -> bool {
    match registry().lock() {
        Ok(set) => set.contains(&pointer),
        Err(poisoned) => poisoned.into_inner().contains(&pointer),
    }
}

thread_local! {
    /// Último mensaje de error del hilo actual (contexto para `ruscadb_last_error`).
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// Guarda un mensaje sanitizado (sin NUL interno) como error del hilo actual.
fn set_last_error(message: &str) {
    let sanitized = message.replace('\0', " ");
    let stored = CString::new(sanitized).unwrap_or_default();
    LAST_ERROR.with(|cell| *cell.borrow_mut() = stored);
}

/// Toma una copia del último error del hilo actual.
fn last_error() -> CString {
    LAST_ERROR.with(|cell| cell.borrow().clone())
}

/// Adquiere el mutex del handle recuperándose de un posible envenenamiento.
fn lock_handle(handle: &RuscadbHandle) -> MutexGuard<'_, Database> {
    match handle.database.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Colapsa un `Result<T, T>` (éxito o código de error) en su valor.
fn resolve<T>(result: Result<T, T>) -> T {
    match result {
        Ok(value) | Err(value) => value,
    }
}

/// Ejecuta `operation` sobre un handle validado, o devuelve el código de error.
///
/// Valida en este orden: puntero no nulo, handle registrado y marca `magic`
/// correcta; solo entonces desreferencia el puntero.
fn with_handle<R>(
    handle: *mut RuscadbHandle,
    operation: impl FnOnce(&RuscadbHandle) -> R,
) -> Result<R, c_int> {
    if handle.is_null() {
        set_last_error("handle nulo");
        return Err(RC_NULL_POINTER);
    }
    if !is_registered(handle as usize) {
        set_last_error("handle inválido o ya liberado");
        return Err(RC_INVALID_HANDLE);
    }
    // SAFETY: `handle` es no nulo y está registrado en la tabla global, por lo
    // tanto apunta a un `RuscadbHandle` vivo creado por `ruscadb_open`.
    let reference = unsafe { &*handle };
    if reference.magic != HANDLE_MAGIC {
        set_last_error("handle corrupto (magic inválido)");
        return Err(RC_INVALID_HANDLE);
    }
    Ok(operation(reference))
}

/// Frontera a prueba de panics: captura cualquier panic y lo convierte en código.
fn guard_ffi(operation: impl FnOnce() -> c_int) -> c_int {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(code) => code,
        Err(_) => {
            set_last_error("panic capturado en la frontera FFI");
            RC_PANIC
        }
    }
}

/// Variante de [`guard_ffi`] para funciones que retornan una longitud `usize`.
fn guard_ffi_usize(operation: impl FnOnce() -> usize) -> usize {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or(0)
}

/// Abre (o crea) una base y devuelve un handle opaco.
///
/// Args:
///     data_path: Ruta UTF-8 terminada en NUL del archivo de páginas.
///     pool_capacity: Número de marcos del buffer pool (`>= 1`).
///     out_handle: Destino donde se escribe el handle creado.
///
/// Returns:
///     [`RC_OK`] y el handle en `out_handle`, o un código de error.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_open(
    data_path: *const c_char,
    pool_capacity: u32,
    out_handle: *mut *mut RuscadbHandle,
) -> c_int {
    guard_ffi(|| {
        if data_path.is_null() || out_handle.is_null() {
            set_last_error("ruscadb_open: puntero nulo (data_path/out_handle)");
            return RC_NULL_POINTER;
        }
        // SAFETY: `data_path` es no nulo y el contrato del ABI exige una cadena
        // C válida terminada en NUL.
        let path_text = unsafe { CStr::from_ptr(data_path) };
        let config = DbConfig::new(
            PathBuf::from(path_text.to_string_lossy().into_owned()),
            pool_capacity as usize,
        );
        match Database::open(config) {
            Ok(database) => {
                let handle = Box::new(RuscadbHandle {
                    magic: HANDLE_MAGIC,
                    database: Mutex::new(database),
                });
                let raw = Box::into_raw(handle);
                register(raw as usize);
                // SAFETY: `out_handle` es no nulo (validado arriba) y apunta a
                // una celda `*mut RuscadbHandle` propiedad del llamador.
                unsafe { core::ptr::write(out_handle, raw) };
                RC_OK
            }
            Err(error) => {
                set_last_error(&format!("ruscadb_open: {error}"));
                RC_DOMAIN_ERROR
            }
        }
    })
}

/// Lee una página del handle hacia `out_buf` (exactamente `PAGE_SIZE` bytes).
///
/// Args:
///     handle: Handle vivo devuelto por [`ruscadb_open`].
///     page_id: Identificador de la página a leer.
///     out_buf: Buffer destino de al menos `PAGE_SIZE` bytes.
///     buf_len: Capacidad de `out_buf` en bytes.
///
/// Returns:
///     [`RC_OK`] si copió la página, o un código de error.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_read_page(
    handle: *mut RuscadbHandle,
    page_id: u64,
    out_buf: *mut u8,
    buf_len: usize,
) -> c_int {
    guard_ffi(|| {
        if out_buf.is_null() {
            set_last_error("ruscadb_read_page: out_buf nulo");
            return RC_NULL_POINTER;
        }
        if buf_len < PAGE_SIZE {
            set_last_error("ruscadb_read_page: buf_len menor que PAGE_SIZE");
            return RC_DOMAIN_ERROR;
        }
        resolve(with_handle(handle, |reference| {
            let mut guard = lock_handle(reference);
            match guard.read_page(PageId(page_id)) {
                Ok(page) => {
                    // SAFETY: `out_buf` es no nulo y `buf_len >= PAGE_SIZE`, por
                    // lo que hay espacio para escribir `PAGE_SIZE` bytes.
                    unsafe {
                        core::ptr::copy_nonoverlapping(page.data().as_ptr(), out_buf, PAGE_SIZE);
                    }
                    RC_OK
                }
                Err(error) => {
                    set_last_error(&format!("ruscadb_read_page: {error}"));
                    RC_DOMAIN_ERROR
                }
            }
        }))
    })
}

/// Escribe `PAGE_SIZE` bytes de `in_buf` en la página `page_id`.
///
/// Args:
///     handle: Handle vivo devuelto por [`ruscadb_open`].
///     page_id: Identificador de la página a escribir.
///     in_buf: Buffer origen de al menos `PAGE_SIZE` bytes.
///     buf_len: Longitud de `in_buf` en bytes.
///
/// Returns:
///     [`RC_OK`] si la página quedó sucia en el pool, o un código de error.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_write_page(
    handle: *mut RuscadbHandle,
    page_id: u64,
    in_buf: *const u8,
    buf_len: usize,
) -> c_int {
    guard_ffi(|| {
        if in_buf.is_null() {
            set_last_error("ruscadb_write_page: in_buf nulo");
            return RC_NULL_POINTER;
        }
        if buf_len < PAGE_SIZE {
            set_last_error("ruscadb_write_page: buf_len menor que PAGE_SIZE");
            return RC_DOMAIN_ERROR;
        }
        // SAFETY: `in_buf` es no nulo y `buf_len >= PAGE_SIZE`, por lo que los
        // primeros `PAGE_SIZE` bytes son legibles.
        let input = unsafe { core::slice::from_raw_parts(in_buf, PAGE_SIZE) };
        resolve(with_handle(handle, |reference| {
            let mut page = Page::new(PageId(page_id));
            page.data_mut().copy_from_slice(input);
            let mut guard = lock_handle(reference);
            match guard.write_page(&page) {
                Ok(()) => RC_OK,
                Err(error) => {
                    set_last_error(&format!("ruscadb_write_page: {error}"));
                    RC_DOMAIN_ERROR
                }
            }
        }))
    })
}

/// Confirma los cambios pendientes (WAL-first + fsync).
///
/// Args:
///     handle: Handle vivo devuelto por [`ruscadb_open`].
///
/// Returns:
///     [`RC_OK`] si el commit fue durable, o un código de error.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_commit(handle: *mut RuscadbHandle) -> c_int {
    guard_ffi(|| {
        resolve(with_handle(handle, |reference| {
            let mut guard = lock_handle(reference);
            match guard.commit() {
                Ok(_lsn) => RC_OK,
                Err(error) => {
                    set_last_error(&format!("ruscadb_commit: {error}"));
                    RC_DOMAIN_ERROR
                }
            }
        }))
    })
}

/// Cierra el handle y libera el motor. Un puntero no registrado se rechaza sin
/// desreferenciar, de modo que un doble `close` no provoca doble free.
///
/// Args:
///     handle: Handle vivo devuelto por [`ruscadb_open`].
///
/// Returns:
///     [`RC_OK`] si se liberó, o un código de error.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_close(handle: *mut RuscadbHandle) -> c_int {
    guard_ffi(|| {
        if handle.is_null() {
            set_last_error("ruscadb_close: handle nulo");
            return RC_NULL_POINTER;
        }
        if !unregister(handle as usize) {
            set_last_error("ruscadb_close: handle no registrado (doble free o inválido)");
            return RC_INVALID_HANDLE;
        }
        // SAFETY: `handle` estaba registrado (se acaba de retirar del registro)
        // y fue creado con `Box::into_raw` en `ruscadb_open`, por lo que
        // reconstruir el `Box` lo libera una única vez.
        let boxed = unsafe { Box::from_raw(handle) };
        let RuscadbHandle { database, .. } = *boxed;
        let database = match database.into_inner() {
            Ok(database) => database,
            Err(poisoned) => poisoned.into_inner(),
        };
        match database.close() {
            Ok(()) => RC_OK,
            Err(error) => {
                set_last_error(&format!("ruscadb_close: {error}"));
                RC_DOMAIN_ERROR
            }
        }
    })
}

/// Copia el último mensaje de error del hilo a `out_buf`, truncándolo a
/// `buf_len` bytes y garantizando terminación en NUL si hay espacio.
///
/// Args:
///     out_buf: Buffer destino, o nulo para solo consultar la longitud.
///     buf_len: Capacidad de `out_buf` en bytes.
///
/// Returns:
///     La longitud del mensaje (sin NUL). Si `out_buf` es nulo, solo la longitud.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_last_error(out_buf: *mut c_char, buf_len: usize) -> usize {
    guard_ffi_usize(|| {
        let message = last_error();
        let bytes = message.as_bytes();
        if out_buf.is_null() || buf_len == 0 {
            return bytes.len();
        }
        let capacity = buf_len.saturating_sub(1);
        let copy = bytes.len().min(capacity);
        // SAFETY: `out_buf` es no nulo y `copy <= buf_len - 1`, por lo que
        // escribir `copy` bytes cae dentro del buffer del llamador.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf.cast::<u8>(), copy) };
        // SAFETY: queda al menos el byte para el NUL (`copy < buf_len`), luego
        // el offset `copy` está dentro del buffer.
        let terminator = unsafe { out_buf.add(copy) };
        // SAFETY: `terminator` apunta dentro de `out_buf` (ver arriba) y es
        // válido para escribir un byte.
        unsafe { core::ptr::write(terminator, 0) };
        bytes.len()
    })
}

/// Serializa filas a JSON y lo copia a `out_buf` respetando `buf_len`.
///
/// Esquema JSON (SPEC-0026, FR-0026-02): array de filas; cada fila es un
/// objeto `columna -> valor`, y cada valor es la forma externa de
/// `ScalarValue` (p. ej. `{"Int":1}`, `{"Text":"x"}`, `"Null"`).
///
/// Semántica del buffer: se escribe el JSON seguido de un byte NUL. Si
/// `buf_len < json.len() + 1` no se escribe nada y se devuelve
/// [`RC_BUFFER_TOO_SMALL`] dejando el tamaño requerido en `ruscadb_last_error`.
fn write_json(out_buf: *mut c_char, buf_len: usize, json: &[u8]) -> c_int {
    let required = json.len() + 1;
    if buf_len < required {
        set_last_error(&format!(
            "ruscadb_execute: buffer insuficiente: se requieren {required} bytes"
        ));
        return RC_BUFFER_TOO_SMALL;
    }
    // SAFETY: `out_buf` es no nulo y `buf_len >= json.len() + 1`, luego los
    // `json.len()` bytes de origen caben en el destino.
    unsafe { core::ptr::copy_nonoverlapping(json.as_ptr(), out_buf.cast::<u8>(), json.len()) };
    // SAFETY: tras copiar `json.len()` bytes queda al menos el byte del NUL
    // (`json.len() < buf_len`), por lo que el offset está dentro del buffer.
    let terminator = unsafe { out_buf.add(json.len()) };
    // SAFETY: `terminator` apunta dentro de `out_buf` y es válido para escribir
    // un byte.
    unsafe { core::ptr::write(terminator, 0) };
    RC_OK
}

/// Ejecuta `sql` sobre un handle validado y copia el JSON resultante.
fn execute_into(
    reference: &RuscadbHandle,
    sql: *const c_char,
    out_buf: *mut c_char,
    buf_len: usize,
) -> c_int {
    // SAFETY: el contrato del ABI exige que `sql` sea una cadena C válida y no
    // nula (el llamador ya validó que no es nula).
    let query = unsafe { CStr::from_ptr(sql) }.to_string_lossy();
    let mut guard = lock_handle(reference);
    let rows = match guard.execute(query.as_ref()) {
        Ok(rows) => rows,
        Err(error) => {
            set_last_error(&format!("ruscadb_execute: {error}"));
            return RC_DOMAIN_ERROR;
        }
    };
    match serde_json::to_vec(&rows) {
        Ok(json) => write_json(out_buf, buf_len, &json),
        Err(error) => {
            set_last_error(&format!("ruscadb_execute: {error}"));
            RC_DOMAIN_ERROR
        }
    }
}

/// Longitud en bytes (sin NUL) del JSON de `sql`, o `0` si falla.
fn execute_len_into(reference: &RuscadbHandle, sql: *const c_char) -> usize {
    // SAFETY: el contrato del ABI exige que `sql` sea una cadena C válida y no
    // nula (el llamador ya validó que no es nula).
    let query = unsafe { CStr::from_ptr(sql) }.to_string_lossy();
    let mut guard = lock_handle(reference);
    match guard.execute(query.as_ref()) {
        Ok(rows) => match serde_json::to_vec(&rows) {
            Ok(json) => json.len(),
            Err(error) => {
                set_last_error(&format!("ruscadb_execute_len: {error}"));
                0
            }
        },
        Err(error) => {
            set_last_error(&format!("ruscadb_execute_len: {error}"));
            0
        }
    }
}

/// Devuelve la longitud en bytes del JSON que produciría `sql` (sin el NUL
/// terminador), o `0` si la consulta o el handle fallan (el detalle queda en
/// `ruscadb_last_error`). Permite dimensionar `out_buf` antes de llamar a
/// [`ruscadb_execute`].
///
/// Args:
///     handle: Handle vivo devuelto por [`ruscadb_open`].
///     sql: Consulta RQL en UTF-8 terminada en NUL.
///
/// Returns:
///     Bytes requeridos para el JSON (sin NUL), o `0` si hay error.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_execute_len(handle: *mut RuscadbHandle, sql: *const c_char) -> usize {
    guard_ffi_usize(|| {
        if sql.is_null() {
            set_last_error("ruscadb_execute_len: sql nulo");
            return 0;
        }
        with_handle(handle, |reference| execute_len_into(reference, sql)).unwrap_or_default()
    })
}

/// Ejecuta una consulta RQL y copia su JSON a `out_buf`.
///
/// Esquema JSON (FR-0026-02): array de filas; cada fila es un objeto
/// `columna -> valor`, y cada valor es la forma externa de `ScalarValue`
/// (p. ej. `{"Int":1}`, `{"Text":"x"}`, `"Null"`). El buffer recibe el JSON
/// seguido de un byte NUL.
///
/// Semántica del buffer: si `buf_len < json.len() + 1` no se escribe nada y
/// se devuelve [`RC_BUFFER_TOO_SMALL`] dejando en `ruscadb_last_error`
/// cuántos bytes se requieren.
///
/// Args:
///     handle: Handle vivo devuelto por [`ruscadb_open`].
///     sql: Consulta RQL en UTF-8 terminada en NUL.
///     out_buf: Buffer destino del JSON.
///     buf_len: Capacidad de `out_buf` en bytes.
///
/// Returns:
///     [`RC_OK`] si copió el JSON, o un código de error.
#[unsafe(no_mangle)]
pub extern "C" fn ruscadb_execute(
    handle: *mut RuscadbHandle,
    sql: *const c_char,
    out_buf: *mut c_char,
    buf_len: usize,
) -> c_int {
    guard_ffi(|| {
        if sql.is_null() {
            set_last_error("ruscadb_execute: sql nulo");
            return RC_NULL_POINTER;
        }
        if out_buf.is_null() {
            set_last_error("ruscadb_execute: out_buf nulo");
            return RC_NULL_POINTER;
        }
        resolve(with_handle(handle, |reference| {
            execute_into(reference, sql, out_buf, buf_len)
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ruscadb::{ColumnDef, ColumnType, ScalarMap, ScalarValue};
    use std::path::Path;

    /// Construye una ruta C terminada en NUL a partir de una ruta UTF-8.
    fn c_path(path: &Path) -> CString {
        CString::new(path.to_str().expect("ruta utf8")).expect("sin NUL en la ruta")
    }

    /// Rellena `t(a INT, b TEXT)` con tres filas usando el motor del handle.
    fn seed_table(handle: *mut RuscadbHandle) {
        // SAFETY: `handle` proviene de `open_handle` y sigue registrado/vivo
        // durante el test, por lo que apunta a un `RuscadbHandle` válido.
        let reference = unsafe { &*handle };
        let mut guard = lock_handle(reference);
        guard
            .create_table(
                "t",
                vec![
                    ColumnDef {
                        name: "a".to_string(),
                        col_type: ColumnType::Int,
                    },
                    ColumnDef {
                        name: "b".to_string(),
                        col_type: ColumnType::Text,
                    },
                ],
            )
            .expect("create_table");
        for (value, label) in [(1_i64, "x"), (2, "x"), (3, "y")] {
            let mut scalars = ScalarMap::new();
            scalars.insert("a".to_string(), ScalarValue::Int(value));
            scalars.insert("b".to_string(), ScalarValue::Text(label.to_string()));
            guard.insert("t", scalars).expect("insert");
        }
    }

    /// AC-0026-01 — una consulta válida devuelve `RC_OK` y un JSON con las filas.
    #[test]
    // @spec AC-0026-01
    fn test_ac_0026_01_execute_returns_json() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let handle = open_handle(&path, 8);
        seed_table(handle);

        let sql = CString::new("SELECT * FROM t").expect("sql");
        let mut buffer = [0 as c_char; 4096];
        let code = ruscadb_execute(handle, sql.as_ptr(), buffer.as_mut_ptr(), buffer.len());
        assert_eq!(code, RC_OK);

        let raw: Vec<u8> = buffer.iter().map(|byte| *byte as u8).collect();
        let text = CStr::from_bytes_until_nul(&raw).expect("NUL");
        let parsed: serde_json::Value = serde_json::from_slice(text.to_bytes()).expect("json");
        let rows = parsed.as_array().expect("array de filas");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get("a"), Some(&serde_json::json!({"Int": 1})));
        assert_eq!(rows[0].get("b"), Some(&serde_json::json!({"Text": "x"})));
        assert_eq!(rows[2].get("a"), Some(&serde_json::json!({"Int": 3})));
        assert_eq!(ruscadb_close(handle), RC_OK);
    }

    /// AC-0026-02 — una query inválida es `RC_DOMAIN_ERROR` con `last_error`.
    #[test]
    // @spec AC-0026-02
    fn test_ac_0026_02_invalid_query_is_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let handle = open_handle(&path, 8);
        seed_table(handle);
        let mut buffer = [0 as c_char; 512];

        let missing = CString::new("SELECT * FROM ausente").expect("sql");
        assert_eq!(
            ruscadb_execute(handle, missing.as_ptr(), buffer.as_mut_ptr(), buffer.len()),
            RC_DOMAIN_ERROR
        );
        assert!(last_error().to_string_lossy().contains("ausente"));

        let malformed = CString::new("no es rql").expect("sql");
        assert_eq!(
            ruscadb_execute(
                handle,
                malformed.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len()
            ),
            RC_DOMAIN_ERROR
        );
        assert!(!last_error().to_string_lossy().is_empty());
        assert_eq!(ruscadb_close(handle), RC_OK);
    }

    /// AC-0026-03 — un buffer pequeño no desborda y reporta el tamaño requerido.
    #[test]
    // @spec AC-0026-03
    fn test_ac_0026_03_small_buffer_reports_size() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let handle = open_handle(&path, 8);
        seed_table(handle);

        let sql = CString::new("SELECT * FROM t").expect("sql");
        let required = ruscadb_execute_len(handle, sql.as_ptr());
        assert!(required > 0, "el tamaño requerido debe ser positivo");

        // BVA: `buf_len` = 0, 1, required-1 y required (falta el byte NUL).
        for len in [0usize, 1, required.saturating_sub(1), required] {
            let mut buffer = vec![0x7Fu8; len + 4];
            let code = ruscadb_execute(handle, sql.as_ptr(), buffer.as_mut_ptr().cast(), len);
            assert_eq!(code, RC_BUFFER_TOO_SMALL, "buf_len={len}");
            assert!(
                buffer[len..].iter().all(|byte| *byte == 0x7F),
                "buf_len={len}: se escribió fuera del buffer"
            );
        }
        let message = last_error().to_string_lossy().into_owned();
        assert!(
            message.contains(&(required + 1).to_string()),
            "el error debe comunicar el tamaño requerido: {message}"
        );

        // Capacidad exacta (JSON + NUL) sí cabe y termina en NUL.
        let mut exact = vec![0u8; required + 1];
        let code = ruscadb_execute(handle, sql.as_ptr(), exact.as_mut_ptr().cast(), exact.len());
        assert_eq!(code, RC_OK);
        assert_eq!(exact[required], 0);
        assert_eq!(ruscadb_close(handle), RC_OK);
    }

    /// AC-0026-04 — handle nulo/inválido/liberado y punteros nulos dan error.
    #[test]
    // @spec AC-0026-04
    fn test_ac_0026_04_invalid_handle_is_error() {
        let sql = CString::new("SELECT * FROM t").expect("sql");
        let mut buffer = [0 as c_char; 64];

        assert_eq!(
            ruscadb_execute(
                std::ptr::null_mut(),
                sql.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len()
            ),
            RC_NULL_POINTER
        );

        let bogus = std::ptr::dangling_mut::<RuscadbHandle>();
        assert_eq!(
            ruscadb_execute(bogus, sql.as_ptr(), buffer.as_mut_ptr(), buffer.len()),
            RC_INVALID_HANDLE
        );
        assert_eq!(ruscadb_execute_len(bogus, sql.as_ptr()), 0);

        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let handle = open_handle(&path, 8);
        assert_eq!(
            ruscadb_execute(handle, std::ptr::null(), buffer.as_mut_ptr(), buffer.len()),
            RC_NULL_POINTER
        );
        assert_eq!(
            ruscadb_execute(handle, sql.as_ptr(), std::ptr::null_mut(), 0),
            RC_NULL_POINTER
        );
        assert_eq!(ruscadb_execute_len(handle, std::ptr::null()), 0);

        assert_eq!(ruscadb_close(handle), RC_OK);
        assert_eq!(
            ruscadb_execute(handle, sql.as_ptr(), buffer.as_mut_ptr(), buffer.len()),
            RC_INVALID_HANDLE
        );
        assert_eq!(ruscadb_execute_len(handle, sql.as_ptr()), 0);
    }

    /// AC-0026-05 — los wrappers Python y Node exponen `execute`.
    #[test]
    // @spec AC-0026-05
    fn test_ac_0026_05_wrappers_expose_execute() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        let python = std::fs::read_to_string(root.join("bindings/python/ruscadb.py"))
            .expect("wrapper Python");
        assert!(python.contains("def execute("), "Python no expone execute");
        assert!(
            python.contains("ruscadb_execute"),
            "Python no enlaza ruscadb_execute"
        );

        let node =
            std::fs::read_to_string(root.join("bindings/node/ruscadb.js")).expect("wrapper Node");
        assert!(
            node.contains("execute(handle, sql)"),
            "Node no expone execute"
        );
        assert!(
            node.contains("ruscadb_execute"),
            "Node no enlaza ruscadb_execute"
        );
    }

    /// Abre un handle válido sobre `path` y falla el test si no lo consigue.
    fn open_handle(path: &Path, capacity: u32) -> *mut RuscadbHandle {
        let path_c = c_path(path);
        let mut handle: *mut RuscadbHandle = std::ptr::null_mut();
        let code = ruscadb_open(path_c.as_ptr(), capacity, &mut handle);
        assert_eq!(code, RC_OK);
        assert!(!handle.is_null());
        handle
    }

    /// AC-0010-01 — abrir y cerrar deja un handle válido sin fugas.
    #[test]
    // @spec AC-0010-01
    fn test_ac_0010_01_open_and_close() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let handle = open_handle(&path, 8);
        assert!(is_registered(handle as usize));
        assert_eq!(ruscadb_close(handle), RC_OK);
        assert!(!is_registered(handle as usize));
    }

    /// AC-0010-02 — escribir, commit y reabrir conserva el contenido.
    #[test]
    // @spec AC-0010-02
    fn test_ac_0010_02_write_commit_reopen() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");

        let handle = open_handle(&path, 8);
        let mut page = [0u8; PAGE_SIZE];
        page[0] = 42;
        page[PAGE_SIZE - 1] = 7;
        assert_eq!(
            ruscadb_write_page(handle, 3, page.as_ptr(), page.len()),
            RC_OK
        );
        assert_eq!(ruscadb_commit(handle), RC_OK);
        assert_eq!(ruscadb_close(handle), RC_OK);

        let reopened = open_handle(&path, 8);
        let mut out = [0u8; PAGE_SIZE];
        assert_eq!(
            ruscadb_read_page(reopened, 3, out.as_mut_ptr(), out.len()),
            RC_OK
        );
        assert_eq!(out[0], 42);
        assert_eq!(out[PAGE_SIZE - 1], 7);
        assert_eq!(ruscadb_close(reopened), RC_OK);
    }

    /// AC-0010-03 — un handle inválido o liberado devuelve error sin desreferenciar.
    #[test]
    // @spec AC-0010-03
    fn test_ac_0010_03_invalid_handle_is_error() {
        let bogus = std::ptr::dangling_mut::<RuscadbHandle>();
        let mut out = [0u8; PAGE_SIZE];
        let marker = [0u8; PAGE_SIZE];
        assert_eq!(
            ruscadb_read_page(bogus, 0, out.as_mut_ptr(), out.len()),
            RC_INVALID_HANDLE
        );
        assert_eq!(
            ruscadb_write_page(bogus, 0, marker.as_ptr(), marker.len()),
            RC_INVALID_HANDLE
        );
        assert_eq!(ruscadb_commit(bogus), RC_INVALID_HANDLE);
        assert_eq!(ruscadb_close(bogus), RC_INVALID_HANDLE);

        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let handle = open_handle(&path, 4);
        assert_eq!(ruscadb_close(handle), RC_OK);
        // use-after-free: el puntero ya no está registrado.
        assert_eq!(
            ruscadb_read_page(handle, 0, out.as_mut_ptr(), out.len()),
            RC_INVALID_HANDLE
        );
        // doble free: el segundo close tampoco libera.
        assert_eq!(ruscadb_close(handle), RC_INVALID_HANDLE);
    }

    /// AC-0010-04 — punteros nulos y longitudes inconsistentes son error.
    #[test]
    // @spec AC-0010-04
    fn test_ac_0010_04_null_pointer_is_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let path_c = c_path(&path);
        let mut handle: *mut RuscadbHandle = std::ptr::null_mut();

        assert_eq!(
            ruscadb_open(std::ptr::null(), 4, &mut handle),
            RC_NULL_POINTER
        );
        assert_eq!(
            ruscadb_open(path_c.as_ptr(), 4, std::ptr::null_mut()),
            RC_NULL_POINTER
        );

        let handle = open_handle(&path, 4);
        let mut out = [0u8; PAGE_SIZE];
        assert_eq!(
            ruscadb_read_page(handle, 0, std::ptr::null_mut(), PAGE_SIZE),
            RC_NULL_POINTER
        );
        assert_eq!(
            ruscadb_write_page(handle, 0, std::ptr::null(), PAGE_SIZE),
            RC_NULL_POINTER
        );

        // Página existente: la longitud inconsistente es lo que decide el error.
        assert_eq!(
            ruscadb_write_page(handle, 0, out.as_ptr(), PAGE_SIZE),
            RC_OK
        );
        assert_eq!(
            ruscadb_read_page(handle, 0, out.as_mut_ptr(), PAGE_SIZE - 1),
            RC_DOMAIN_ERROR
        );
        assert_eq!(
            ruscadb_write_page(handle, 0, out.as_ptr(), 1),
            RC_DOMAIN_ERROR
        );
        assert_eq!(ruscadb_close(handle), RC_OK);
    }

    /// Un `pool_capacity` inválido se traduce en error de dominio.
    #[test]
    fn test_open_with_zero_capacity_is_domain_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let path_c = c_path(&path);
        let mut handle: *mut RuscadbHandle = std::ptr::null_mut();
        assert_eq!(
            ruscadb_open(path_c.as_ptr(), 0, &mut handle),
            RC_DOMAIN_ERROR
        );
        assert!(handle.is_null());
    }

    /// Leer una página fuera de rango del archivo es error de dominio.
    #[test]
    fn test_read_out_of_range_page_is_domain_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let handle = open_handle(&path, 4);
        let mut out = [0u8; PAGE_SIZE];
        assert_eq!(
            ruscadb_read_page(handle, 99, out.as_mut_ptr(), out.len()),
            RC_DOMAIN_ERROR
        );
        assert_eq!(ruscadb_close(handle), RC_OK);
    }

    /// `last_error` copia el mensaje y devuelve su longitud; `out_buf` nulo solo
    /// devuelve la longitud.
    #[test]
    fn test_last_error_reports_and_truncates() {
        let bogus = std::ptr::dangling_mut::<RuscadbHandle>();
        assert_eq!(ruscadb_commit(bogus), RC_INVALID_HANDLE);
        let length = ruscadb_last_error(std::ptr::null_mut(), 0);
        assert!(length > 0);

        let mut buffer = [0 as c_char; 64];
        let copied = ruscadb_last_error(buffer.as_mut_ptr(), buffer.len());
        assert_eq!(copied, length);
        let bytes: Vec<u8> = buffer.iter().map(|byte| *byte as u8).collect();
        let text = std::ffi::CStr::from_bytes_until_nul(&bytes).expect("NUL terminador");
        assert!(!text.to_bytes().is_empty());

        // Buffer diminuto: trunca y sigue terminando en NUL.
        let mut tiny = [0 as c_char; 2];
        ruscadb_last_error(tiny.as_mut_ptr(), tiny.len());
        assert_eq!(tiny[1], 0);

        // `buf_len == 0` con buffer no nulo: solo consulta, sin escribir.
        let mut sentinel = [0x7Fi8; 8];
        assert_eq!(ruscadb_last_error(sentinel.as_mut_ptr(), 0), length);
        assert_eq!(sentinel[0], 0x7F);
    }

    /// Un handle con `magic` incorrecto se rechaza sin usar el motor.
    #[test]
    fn test_handle_with_wrong_magic_is_invalid() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("db.data");
        let database = Database::open(DbConfig::new(&path, 4)).expect("open");
        let mut fake = Box::new(RuscadbHandle {
            magic: 0,
            database: Mutex::new(database),
        });
        let raw: *mut RuscadbHandle = &mut *fake;
        assert!(register(raw as usize));
        // Re-registrar la misma dirección no crea una entrada nueva.
        assert!(!register(raw as usize));

        let mut out = [0u8; PAGE_SIZE];
        assert_eq!(
            ruscadb_read_page(raw, 0, out.as_mut_ptr(), out.len()),
            RC_INVALID_HANDLE
        );

        assert!(unregister(raw as usize));
        drop(fake);
    }
}
