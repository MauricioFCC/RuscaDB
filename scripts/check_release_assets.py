#!/usr/bin/env python3
"""Valida la matriz target→asset del release sin red (SPEC-0208).

Comprueba que `release.yml`, los instaladores y la documentación hablan de los
mismos targets/assets y que existe un fallback reproducible a `cargo install`.

Uso:
    python scripts/check_release_assets.py --dry-run
    python scripts/check_release_assets.py --dry-run --json

Salidas:
    0 todo coherente; 1 incidencias; 2 root inválido.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

TARGETS = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
]
FALLBACK = (
    "cargo install --git https://github.com/MauricioFCC/RuscaDB "
    "--package ruscadb-cli --locked"
)


def check(root: pathlib.Path) -> list[str]:
    """Devuelve las incidencias de la matriz y los instaladores.

    Args:
        root: Raíz del repositorio.

    Returns:
        Lista de incidencias legibles (vacía si todo es coherente).
    """
    issues: list[str] = []
    workflow = root / ".github" / "workflows" / "release.yml"
    install_sh = root / "scripts" / "install.sh"
    install_ps1 = root / "scripts" / "install.ps1"
    readme = root / "README.md"
    install_doc = root / "docs" / "instalacion.md"

    if not workflow.exists():
        return ["falta .github/workflows/release.yml"]
    yaml = workflow.read_text(encoding="utf-8")
    declared = set(re.findall(r"^\s*-\s*target:\s*(\S+)", yaml, re.M))
    if declared != set(TARGETS):
        issues.append(
            f"release.yml targets={sorted(declared)} != matriz={sorted(TARGETS)}"
        )
    for target in TARGETS:
        asset = f"ruscadb-${{{{ matrix.target }}}}.tar.gz"
        if target not in declared:
            issues.append(f"release.yml no declara el target {target}")
        if asset not in yaml:
            issues.append(f"release.yml no empaqueta {asset}")
            break
    if "ruscadb-${{ matrix.target }}.sha256" not in yaml:
        issues.append("release.yml no publica checksums .sha256")

    for path in (install_sh, install_ps1):
        if not path.exists():
            issues.append(f"falta {path.relative_to(root)}")
            continue
        text = path.read_text(encoding="utf-8")
        for needle, why in [
            ("releases/latest", "resuelve la última versión desde releases/latest"),
            ("releases/download", "descarga assets de releases/download"),
            ("ruscadb-", "construye el nombre del asset"),
            ("cargo install --git", "documenta el fallback cargo install --git"),
            ("--package ruscadb-cli --locked", "fija paquete y lockfile del fallback"),
        ]:
            if needle not in text:
                issues.append(f"{path.name}: falta '{why}'")

    for path in (readme, install_doc):
        if not path.exists():
            issues.append(f"falta {path.relative_to(root)}")
            continue
        text = path.read_text(encoding="utf-8")
        for needle, why in [
            (FALLBACK, "fallback cargo install --git"),
            ("--dry-run", "validación con --dry-run"),
            ("404", "estado sin release publicado"),
        ]:
            if needle not in text:
                issues.append(f"{path.relative_to(root)}: falta '{why}'")
    return issues


def main(argv: list[str] | None = None) -> int:
    """Punto de entrada del validador.

    Args:
        argv: Argumentos CLI.

    Returns:
        Código de salida 0/1/2.
    """
    parser = argparse.ArgumentParser(
        description="Valida la matriz de assets del release (offline)."
    )
    parser.add_argument(
        "--root", default=str(pathlib.Path(__file__).resolve().parents[1])
    )
    parser.add_argument("--dry-run", action="store_true", default=True)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)

    root = pathlib.Path(args.root).resolve()
    if not root.is_dir():
        print(f"[FAIL] check_release_assets: root inválido: {root}", file=sys.stderr)
        return 2
    issues = check(root)
    report = {
        "ok": not issues,
        "targets": len(TARGETS),
        "assets": [f"ruscadb-{target}.tar.gz" for target in TARGETS],
        "issues": issues,
        "dry_run": True,
    }
    if args.json:
        print(json.dumps(report, sort_keys=True))
    elif issues:
        print(f"[FAIL] release-assets: {len(issues)} incidencias:", file=sys.stderr)
        for issue in issues:
            print(f"  - {issue}", file=sys.stderr)
    else:
        print(
            f"[OK] release-assets: {len(TARGETS)} targets, "
            f"{len(TARGETS)} assets, fallback git documentado (dry-run)"
        )
    return 0 if not issues else 1


if __name__ == "__main__":
    raise SystemExit(main())
