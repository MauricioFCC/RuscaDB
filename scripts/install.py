#!/usr/bin/env python3
"""Instalador del ejecutable ``ruscadb`` (CLI) en Windows, Linux y macOS.

Equivalente en Python a ``scripts/install.ps1`` (Windows) y
``scripts/install.sh`` (POSIX), para poder ejecutarlo aunque la política de
ejecución de PowerShell bloquee los ``.ps1``:

    python scripts/install.py
    python scripts/install.py --version 0.1.0 --install-dir C:\\tools\\ruscadb

Pasos:
1. Detecta target (arquitectura + SO) y resuelve la versión (último release).
2. Descarga ``ruscadb-<target>.tar.gz`` y su ``.sha256`` (si existe).
3. Extrae de forma segura, verifica el checksum SHA-256 del binario.
4. Instala en ``--install-dir`` como ``ruscadb.exe`` (Windows) o ``ruscadb``.

Variables de entorno (paridad con los otros instaladores):
    RUSCADB_VERSION  versión a instalar (por defecto: último release)
    INSTALL_DIR      directorio destino
    RUSCADB_REPO     owner/repo de GitHub (por defecto: MauricioFCC/RuscaDB)

Seguridad: sin secretos, valida repo/versión/checksum, extracción sin path
traversal (``tarfile`` data filter) y escritura atómica del binario.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import platform
import re
import shutil
import stat
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import NoReturn

DEFAULT_REPO = "MauricioFCC/RuscaDB"
BINARY_NAME = "ruscadb"
CHECKSUM_HEX = re.compile(r"^[0-9a-f]{64}$")
REPO_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
VERSION_PATTERN = re.compile(r"^[0-9][A-Za-z0-9._+-]*$")
USER_AGENT = "ruscadb-install-py/1.0"


def log(message: str) -> None:
    """Imprime un mensaje con el prefijo del instalador."""
    print(f"[ruscadb] {message}", flush=True)


def warn(message: str) -> None:
    """Imprime un aviso con el prefijo del instalador."""
    print(f"[ruscadb] aviso: {message}", file=sys.stderr, flush=True)


def fail(message: str) -> NoReturn:
    """Termina el instalador con error legible y exit code 1."""
    print(f"[ruscadb] ERROR: {message}", file=sys.stderr, flush=True)
    raise SystemExit(1)


def detect_target() -> str:
    """Devuelve el triple target Rust soportado por el release.

    Returns:
        ``x86_64-pc-windows-msvc``, ``aarch64-pc-windows-msvc``,
        ``x86_64-unknown-linux-gnu``, ``aarch64-unknown-linux-gnu``,
        ``x86_64-apple-darwin`` o ``aarch64-apple-darwin``.

    Raises:
        SystemExit: Si el sistema o la arquitectura no están soportados.
    """
    system = platform.system()
    machine = platform.machine().lower()
    arch = "aarch64" if machine in {"arm64", "aarch64"} else "x86_64"
    if system == "Windows":
        return f"{arch}-pc-windows-msvc"
    if system == "Linux":
        return f"{arch}-unknown-linux-gnu"
    if system == "Darwin":
        return f"{arch}-apple-darwin"
    fail(f"sistema no soportado: {system} (usa el binario manual desde Releases)")


def default_install_dir() -> Path:
    """Directorio destino por defecto según el sistema operativo."""
    if platform.system() == "Windows":
        base = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        return Path(base) / "Programs" / BINARY_NAME / "bin"
    return Path.home() / ".local" / "bin"


def _http_get(url: str) -> bytes:
    """Descarga ``url`` y devuelve el cuerpo en bytes (con User-Agent)."""
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def resolve_version(requested: str, repo: str) -> str:
    """Resuelve la versión a instalar (normaliza prefijo ``v``).

    Args:
        requested: Versión explícita (vacía para consultar el último release).
        repo: Repositorio ``owner/repo``.

    Returns:
        Versión sin prefijo ``v`` (p. ej. ``0.1.0``).
    """
    if requested:
        return requested.strip().lstrip("vV")
    try:
        payload = _http_get(f"https://api.github.com/repos/{repo}/releases/latest")
    except (urllib.error.URLError, OSError) as exc:
        fail(f"no se pudo resolver la última versión: {exc}")
    match = re.search(r'"tag_name"\s*:\s*"v?([^"]+)"', payload.decode("utf-8"))
    if not match:
        fail("la respuesta de GitHub no contiene tag_name")
    return match.group(1)


def verify_checksum(binary: Path, expected_hex: str) -> None:
    """Verifica el SHA-256 del binario descargado.

    Args:
        binary: Ruta del binario extraído.
        expected_hex: Digest esperado (64 hex, minúsculas).

    Raises:
        SystemExit: Si el digest no coincide.
    """
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    if digest != expected_hex:
        fail(f"checksum inválido (esperado {expected_hex}, obtenido {digest})")
    log("checksum verificado")


def fetch_checksum(url: str, target: str) -> str | None:
    """Descarga el ``.sha256`` del release; ``None`` si no está publicado."""
    try:
        payload = _http_get(url)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            warn(f"no hay checksum publicado para {target}")
            return None
        fail(f"error descargando checksum: {exc}")
    except (urllib.error.URLError, OSError) as exc:
        fail(f"error descargando checksum: {exc}")
    expected = payload.decode("utf-8", errors="replace").split()[0].lower()
    if not CHECKSUM_HEX.match(expected):
        fail(f"checksum malformado en {url}: {expected!r}")
    return expected


def safe_extract(archive: Path, destination: Path) -> None:
    """Extrae un ``tar.gz`` rechazando rutas absolutas o con ``..``.

    Args:
        archive: Fichero ``.tar.gz`` descargado.
        destination: Directorio de extracción.

    Raises:
        SystemExit: Si el archivo es inválido o intenta path traversal.
    """
    destination.mkdir(parents=True, exist_ok=True)
    try:
        with tarfile.open(archive, "r:gz") as tar:
            if hasattr(tarfile, "data_filter"):
                tar.extractall(destination, filter="data")
                return
            for member in tar.getmembers():
                member_path = destination / member.name
                if member.name.startswith(("/", "..")) or destination not in member_path.parents:
                    fail(f"archivo con ruta insegura: {member.name!r}")
            tar.extractall(destination)
    except tarfile.TarError as exc:
        fail(f"archivo tar.gz inválido: {exc}")


def install_binary(source: Path, install_dir: Path) -> Path:
    """Copia el binario a ``install_dir`` de forma atómica.

    Args:
        source: Binario extraído del release.
        install_dir: Directorio destino.

    Returns:
        Ruta final del ejecutable instalado.
    """
    install_dir.mkdir(parents=True, exist_ok=True)
    suffix = ".exe" if platform.system() == "Windows" else ""
    destination = install_dir / f"{BINARY_NAME}{suffix}"
    temporary = destination.with_name(destination.name + ".tmp")
    shutil.copyfile(source, temporary)
    if platform.system() != "Windows":
        temporary.chmod(temporary.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    os.replace(temporary, destination)
    return destination


def build_urls(repo: str, version: str, target: str) -> tuple[str, str]:
    """Construye las URLs del archivo y de su checksum en el release."""
    base = f"https://github.com/{repo}/releases/download/v{version}"
    return (
        f"{base}/{BINARY_NAME}-{target}.tar.gz",
        f"{base}/{BINARY_NAME}-{target}.sha256",
    )


def run_install(args: argparse.Namespace) -> int:
    """Ejecuta la instalación completa y devuelve el exit code."""
    if not REPO_PATTERN.match(args.repo):
        fail(f"repo inválido: {args.repo!r} (formato owner/repo)")
    target = detect_target()
    version = resolve_version(args.version, args.repo)
    if not VERSION_PATTERN.match(version):
        fail(f"versión inválida: {version!r}")
    archive_url, checksum_url = build_urls(args.repo, version, target)
    install_dir = Path(args.install_dir).expanduser().resolve()
    if args.dry_run:
        log(f"dry-run: ruscadb {version} para {target}")
        log(f"dry-run: descarga {archive_url}")
        log(f"dry-run: instalaría en {install_dir}")
        return 0
    log(f"instalando ruscadb {version} para {target}")
    with tempfile.TemporaryDirectory(prefix="ruscadb-") as work:
        workdir = Path(work)
        archive = workdir / "ruscadb.tar.gz"
        archive.write_bytes(_http_get(archive_url))
        safe_extract(archive, workdir)
        binary = workdir / f"{BINARY_NAME}-{target}"
        if not binary.is_file():
            fail(f"el archivo no contiene {binary.name}")
        expected = fetch_checksum(checksum_url, target)
        if expected:
            verify_checksum(binary, expected)
        destination = install_binary(binary, install_dir)
    log(f"instalado en {destination}")
    log(f"agrega {install_dir} al PATH si no está ya")
    log("prueba: ruscadb version")
    return 0


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Parsea los argumentos de línea de comandos."""
    parser = argparse.ArgumentParser(
        description="Instala el ejecutable ruscadb desde GitHub Releases",
    )
    parser.add_argument("--version", default=os.environ.get("RUSCADB_VERSION", ""),
                        help="versión a instalar (default: último release)")
    parser.add_argument("--install-dir", default=os.environ.get("INSTALL_DIR", ""),
                        help="directorio destino del ejecutable")
    parser.add_argument("--repo", default=os.environ.get("RUSCADB_REPO", DEFAULT_REPO),
                        help="owner/repo de GitHub")
    parser.add_argument("--dry-run", action="store_true",
                        help="resuelve target/versión/URL sin descargar ni instalar")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Entrada del instalador."""
    args = parse_args(argv)
    if not args.install_dir:
        args.install_dir = str(default_install_dir())
    return run_install(args)


if __name__ == "__main__":
    raise SystemExit(main())
