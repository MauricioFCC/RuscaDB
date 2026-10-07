'use strict';

/**
 * Wrapper Node.js (koffi) sobre el C-ABI estable de RuscaDB (SPEC-0010).
 *
 * Carga la biblioteca dinamica `ruscadb_ffi` compilada con
 * `cargo build -p ruscadb-ffi` y expone una API delgada: `open`, `read_page`,
 * `write_page`, `commit`, `close`, `execute` y `lastError`. No reimplementa
 * logica: todo el trabajo lo hace el motor Rust a traves del ABI.
 *
 * Uso:
 *   const ruscadb = require('./ruscadb');
 *   const handle = ruscadb.open('mi.db', 8);
 *   ruscadb.writePage(handle, 0, Buffer.alloc(ruscadb.PAGE_SIZE));
 *   ruscadb.commit(handle);
 *   ruscadb.close(handle);
 *
 * La ruta de la biblioteca se toma de `RUSCADB_FFI_LIB` o de
 * `target/{debug,release}` (ver `resolveLibrary`).
 */

const fs = require('node:fs');
const path = require('node:path');
const koffi = require('koffi');

/** Tamano de pagina de RuscaDB (4 KiB), espejo del C-ABI. */
const PAGE_SIZE = 4096;

/** Codigos de retorno del C-ABI. */
const RC_OK = 0;
const RC_NULL_POINTER = 1;
const RC_INVALID_HANDLE = 2;
const RC_DOMAIN_ERROR = 3;
const RC_PANIC = 4;
const RC_BUFFER_TOO_SMALL = 5;

/** Error devuelto por el C-ABI de RuscaDB. */
class RuscadbError extends Error {
  /**
   * @param {number} code Codigo de retorno del ABI.
   * @param {string} message Mensaje legible obtenido del ABI.
   */
  constructor(code, message) {
    super(`ruscadb: codigo ${code}: ${message}`);
    this.name = 'RuscadbError';
    this.code = code;
    this.message = message;
  }
}

/**
 * Nombres candidatos del artefacto dinamico segun la plataforma.
 *
 * @returns {string[]} Nombres de archivo probables.
 */
function libraryNames() {
  if (process.platform === 'win32') {
    return ['ruscadb_ffi.dll', 'ruscadb.dll'];
  }
  if (process.platform === 'darwin') {
    return ['libruscadb_ffi.dylib', 'libruscadb.dylib'];
  }
  return ['libruscadb_ffi.so', 'libruscadb.so'];
}

/**
 * Encuentra la biblioteca del C-ABI.
 *
 * @param {string} [explicitPath] Ruta explicita indicada por el llamador.
 * @returns {string} Ruta de la biblioteca encontrada.
 * @throws {Error} Si no se encuentra ninguna biblioteca.
 */
function resolveLibrary(explicitPath) {
  const candidates = [];
  if (explicitPath) {
    candidates.push(explicitPath);
  }
  if (process.env.RUSCADB_FFI_LIB) {
    candidates.push(process.env.RUSCADB_FFI_LIB);
  }
  for (const profile of ['debug', 'release']) {
    for (const name of libraryNames()) {
      candidates.push(path.join('target', profile, name));
    }
  }
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    'no se encontro la biblioteca del C-ABI; compila con ' +
      '`cargo build -p ruscadb-ffi` o define RUSCADB_FFI_LIB ' +
      `(probadas: ${candidates.join(', ')})`,
  );
}

/**
 * Carga la biblioteca y devuelve los simbolos del ABI.
 *
 * @param {string} [explicitPath] Ruta explicita de la biblioteca.
 * @returns {object} Simbolos crudos enlazados por koffi.
 */
function loadSymbols(explicitPath) {
  const library = koffi.load(resolveLibrary(explicitPath));
  return {
    open: library.func('ruscadb_open', 'int', [
      'str',
      'uint32',
      koffi.out(koffi.pointer('void')),
    ]),
    readPage: library.func('ruscadb_read_page', 'int', [
      koffi.pointer('void'),
      'uint64',
      'uint8 *',
      'size_t',
    ]),
    writePage: library.func('ruscadb_write_page', 'int', [
      koffi.pointer('void'),
      'uint64',
      'uint8 *',
      'size_t',
    ]),
    commit: library.func('ruscadb_commit', 'int', [koffi.pointer('void')]),
    close: library.func('ruscadb_close', 'int', [koffi.pointer('void')]),
    executeLen: library.func('ruscadb_execute_len', 'size_t', [
      koffi.pointer('void'),
      'str',
    ]),
    execute: library.func('ruscadb_execute', 'int', [
      koffi.pointer('void'),
      'str',
      'char *',
      'size_t',
    ]),
    lastError: library.func('ruscadb_last_error', 'size_t', ['char *', 'size_t']),
  };
}

/**
 * Construye la API del wrapper sobre los simbolos crudos.
 *
 * @param {object} symbols Simbolos devueltos por `loadSymbols`.
 * @returns {object} API publica del wrapper.
 */
function buildApi(symbols) {
  const lastError = () => {
    const length = Number(symbols.lastError(null, 0));
    if (length <= 0) {
      return '';
    }
    const buffer = Buffer.alloc(length + 1);
    symbols.lastError(buffer, buffer.length);
    return buffer.toString('utf8', 0, length);
  };

  const check = (code) => {
    if (code !== RC_OK) {
      throw new RuscadbError(code, lastError());
    }
  };

  return {
    PAGE_SIZE,
    RC_OK,
    RC_NULL_POINTER,
    RC_INVALID_HANDLE,
    RC_DOMAIN_ERROR,
    RC_PANIC,
    RC_BUFFER_TOO_SMALL,
    RuscadbError,
    lastError,

    /**
     * Abre (o crea) una base RuscaDB.
     *
     * @param {string} dataPath Ruta del archivo de paginas.
     * @param {number} [poolCapacity=8] Numero de marcos del buffer pool.
     * @returns {object} Handle opaco del ABI.
     */
    open(dataPath, poolCapacity = 8) {
      const out = [null];
      check(symbols.open(dataPath, poolCapacity >>> 0, out));
      return out[0];
    },

    /**
     * Lee una pagina completa (`PAGE_SIZE` bytes).
     *
     * @param {object} handle Handle devuelto por `open`.
     * @param {number|bigint} pageId Identificador de la pagina.
     * @returns {Buffer} Contenido de la pagina.
     */
    readPage(handle, pageId) {
      const buffer = Buffer.alloc(PAGE_SIZE);
      check(symbols.readPage(handle, BigInt(pageId), buffer, PAGE_SIZE));
      return buffer;
    },

    /**
     * Escribe una pagina completa (`PAGE_SIZE` bytes).
     *
     * @param {object} handle Handle devuelto por `open`.
     * @param {number|bigint} pageId Identificador de la pagina.
     * @param {Buffer} data Contenido de exactamente `PAGE_SIZE` bytes.
     * @throws {RangeError} Si `data` no mide `PAGE_SIZE` bytes.
     */
    writePage(handle, pageId, data) {
      if (data.length !== PAGE_SIZE) {
        throw new RangeError(
          `data debe medir PAGE_SIZE=${PAGE_SIZE} bytes, no ${data.length}`,
        );
      }
      check(symbols.writePage(handle, BigInt(pageId), data, PAGE_SIZE));
    },

    /**
     * Confirma los cambios pendientes (WAL-first + fsync).
     *
     * @param {object} handle Handle devuelto por `open`.
     */
    commit(handle) {
      check(symbols.commit(handle));
    },

    /**
     * Cierra el handle y libera el motor.
     *
     * @param {object} handle Handle devuelto por `open`.
     */
    close(handle) {
      check(symbols.close(handle));
    },

    /**
     * Ejecuta una consulta RQL y devuelve el JSON de las filas.
     *
     * Esquema JSON: array de filas; cada fila es un objeto `columna -> valor`
     * y cada valor usa la forma externa de `ScalarValue` (por ejemplo
     * `{"Int": 1}` o `{"Text": "x"}`).
     *
     * @param {object} handle Handle devuelto por `open`.
     * @param {string} sql Consulta RQL.
     * @returns {string} JSON serializado de las filas.
     */
    execute(handle, sql) {
      const required = Number(symbols.executeLen(handle, sql));
      if (required <= 0) {
        throw new RuscadbError(RC_DOMAIN_ERROR, lastError());
      }
      const buffer = Buffer.alloc(required + 1);
      check(symbols.execute(handle, sql, buffer, buffer.length));
      return buffer.toString('utf8', 0, required);
    },
  };
}

let defaultApi = null;

/**
 * Devuelve la API del wrapper, cargando la biblioteca en el primer uso.
 *
 * @param {string} [explicitPath] Ruta explicita de la biblioteca.
 * @returns {object} API publica del wrapper.
 */
function load(explicitPath) {
  if (explicitPath || defaultApi === null) {
    const api = buildApi(loadSymbols(explicitPath));
    if (!explicitPath) {
      defaultApi = api;
    }
    return api;
  }
  return defaultApi;
}

const api = {
  PAGE_SIZE,
  RC_OK,
  RC_NULL_POINTER,
  RC_INVALID_HANDLE,
  RC_DOMAIN_ERROR,
  RC_PANIC,
  RC_BUFFER_TOO_SMALL,
  RuscadbError,
  load,
  lastError: () => load().lastError(),
  open: (dataPath, poolCapacity) => load().open(dataPath, poolCapacity),
  readPage: (handle, pageId) => load().readPage(handle, pageId),
  writePage: (handle, pageId, data) => load().writePage(handle, pageId, data),
  commit: (handle) => load().commit(handle),
  close: (handle) => load().close(handle),
  execute: (handle, sql) => load().execute(handle, sql),
};

module.exports = api;
