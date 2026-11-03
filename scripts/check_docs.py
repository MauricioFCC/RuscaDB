#!/usr/bin/env python3
"""Gate de calidad de documentación (SPEC-0199).

Comprueba, sin dependencias externas:
- enlaces relativos existentes (no valida anclas);
- rutas absolutas de usuario (`C:\\Users\\...`, `/home/<user>/`);
- bloques de código balanceados (fences ``` o ~~~);
- un único H1 por documento;
- que todo documento de primer nivel de `docs/` esté enlazado desde
  `docs/README.md`;
- que existan los documentos canónicos mínimos.

Uso:
    python scripts/check_docs.py [--root DIR] [--json]

Salidas:
    0 si todo está limpio; 1 si hay incidencias; 2 si el root no es válido.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

REQUIRED = [
    "README.md",
    "AGENTS.md",
    "CHANGELOG.md",
    "docs/README.md",
    "docs/verification.md",
    "docs/specs-index.md",
]

LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
PERSONAL_WINDOWS = re.compile(r"[A-Za-z]:[\\/]+Users[\\/]+[^\\/\s\"']+")
PERSONAL_POSIX = re.compile(r"/home/[^/\s\"']+/")


def slug(text: str) -> str:
    """Convierte un título Markdown en el ancla estilo GitHub.

    Args:
        text: Texto del encabezado sin `#`.

    Returns:
        Ancla en minúsculas, sin puntuación, con espacios como `-`.
    """
    result = []
    for character in text.strip().lower():
        if character.isalnum() or character in "-_":
            result.append(character)
        elif character == " ":
            result.append("-")
    return "".join(result)


def anchors(path: pathlib.Path) -> set[str]:
    """Extrae las anclas de los encabezados de un documento.

    Args:
        path: Documento Markdown.

    Returns:
        Conjunto de anclas (sin `#`).
    """
    text = path.read_text(encoding="utf-8", errors="replace")
    found = set()
    in_fence = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("```") or stripped.startswith("~~~"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        match = HEADING.match(line)
        if match:
            found.add(slug(match.group(2)))
    return found


def iter_docs(root: pathlib.Path) -> list[pathlib.Path]:
    """Lista los Markdown versionables relevantes para el gate.

    Args:
        root: Raíz del repositorio.

    Returns:
        `README.md`, `AGENTS.md`, `CHANGELOG.md` y `docs/**/*.md`, excluyendo
        `docs/archive/` (documentos históricos que no se mantienen).
    """
    files = [root / name for name in ("README.md", "AGENTS.md", "CHANGELOG.md")]
    docs = root / "docs"
    if docs.exists():
        for path in docs.rglob("*.md"):
            if "archive" in path.parts:
                continue
            files.append(path)
    return [path for path in files if path.exists()]


def check_file(path: pathlib.Path, root: pathlib.Path) -> list[str]:
    """Analiza un documento y devuelve incidencias `ruta:línea: mensaje`.

    Args:
        path: Documento a analizar.
        root: Raíz usada para resolver enlaces relativos.

    Returns:
        Lista de incidencias legibles.
    """
    issues: list[str] = []
    text = path.read_text(encoding="utf-8", errors="replace")
    relative = path.relative_to(root).as_posix()
    local_anchors = anchors(path)

    fences = 0
    h1 = 0
    in_fence = False
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("```") or stripped.startswith("~~~"):
            fences += 1
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if "docs-gate:allow" in line:
            continue
        if line.startswith("# "):
            h1 += 1

        for match in LINK.finditer(line):
            target = match.group(1).strip()
            if not target or target.startswith(("http://", "https://", "mailto:")):
                continue
            raw_target = target
            target, _, fragment = target.partition("#")
            target = target.strip("<>")
            if not target:
                if fragment and fragment not in local_anchors:
                    issues.append(
                        f"{relative}:{number}: ancla rota -> #{fragment}"
                    )
                continue
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                issues.append(f"{relative}:{number}: enlace roto -> {raw_target}")
            elif fragment and resolved.suffix == ".md" and fragment not in anchors(resolved):
                issues.append(
                    f"{relative}:{number}: ancla rota -> {raw_target}"
                )

        for regex, kind in (
            (PERSONAL_WINDOWS, "ruta Windows de usuario"),
            (PERSONAL_POSIX, "ruta POSIX de usuario"),
        ):
            if regex.search(line):
                issues.append(f"{relative}:{number}: {kind} prohibida")

    if fences % 2 != 0:
        issues.append(f"{relative}: bloques de código desbalanceados ({fences} fences)")
    if h1 > 1:
        issues.append(f"{relative}: {h1} encabezados H1 (debe haber uno)")
    return issues


def check_index(root: pathlib.Path) -> list[str]:
    """Verifica que todo `docs/*.md` de primer nivel esté enlazado.

    Args:
        root: Raíz del repositorio.

    Returns:
        Incidencias por documentos no enlazados desde `docs/README.md`.
    """
    index = root / "docs" / "README.md"
    if not index.exists():
        return ["docs/README.md: falta el índice de documentación"]
    text = index.read_text(encoding="utf-8", errors="replace")
    issues = []
    for path in sorted((root / "docs").glob("*.md")):
        if path.name == "README.md":
            continue
        if path.name not in text:
            issues.append(f"docs/README.md: no enlaza '{path.name}'")
    return issues


def run(root: pathlib.Path) -> dict:
    """Ejecuta todas las comprobaciones.

    Args:
        root: Raíz del repositorio.

    Returns:
        Diccionario con `ok`, `files`, `links` e `issues`.
    """
    issues: list[str] = []
    for required in REQUIRED:
        if not (root / required).exists():
            issues.append(f"falta documento canónico: {required}")

    files = iter_docs(root)
    links = 0
    for path in files:
        text = path.read_text(encoding="utf-8", errors="replace")
        links += len(LINK.findall(text))
        issues.extend(check_file(path, root))
    issues.extend(check_index(root))
    return {"ok": not issues, "files": len(files), "links": links, "issues": issues}


def main(argv: list[str] | None = None) -> int:
    """Punto de entrada del gate.

    Args:
        argv: Argumentos de línea de comandos.

    Returns:
        Código de salida (0/1/2).
    """
    parser = argparse.ArgumentParser(description="Gate de documentación (SPEC-0199).")
    parser.add_argument("--root", default=str(pathlib.Path(__file__).resolve().parents[1]))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)

    root = pathlib.Path(args.root).resolve()
    if not root.is_dir():
        print(f"[FAIL] check_docs: root inválido: {root}", file=sys.stderr)
        return 2

    report = run(root)
    if args.json:
        print(json.dumps(report, sort_keys=True))
    elif report["ok"]:
        print(
            f"[OK] docs: {report['files']} ficheros, "
            f"{report['links']} enlaces, 0 incidencias"
        )
    else:
        print(f"[FAIL] check_docs: {len(report['issues'])} incidencias:", file=sys.stderr)
        for issue in report["issues"]:
            print(f"  - {issue}", file=sys.stderr)
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
