#!/usr/bin/env python3
"""Valida mensajes Conventional Commits (SPEC-0089).

Uso:

    python scripts/check_commits.py --range origin/main..HEAD

    python scripts/check_commits.py --file mensajes.txt

    python scripts/check_commits.py --message "feat(db): soporte KNN"

Reglas (Conventional Commits 1.0):

- Formato ``type(scope)!: descripcion``; ``scope`` y ``!`` son opcionales.
- ``type`` debe pertenecer al conjunto permitido (build, chore, ci, docs,
  feat, fix, perf, refactor, revert, style, test).
- Los commits de merge (``Merge ...``) y los revert automaticos de git
  (``Revert "..."``) se ignoran.
- Sin mensajes que revisar (rango/fichero vacio) => exit 2 (fallo ruidoso,
  nunca verde silencioso).

Codigos de salida:

- ``0`` = todos los mensajes son validos (o ignorados).
- ``1`` = al menos un mensaje invalido (nombra el infractor).
- ``2`` = error de entrada (sin mensajes, fichero inexistente, rango invalido).

Solo stdlib de Python.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

#: Tipos de Conventional Commits admitidos en el repositorio.
ALLOWED_TYPES: tuple[str, ...] = (
    "build",
    "chore",
    "ci",
    "docs",
    "feat",
    "fix",
    "perf",
    "refactor",
    "revert",
    "style",
    "test",
)

#: ``type(scope)!: descripcion`` con scope/breaking opcionales.
PATTERN = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^()]+)\))?(?P<breaking>!)?: (?P<subject>\S.*)$"
)


def is_ignored(subject: str) -> bool:
    """Indica si el asunto debe ignorarse (merge/revert automaticos).

    Args:
        subject: Primera linea del mensaje de commit.

    Returns:
        ``True`` si es un merge o revert generado por git/herramientas.
    """
    return subject.startswith("Merge ") or subject.startswith('Revert "')


def lint_subject(subject: str) -> str | None:
    """Valida un asunto individual.

    Args:
        subject: Primera linea del mensaje de commit.

    Returns:
        ``None`` si es valido o ignorado; si no, el motivo del rechazo.
    """
    if is_ignored(subject):
        return None
    match = PATTERN.match(subject)
    if match is None:
        return "no sigue 'type(scope): descripcion'"
    if match.group("type") not in ALLOWED_TYPES:
        return f"tipo '{match.group('type')}' no permitido"
    return None


def lint_subjects(subjects: list[str]) -> list[tuple[str, str]]:
    """Filtra los asuntos invalidos con el motivo del rechazo.

    Args:
        subjects: Asuntos (primera linea de cada commit).

    Returns:
        Lista de ``(mensaje, motivo)`` de los invalidos; vacia si todos valen.
    """
    offenders: list[tuple[str, str]] = []
    for subject in subjects:
        reason = lint_subject(subject)
        if reason is not None:
            offenders.append((subject, reason))
    return offenders


def git_subjects(rev_range: str) -> list[str] | None:
    """Lee los asuntos de los commits de un rango de git.

    Args:
        rev_range: Rango tipo ``origin/main..HEAD``.

    Returns:
        Lista de asuntos (sin merges); ``None`` si el rango no se pudo leer.
    """
    try:
        output = subprocess.run(
            ["git", "log", "--no-merges", "--format=%s", rev_range],
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"check_commits: no se pudo leer el rango '{rev_range}': {exc}")
        return None
    return [line for line in output.stdout.splitlines() if line.strip()]


def read_subjects(path: str) -> list[str] | None:
    """Lee asuntos de un fichero (un mensaje por linea, ``#`` comenta).

    Args:
        path: Ruta del fichero de mensajes.

    Returns:
        Lista de asuntos; ``None`` si el fichero no se pudo leer.
    """
    try:
        text = Path(path).read_text(encoding="utf-8-sig")
    except OSError as exc:
        print(f"check_commits: no se pudo leer '{path}': {exc}")
        return None
    return [
        line.strip()
        for line in text.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parsea los argumentos de entrada.

    Args:
        argv: Argumentos (incluye el nombre del script en ``[0]``).

    Returns:
        Namespace con exactamente uno de ``rev_range``/``path``/``message``.
    """
    parser = argparse.ArgumentParser(
        description="Valida mensajes Conventional Commits (SPEC-0089)."
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--range", dest="rev_range", help="Rango de git A..B")
    group.add_argument("--file", dest="path", help="Fichero con un asunto por linea")
    group.add_argument("--message", dest="message", help="Un unico asunto")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    """Punto de entrada del linter.

    Args:
        argv: Argumentos de linea de comandos (incluye el script en ``[0]``).

    Returns:
        Exit code: ``0`` valido, ``1`` invalido, ``2`` error de entrada.
    """
    args = parse_args(argv)
    if args.rev_range is not None:
        subjects = git_subjects(args.rev_range)
    elif args.path is not None:
        subjects = read_subjects(args.path)
    else:
        stripped = args.message.strip()
        subjects = [stripped] if stripped else []
    if subjects is None:
        return 2
    if not subjects:
        print("check_commits: sin mensajes que revisar (rango/fichero vacio)")
        return 2
    offenders = lint_subjects(subjects)
    if offenders:
        print(f"check_commits: {len(offenders)} mensaje(s) invalido(s):")
        for subject, reason in offenders:
            print(f"  - {subject!r}: {reason}")
        return 1
    print(f"check_commits: OK ({len(subjects)} mensaje(s) Conventional Commits)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
