"""Wrapper Python (ctypes) sobre el C-ABI estable de RuscaDB.

Carga la biblioteca dinamica `ruscadb_ffi` compilada con
``cargo build -p ruscadb-ffi`` y expone una API delgada: ``open``,
``read_page``, ``write_page``, ``commit``, ``close``, ``execute`` y
``last_error``. No reimplementa logica: todo el trabajo lo hace el motor Rust
a traves del ABI.

Uso:
    import ruscadb

    handle = ruscadb.open("mi.db", pool_capacity=8)
    ruscadb.write_page(handle, 0, bytes(ruscadb.PAGE_SIZE))
    ruscadb.commit(handle)
    ruscadb.close(handle)

La ruta de la biblioteca se toma de ``RUSCADB_FFI_LIB`` o de
``target/{debug,release}`` (ver :func:`load_library`).
"""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes import POINTER, c_char_p, c_int, c_size_t, c_uint32, c_uint64, c_void_p
from pathlib import Path

#: Tamano de pagina de RuscaDB (4 KiB), espejo del C-ABI.
PAGE_SIZE = 4096

#: Codigos de retorno del C-ABI.
RC_OK = 0
RC_NULL_POINTER = 1
RC_INVALID_HANDLE = 2
RC_DOMAIN_ERROR = 3
RC_PANIC = 4
RC_BUFFER_TOO_SMALL = 5

_U8P = POINTER(ctypes.c_uint8)
_HANDLE_P = POINTER(c_void_p)

# Biblioteca cargada de forma perezosa (una sola vez por proceso).
_LIB: ctypes.CDLL | None = None


class RuscadbError(RuntimeError):
    """Error devuelto por el C-ABI de RuscaDB."""

    def __init__(self, code: int, message: str) -> None:
        """Inicializa el error con el codigo y el mensaje del ABI.

        Args:
            code: Codigo de retorno devuelto por el ABI.
            message: Mensaje legible obtenido con ``ruscadb_last_error``.
        """
        super().__init__(f"ruscadb: codigo {code}: {message}")
        self.code = code
        self.message = message


def _library_names() -> list[str]:
    """Nombres candidatos del artefacto dinamico segun la plataforma."""
    if sys.platform.startswith("win"):
        return ["ruscadb_ffi.dll", "ruscadb.dll"]
    if sys.platform == "darwin":
        return ["libruscadb_ffi.dylib", "libruscadb.dylib"]
    return ["libruscadb_ffi.so", "libruscadb.so"]


def _library_candidates(path: str | os.PathLike[str] | None) -> list[Path]:
    """Construye las rutas probables de la biblioteca en orden de prioridad.

    Args:
        path: Ruta explicita indicada por el llamador, o None.

    Returns:
        Lista de rutas candidatas a probar.
    """
    candidates: list[Path] = []
    if path is not None:
        candidates.append(Path(path))
    explicit = os.environ.get("RUSCADB_FFI_LIB")
    if explicit:
        candidates.append(Path(explicit))
    for profile in ("debug", "release"):
        for name in _library_names():
            candidates.append(Path("target") / profile / name)
    return candidates


def _locate_library(path: str | os.PathLike[str] | None) -> Path:
    """Encuentra la primera biblioteca existente.

    Args:
        path: Ruta explicita indicada por el llamador, o None.

    Returns:
        La ruta de la biblioteca encontrada.

    Raises:
        FileNotFoundError: si ninguna candidata existe.
    """
    candidates = _library_candidates(path)
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    searched = ", ".join(str(candidate) for candidate in candidates)
    raise FileNotFoundError(
        "no se encontro la biblioteca del C-ABI; compila con "
        f"`cargo build -p ruscadb-ffi` o define RUSCADB_FFI_LIB (probadas: {searched})"
    )


def load_library(path: str | os.PathLike[str] | None = None) -> ctypes.CDLL:
    """Carga la biblioteca del C-ABI y enlaza sus firmas.

    Args:
        path: Ruta explicita de la biblioteca; si es None se busca en
            ``RUSCADB_FFI_LIB`` y en ``target/{debug,release}``.

    Returns:
        La biblioteca cargada con ``argtypes``/``restype`` configurados.

    Raises:
        FileNotFoundError: si no se encuentra la biblioteca.
    """
    library = ctypes.CDLL(str(_locate_library(path)))

    library.ruscadb_open.argtypes = [c_char_p, c_uint32, _HANDLE_P]
    library.ruscadb_open.restype = c_int
    library.ruscadb_read_page.argtypes = [c_void_p, c_uint64, _U8P, c_size_t]
    library.ruscadb_read_page.restype = c_int
    library.ruscadb_write_page.argtypes = [c_void_p, c_uint64, _U8P, c_size_t]
    library.ruscadb_write_page.restype = c_int
    library.ruscadb_commit.argtypes = [c_void_p]
    library.ruscadb_commit.restype = c_int
    library.ruscadb_close.argtypes = [c_void_p]
    library.ruscadb_close.restype = c_int
    library.ruscadb_execute_len.argtypes = [c_void_p, c_char_p]
    library.ruscadb_execute_len.restype = c_size_t
    library.ruscadb_execute.argtypes = [c_void_p, c_char_p, c_char_p, c_size_t]
    library.ruscadb_execute.restype = c_int
    library.ruscadb_last_error.argtypes = [c_char_p, c_size_t]
    library.ruscadb_last_error.restype = c_size_t
    return library


def _lib() -> ctypes.CDLL:
    """Devuelve la biblioteca global, cargandola en el primer uso."""
    global _LIB
    if _LIB is None:
        _LIB = load_library()
    return _LIB


def last_error() -> str:
    """Devuelve el ultimo mensaje de error del hilo actual.

    Returns:
        El mensaje en UTF-8 (vacio si no hay error registrado).
    """
    library = _lib()
    length = int(library.ruscadb_last_error(None, 0))
    if length <= 0:
        return ""
    buffer = ctypes.create_string_buffer(length + 1)
    library.ruscadb_last_error(buffer, c_size_t(len(buffer)))
    return buffer.value.decode("utf-8", errors="replace")


def _check(code: int) -> None:
    """Convierte un codigo del ABI en excepcion si no es ``RC_OK``.

    Args:
        code: Codigo de retorno del ABI.

    Raises:
        RuscadbError: si ``code`` es distinto de ``RC_OK``.
    """
    if code != RC_OK:
        raise RuscadbError(code, last_error())


def open(  # noqa: A001 - nombre exigido por el contrato publico del wrapper
    data_path: str | os.PathLike[str], pool_capacity: int = 8
) -> c_void_p:
    """Abre (o crea) una base RuscaDB.

    Args:
        data_path: Ruta del archivo de paginas.
        pool_capacity: Numero de marcos del buffer pool (>= 1).

    Returns:
        El handle opaco devuelto por el ABI.

    Raises:
        RuscadbError: si el ABI devuelve un error.
    """
    handle = c_void_p()
    bytes_path = os.fsencode(os.fspath(data_path))
    _check(
        _lib().ruscadb_open(bytes_path, c_uint32(pool_capacity), ctypes.byref(handle))
    )
    return handle


def read_page(handle: c_void_p, page_id: int) -> bytes:
    """Lee una pagina completa (``PAGE_SIZE`` bytes).

    Args:
        handle: Handle devuelto por :func:`open`.
        page_id: Identificador de la pagina.

    Returns:
        El contenido de la pagina como ``bytes``.

    Raises:
        RuscadbError: si el ABI devuelve un error.
    """
    buffer = (ctypes.c_uint8 * PAGE_SIZE)()
    _check(
        _lib().ruscadb_read_page(
            handle, c_uint64(page_id), ctypes.cast(buffer, _U8P), c_size_t(PAGE_SIZE)
        )
    )
    return bytes(buffer)


def write_page(handle: c_void_p, page_id: int, data: bytes) -> None:
    """Escribe una pagina completa (``PAGE_SIZE`` bytes).

    Args:
        handle: Handle devuelto por :func:`open`.
        page_id: Identificador de la pagina.
        data: Contenido de exactamente ``PAGE_SIZE`` bytes.

    Raises:
        ValueError: si ``data`` no mide ``PAGE_SIZE`` bytes.
        RuscadbError: si el ABI devuelve un error.
    """
    payload = bytes(data)
    if len(payload) != PAGE_SIZE:
        raise ValueError(f"data debe medir PAGE_SIZE={PAGE_SIZE} bytes, no {len(payload)}")
    buffer = (ctypes.c_uint8 * PAGE_SIZE).from_buffer_copy(payload)
    _check(
        _lib().ruscadb_write_page(
            handle, c_uint64(page_id), ctypes.cast(buffer, _U8P), c_size_t(PAGE_SIZE)
        )
    )


def commit(handle: c_void_p) -> None:
    """Confirma los cambios pendientes (WAL-first + fsync).

    Args:
        handle: Handle devuelto por :func:`open`.

    Raises:
        RuscadbError: si el ABI devuelve un error.
    """
    _check(_lib().ruscadb_commit(handle))


def close(handle: c_void_p) -> None:
    """Cierra el handle y libera el motor.

    Args:
        handle: Handle devuelto por :func:`open`.

    Raises:
        RuscadbError: si el ABI devuelve un error.
    """
    _check(_lib().ruscadb_close(handle))


def execute(handle: c_void_p, sql: str) -> str:
    """Ejecuta una consulta RQL y devuelve el JSON de las filas.

    Esquema JSON: array de filas; cada fila es un objeto ``columna -> valor``
    y cada valor usa la forma externa de ``ScalarValue`` (por ejemplo
    ``{"Int": 1}`` o ``{"Text": "x"}``).

    Args:
        handle: Handle devuelto por :func:`open`.
        sql: Consulta RQL en texto UTF-8.

    Returns:
        El JSON serializado de las filas.

    Raises:
        RuscadbError: si la consulta o el handle fallan.
    """
    library = _lib()
    encoded = sql.encode("utf-8")
    length = int(library.ruscadb_execute_len(handle, encoded))
    if length <= 0:
        raise RuscadbError(RC_DOMAIN_ERROR, last_error())
    buffer = ctypes.create_string_buffer(length + 1)
    _check(library.ruscadb_execute(handle, encoded, buffer, c_size_t(len(buffer))))
    return buffer.value.decode("utf-8", errors="replace")


__all__ = [
    "PAGE_SIZE",
    "RC_OK",
    "RC_NULL_POINTER",
    "RC_INVALID_HANDLE",
    "RC_DOMAIN_ERROR",
    "RC_PANIC",
    "RC_BUFFER_TOO_SMALL",
    "RuscadbError",
    "load_library",
    "last_error",
    "open",
    "read_page",
    "write_page",
    "commit",
    "close",
    "execute",
]
