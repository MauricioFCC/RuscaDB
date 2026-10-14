#!/usr/bin/env python3
"""Valida el CHANGELOG.md para un release (SPEC-0089).

Uso:

    python scripts/check_changelog.py CHANGELOG.md

    python scripts/check_changelog.py CHANGELOG.md --tag v0.1.0

Reglas (Keep a Changelog):

- Debe existir la seccion ``## [Unreleased]`` con contenido (alguna linea de
  texto o de lista bajo ella).
- Con ``--tag vX.Y.Z`` debe existir ademas la seccion ``## [X.Y.Z]`` con
  contenido (el tag que se libera tiene que estar documentado).

Codigos de salida:

- ``0`` = el CHANGELOG cumple.
- ``1`` = incumple (se nombra cada problema).
- ``2`` = fichero inexistente o no legible (fallo ruidoso).

Solo stdlib de Python.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

UNRELEASED_HEADER = "## [Unreleased]"


def section_body(text: str, header_prefix: str) -> list[str] | None:
    """Devuelve las lineas del cuerpo de una seccion ``## ...``.

    Args:
        text: Contenido completo del CHANGELOG.
        header_prefix: Prefijo del encabezado de nivel 2 a extraer
            (p. ej. ``## [Unreleased]`` o ``## [0.1.0]``).

    Returns:
        Lineas del cuerpo (sin el encabezado) hasta el siguiente ``## ``; o
        ``None`` si la seccion no existe.
    """
    body: list[str] = []
    collecting = False
    for line in text.splitlines():
        if line.startswith("## "):
            if collecting:
                break
            if line.startswith(header_prefix):
                collecting = True
            continue
        if collecting:
            body.append(line)
    return body if collecting else None


def has_content(body: list[str]) -> bool:
    """Indica si el cuerpo de la seccion tiene contenido real.

    Args:
        body: Lineas del cuerpo de una seccion.

    Returns:
        ``True`` si hay al menos una linea no vacia que no sea encabezado
        (``#``); ``False`` si la seccion esta vacia o solo tiene subtitulos.
    """
    for line in body:
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            return True
    return False


def normalize_version(tag: str) -> str:
    """Quita el prefijo ``v``/``V`` de un tag de version.

    Args:
        tag: Tag tal como se empujo (p. ej. ``v0.1.0``).

    Returns:
        Version sin prefijo (p. ej. ``0.1.0``).
    """
    version = tag.strip()
    if version[:1] in ("v", "V"):
        return version[1:]
    return version


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parsea los argumentos de entrada.

    Args:
        argv: Argumentos (incluye el nombre del script en ``[0]``).

    Returns:
        Namespace con ``path`` y ``tag`` (opcional).
    """
    parser = argparse.ArgumentParser(
        description="Valida el CHANGELOG.md para un release (SPEC-0089)."
    )
    parser.add_argument("path", nargs="?", default="CHANGELOG.md")
    parser.add_argument("--tag", default=None, help="Tag a liberar (p. ej. v0.1.0)")
    parser.add_argument(
        "--notes-out",
        dest="notes_out",
        default=None,
        help="Si se indica (junto a --tag), escribe las notas del release en ese fichero",
    )
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    """Punto de entrada del gate de CHANGELOG.

    Args:
        argv: Argumentos de linea de comandos (incluye el script en ``[0]``).

    Returns:
        Exit code: ``0`` cumple, ``1`` incumple, ``2`` error de entrada.
    """
    args = parse_args(argv)
    try:
        text = Path(args.path).read_text(encoding="utf-8-sig")
    except OSError as exc:
        print(f"check_changelog: no se pudo leer '{args.path}': {exc}")
        return 2

    problems: list[str] = []
    unreleased = section_body(text, UNRELEASED_HEADER)
    if unreleased is None:
        problems.append(f"falta la seccion '{UNRELEASED_HEADER}'")
    elif not has_content(unreleased):
        problems.append(f"la seccion '{UNRELEASED_HEADER}' esta vacia")

    if args.tag:
        version = normalize_version(args.tag)
        header = f"## [{version}]"
        body = section_body(text, header)
        if body is None:
            problems.append(
                f"falta la seccion '{header}' para el tag '{args.tag}'"
            )
        elif not has_content(body):
            problems.append(f"la seccion '{header}' esta vacia")
        elif args.notes_out:
            notes = "\n".join(body).strip() + "\n"
            Path(args.notes_out).write_text(notes, encoding="utf-8")
            print(f"check_changelog: notas escritas en '{args.notes_out}'")

    if problems:
        print(f"check_changelog: '{args.path}' no cumple:")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print(f"check_changelog: OK ('{args.path}')")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
