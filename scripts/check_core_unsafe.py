#!/usr/bin/env python3
"""Guard de ``unsafe`` de RuscaDB (gate T1).

Verifica la politica de ``unsafe`` (``docs/RuscaDB-roadmap.md`` §6.3):

1. ``crates/ruscadb-core/src/lib.rs`` declara ``#![forbid(unsafe_code)]``.
2. Ninguna crate usa la palabra clave ``unsafe`` por encima de su presupuesto
   declarado en ``unsafe-allowlist.toml`` (F0: presupuesto 0 en todas).
3. Cada uso de ``unsafe`` detectado se reporta con archivo y linea.

El detector ignora comentarios, literales de cadena y nombres de lint como
``unsafe_code``; solo cuenta *uso* real (``unsafe {``, ``unsafe fn``, ...).

Salida: exit 0 si cumple; exit 1 con diagnostico legible si no.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES_DIR = ROOT / "crates"
BUDGET_FILE = ROOT / "unsafe-allowlist.toml"
CORE_LIB = CRATES_DIR / "ruscadb-core" / "src" / "lib.rs"

# Elimina comentarios de linea, de bloque y literales de cadena.
COMMENT_RE = re.compile(r"//[^\n]*|/\*.*?\*/", re.DOTALL)
STRING_RE = re.compile(r'"(?:\\.|[^"\\])*"')
# `unsafe` como palabra clave real; excluye `unsafe_code`, `unsafe_op_...`.
UNSAFE_RE = re.compile(r"\bunsafe\b(?!_)")


def load_budget() -> dict[str, int]:
    """Lee el presupuesto de unsafe por crate desde el TOML."""
    with BUDGET_FILE.open("rb") as handle:
        data = tomllib.load(handle)
    return {str(k): int(v) for k, v in data.get("budget", {}).items()}


def strip_noise(source: str) -> str:
    """Elimina comentarios y strings para no contar ruido como uso."""
    source = COMMENT_RE.sub(" ", source)
    return STRING_RE.sub('""', source)


def find_usages(source: str) -> list[int]:
    """Devuelve las lineas (1-indexed) con uso real de `unsafe`."""
    clean = strip_noise(source)
    lines: list[int] = []
    for number, line in enumerate(clean.splitlines(), start=1):
        if UNSAFE_RE.search(line):
            lines.append(number)
    return lines


def main() -> int:
    """Ejecuta las verificaciones y devuelve el exit code."""
    errors: list[str] = []

    # 1. `#![forbid(unsafe_code)]` obligatorio en core.
    if not CORE_LIB.exists():
        errors.append(f"no existe {CORE_LIB.relative_to(ROOT)}")
    else:
        core_text = CORE_LIB.read_text(encoding="utf-8")
        if "#![forbid(unsafe_code)]" not in core_text:
            errors.append(
                "ruscadb-core/src/lib.rs no declara #![forbid(unsafe_code)]"
            )

    # 2/3. Uso de `unsafe` vs presupuesto por crate.
    budget = load_budget()
    for crate_dir in sorted(p for p in CRATES_DIR.iterdir() if p.is_dir()):
        crate = crate_dir.name
        limit = budget.get(crate, 0)
        src = crate_dir / "src"
        if not src.exists():
            continue
        usages: list[tuple[Path, int]] = []
        for rs_file in sorted(src.rglob("*.rs")):
            for line in find_usages(rs_file.read_text(encoding="utf-8")):
                usages.append((rs_file, line))
        if len(usages) > limit:
            for file, line in usages:
                rel = file.relative_to(ROOT)
                errors.append(
                    f"{rel}:{line}: uso de unsafe (presupuesto de "
                    f"'{crate}' = {limit})"
                )

    if errors:
        print("[FAIL] check_core_unsafe: politica de unsafe violada:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print("[OK] check_core_unsafe: core sin unsafe; 0 usos sobre presupuesto.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
