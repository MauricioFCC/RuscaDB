#!/usr/bin/env python3
"""Reporte y gate explícito de cobertura de **ramas** (BRDA) por crate.

SPEC-0092: el job ``coverage`` de ``ci.yml`` ya aplica
``scripts/coverage_gate.py ... --min-branch``, pero no publicaba un reporte
explícito de ramas. Este script reutiliza el parser de ``coverage_gate.py``
(importado como módulo, sin duplicar la lógica de lcov) para:

1. imprimir la cobertura de ramas de cada scope y de cada crate dentro del
   scope (ordenada de peor a mejor, para señalar dónde endurecer);
2. fallar (exit 1) si algún scope queda por debajo de su ``--min-branch``;
3. volcar el reporte a JSON (``--json-out``) para adjuntarlo como artefacto.

Uso::

    branch_coverage_report.py lcov.info \
        --scope ruscadb-core/ --min-branch 70 \
        --scope ruscadb-txn/  --min-branch 70 \
        --scope ruscadb-wal/  --min-branch 70 \
        --json-out branch-coverage.json

Salida: exit 0 todos cumplen; exit 1 algún scope bajo umbral; exit 2 lcov
inexistente/vacío o scope sin ficheros (fallo ruidoso, contrato SPEC-0061).
Solo stdlib de Python.
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_GATE_PATH = _HERE / "coverage_gate.py"
# `coverage_gate.py` solo importa `sys`, así que cargarlo como módulo es seguro
# y evita duplicar el parser de lcov (SPEC-0092/NF-01).
_spec = importlib.util.spec_from_file_location("coverage_gate", _GATE_PATH)
if _spec is None or _spec.loader is None:  # pragma: no cover - entorno roto
    print(f"branch-coverage: no se pudo cargar {_GATE_PATH}")
    sys.exit(2)
coverage_gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(coverage_gate)

USAGE = """usage: branch_coverage_report.py lcov.info \\
           [--scope S --min-branch B]... [--json-out PATH]

Reporte explícito de cobertura de ramas (BRDA) por scope y crate (SPEC-0092).

Cada `--scope` declara un scope con su umbral `--min-branch`; sin `--scope` se
usa el default `ruscadb-core/` con 70.

Salida: exit 0 todos cumplen; exit 1 algún scope bajo umbral; exit 2 lcov
inexistente/vacío o scope sin ficheros.
"""

DEFAULT_SCOPE = "ruscadb-core/"
DEFAULT_MIN_BRANCH = 70.0


def parse_args(argv):
    """Parsea los argumentos CLI del reporte.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en ``[0]``).

    Returns:
        ``(lcov_path, scopes, json_out)`` donde ``scopes`` es una lista de
        diccionarios ``{"scope", "min_branch"}`` y ``json_out`` es la ruta de
        salida JSON o ``None``.
    """
    args = list(argv[1:])
    json_out = None
    if "--json-out" in args:
        index = args.index("--json-out")
        if index + 1 < len(args):
            json_out = args[index + 1]
            del args[index:index + 2]
        else:
            del args[index:]
    path, scopes = coverage_gate.parse_args([argv[0], *args])
    return path, scopes, json_out


def scope_report(entry, records):
    """Construye el reporte de ramas de un scope.

    Args:
        entry: Diccionario ``{"scope", "min_line", "min_branch"}``.
        records: Registros lcov por ruta.

    Returns:
        Diccionario con ``scope``, ``branch_percent``, ``branch_hit``,
        ``branch_total``, ``min_branch``, ``ok`` y ``crates`` (lista ordenada de
        peor a mejor cobertura de ramas).
    """
    group = coverage_gate.select_scope(records, entry["scope"])
    totals = coverage_gate.aggregate(group)
    branch_percent = coverage_gate.percent(totals[3], totals[2])
    crates = []
    for name, bucket in coverage_gate.group_by_crate(group, entry["scope"]).items():
        crates.append(
            {
                "crate": name,
                "branch_percent": round(
                    coverage_gate.percent(bucket[3], bucket[2]), 1
                ),
                "branch_hit": bucket[3],
                "branch_total": bucket[2],
            }
        )
    crates.sort(key=lambda item: (item["branch_percent"], item["crate"]))
    # Sin BRDA en el scope no hay ramas que medir: se reporta y pasa (igual que
    # coverage_gate).
    ok = totals[2] == 0 or branch_percent >= entry["min_branch"]
    return {
        "scope": entry["scope"],
        "branch_percent": round(branch_percent, 1),
        "branch_hit": totals[3],
        "branch_total": totals[2],
        "min_branch": entry["min_branch"],
        "ok": ok,
        "crates": crates,
    }


def main(argv):
    """Punto de entrada del reporte.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en ``[0]``).

    Returns:
        Exit code: ``0`` PASS, ``1`` algún scope bajo umbral, ``2`` error de
        entrada.
    """
    if "--help" in argv or "-h" in argv:
        print(USAGE)
        return 0
    path, scopes, json_out = parse_args(argv)
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except FileNotFoundError:
        print(f"branch-coverage: {path} no existe (sin corrida llvm-cov)")
        return 2
    records = coverage_gate.parse_lcov_records(text)
    if not records:
        print("branch-coverage: lcov vacío (sin ficheros SF:)")
        return 2

    report = []
    all_ok = True
    for entry in scopes:
        group = coverage_gate.select_scope(records, entry["scope"])
        if not group:
            print(
                f"branch-coverage: sin datos para el scope '{entry['scope']}' "
                f"(lcov vacío o scope sin ficheros)"
            )
            return 2
        item = scope_report(entry, records)
        report.append(item)
        state = "PASS" if item["ok"] else "FAIL"
        if item["branch_total"] == 0:
            print(
                f"branch-coverage {item['scope']}: rama=n/a (sin BRDA) "
                f"min={entry['min_branch']:g} {state}"
            )
        else:
            print(
                f"branch-coverage {item['scope']}: "
                f"rama={item['branch_percent']:.1f}%/{entry['min_branch']:g} {state}"
            )
        for crate in item["crates"]:
            print(
                f"    - {crate['crate']}: rama={crate['branch_percent']:.1f}% "
                f"({crate['branch_hit']}/{crate['branch_total']})"
            )
        all_ok = all_ok and item["ok"]

    if json_out:
        payload = {"scopes": report, "ok": all_ok}
        try:
            with open(json_out, "w", encoding="utf-8") as handle:
                json.dump(payload, handle, indent=2, sort_keys=True)
                handle.write("\n")
        except OSError as exc:
            print(f"branch-coverage: no se pudo escribir '{json_out}': {exc}")
            return 2

    if all_ok:
        print("branch-coverage: PASS")
        return 0
    print("branch-coverage: FAIL (algún scope bajo su umbral de ramas)")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
