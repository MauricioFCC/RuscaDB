#!/usr/bin/env python3
"""Gate RED-first en CI (SPEC-0094).

Para cada spec **en curso** (`status: planned`/`draft`) tocado por el PR,
ejecuta ``cargo xtask red <spec>``, que exige que los tests citados por sus ACs
**fallen** (evidencia test-first). Un spec `implemented` no aplica (sus tests
deben pasar) y se omite.

Uso::

    # Descubre los specs del diff del PR y verifica los que están en curso.
    python scripts/red_first_gate.py --diff pr.diff
    # O una lista explícita.
    python scripts/red_first_gate.py --specs specs/foo.md specs/bar.md
    # Solo lista los specs en curso (sin ejecutar cargo).
    python scripts/red_first_gate.py --list

Salida: exit 0 si no hay specs en curso o todos son RED; exit 1 si alguno ya
pasa; exit 2 ante un spec ausente o un fallo de entrada (ruidoso, SPEC-0061).
Solo stdlib de Python.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
USAGE = """usage: red_first_gate.py [--diff PR_DIFF | --specs S1 [S2...] | --list]

Gate RED-first (SPEC-0094): los specs en curso (`planned`/`draft`) deben tener
sus tests en rojo. `--list` solo imprime los specs en curso.
"""

IN_PROGRESS = ("planned", "draft")


def spec_status(text: str) -> str:
    """Extrae el estado del frontmatter de un spec.

    Args:
        text: Contenido del spec.

    Returns:
        El valor de ``status:`` (cadena vacía si falta).
    """
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("status:"):
            return stripped.split(":", 1)[1].strip()
    return ""


def is_in_progress(path: Path) -> bool:
    """Indica si el spec existe y su estado es de trabajo en curso.

    Args:
        path: Ruta del spec.

    Returns:
        ``True`` si el fichero existe y ``status`` es ``planned``/``draft``.
    """
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return False
    return spec_status(text) in IN_PROGRESS


def specs_from_diff(diff_path: Path) -> list[Path]:
    """Extrae los specs tocados por un diff de git.

    Args:
        diff_path: Ruta del fichero ``.diff``.

    Returns:
        Lista de rutas de spec existentes bajo ``specs/``.
    """
    try:
        text = diff_path.read_text(encoding="utf-8")
    except OSError as exc:
        print(f"red-first: no se pudo leer el diff '{diff_path}': {exc}")
        sys.exit(2)
    found: list[Path] = []
    for raw in text.splitlines():
        for prefix in ("+++ b/", "--- a/"):
            if not raw.startswith(prefix):
                continue
            candidate = raw[len(prefix):].strip()
            if not candidate.startswith("specs/") or not candidate.endswith(".md"):
                continue
            path = ROOT / candidate
            if path.exists() and path not in found:
                found.append(path)
    return found


def parse_args(argv: list[str]) -> tuple[list[Path], bool]:
    """Parsea los argumentos CLI.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en ``[0]``).

    Returns:
        ``(specs, list_only)``.

    Raises:
        SystemExit: exit 2 si la invocación es inválida.
    """
    args = argv[1:]
    if "--help" in args or "-h" in args:
        print(USAGE)
        sys.exit(0)
    list_only = "--list" in args
    specs: list[Path] = []
    if "--diff" in args:
        index = args.index("--diff")
        if index + 1 >= len(args):
            print("red-first: --diff requiere la ruta del fichero .diff")
            sys.exit(2)
        specs = specs_from_diff(Path(args[index + 1]))
    if "--specs" in args:
        index = args.index("--specs")
        names = [name for name in args[index + 1:] if not name.startswith("--")]
        if not names:
            print("red-first: --specs requiere al menos una ruta")
            sys.exit(2)
        specs = []
        missing = []
        for name in names:
            candidate = Path(name)
            path = candidate if candidate.is_absolute() else ROOT / candidate
            if not path.exists():
                missing.append(name)
            else:
                specs.append(path)
        if missing:
            print(f"red-first: spec(s) inexistente(s): {', '.join(missing)}")
            sys.exit(2)
    if not specs and not list_only and "--diff" not in args and "--specs" not in args:
        print(USAGE)
        sys.exit(2)
    return specs, list_only


def main(argv: list[str]) -> int:
    """Ejecuta el gate RED-first.

    Args:
        argv: Argumentos de línea de comandos.

    Returns:
        Exit code: ``0`` OK, ``1`` algún spec ya pasa, ``2`` error de entrada.
    """
    specs, list_only = parse_args(argv)
    in_progress = [spec for spec in specs if is_in_progress(spec)]

    if list_only:
        for spec in sorted(in_progress):
            try:
                print(spec.relative_to(ROOT).as_posix())
            except ValueError:
                print(spec.as_posix())
        return 0

    if not in_progress:
        print("red-first: OK (ningún spec en curso tocado por el PR)")
        return 0

    failed = False
    for spec in in_progress:
        relative = spec.relative_to(ROOT).as_posix()
        result = subprocess.run(
            ["cargo", "xtask", "red", relative], cwd=ROOT, check=False
        )
        if result.returncode != 0:
            print(f"red-first: {relative} no cumple RED-first (exit {result.returncode})")
            failed = True
    if failed:
        print("red-first: FAIL (algún spec en curso sin evidencia RED)")
        return 1
    print(f"red-first: OK ({len(in_progress)} spec(s) en curso son RED)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
