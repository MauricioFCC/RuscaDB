#!/usr/bin/env python3
"""Calcula el mutation score agregado y falla si no alcanza el umbral.

Lee uno o varios ``outcomes.json`` de cargo-mutants (por shard o workspace
completo) y calcula:

    MS = (caught + timeout) / (caught + missed + timeout)

Los mutantes ``Unviable`` (no compilan) y el ``Success`` del baseline no
cuentan. Un ``timeout`` se considera mutante detectado (la suite no termino en
el presupuesto, lo que ya senala una regresion de comportamiento).

Uso:
    python scripts/mutation_score.py --threshold 85 mutants.out/outcomes.json
    python scripts/mutation_score.py --threshold 85 mutants-shards/*/outcomes.json

Salida:
    0 si MS >= umbral, 1 si MS < umbral, 2 si no hay mutantes evaluables o un
    fichero es invalido.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Iterable

CAUGHT = "CaughtMutant"
MISSED = "MissedMutant"
TIMEOUT = "Timeout"
UNVIABLE = "Unviable"
BASELINE = "Success"


def load_outcomes(path: Path) -> list[dict]:
    """Carga la lista de outcomes de un fichero de cargo-mutants.

    Args:
        path: Ruta a ``outcomes.json``.

    Returns:
        Lista de outcomes.

    Raises:
        ValueError: si el JSON no tiene el formato esperado.
    """
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ValueError(f"{path}: no se pudo leer: {error}") from error
    except json.JSONDecodeError as error:
        raise ValueError(f"{path}: JSON invalido: {error}") from error

    if isinstance(payload, dict):
        outcomes = payload.get("outcomes")
        if not isinstance(outcomes, list):
            raise ValueError(f"{path}: falta la clave 'outcomes'")
        return outcomes
    if isinstance(payload, list):
        return payload
    raise ValueError(f"{path}: se esperaba un objeto o una lista")


def summarize(outcomes: Iterable[dict]) -> dict[str, int]:
    """Cuenta outcomes por resumen.

    Args:
        outcomes: Outcomes de cargo-mutants.

    Returns:
        Diccionario ``summary -> cantidad``.
    """
    counts: dict[str, int] = {}
    for outcome in outcomes:
        summary = outcome.get("summary", "Unknown")
        counts[summary] = counts.get(summary, 0) + 1
    return counts


def score(counts: dict[str, int]) -> tuple[float, int, int, int]:
    """Calcula el mutation score.

    Args:
        counts: Conteo por resumen.

    Returns:
        ``(score, caught, missed, timeout)``; ``score`` en ``0.0..=1.0``.
    """
    caught = counts.get(CAUGHT, 0)
    missed = counts.get(MISSED, 0)
    timeout = counts.get(TIMEOUT, 0)
    evaluated = caught + missed + timeout
    if evaluated == 0:
        return 0.0, caught, missed, timeout
    return (caught + timeout) / evaluated, caught, missed, timeout


def main(argv: list[str] | None = None) -> int:
    """Punto de entrada.

    Args:
        argv: Argumentos de linea de comandos (por defecto ``sys.argv``).

    Returns:
        Codigo de salida del proceso.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "outcomes",
        nargs="+",
        type=Path,
        help="Ficheros outcomes.json de cargo-mutants",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=85.0,
        help="Umbral minimo de mutation score en porcentaje (default: 85)",
    )
    parser.add_argument(
        "--allow-empty",
        action="store_true",
        help="No fallar si no hay mutantes evaluables",
    )
    args = parser.parse_args(argv)

    all_outcomes: list[dict] = []
    for path in args.outcomes:
        try:
            outcomes = load_outcomes(path)
        except ValueError as error:
            print(f"[ERROR] {error}", file=sys.stderr)
            return 2
        counts = summarize(outcomes)
        file_score, caught, missed, timeout = score(counts)
        print(
            f"[shard] {path}: MS={file_score * 100:.2f}% "
            f"(caught={caught}, missed={missed}, timeout={timeout}, "
            f"unviable={counts.get(UNVIABLE, 0)}, baseline={counts.get(BASELINE, 0)})"
        )
        all_outcomes.extend(outcomes)

    counts = summarize(all_outcomes)
    global_score, caught, missed, timeout = score(counts)
    evaluated = caught + missed + timeout
    if evaluated == 0:
        message = "[ERROR] sin mutantes evaluables; el gate no puede dar verde"
        if args.allow_empty:
            print(f"{message} (--allow-empty)")
            return 0
        print(message, file=sys.stderr)
        return 2

    percent = global_score * 100
    verdict = "OK" if percent >= args.threshold else "FALLA"
    print(
        f"[{verdict}] mutation score global = {percent:.2f}% "
        f"(objetivo >= {args.threshold:.2f}%; caught={caught}, "
        f"missed={missed}, timeout={timeout}, unviable={counts.get(UNVIABLE, 0)})"
    )
    return 0 if percent >= args.threshold else 1


if __name__ == "__main__":
    raise SystemExit(main())
