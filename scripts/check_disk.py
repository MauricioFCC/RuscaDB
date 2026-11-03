"""Guard de espacio libre para desarrollo local (RuscaDB).

`target/` puede crecer a decenas de GiB (perfiles, features, mutantes); un disco
lleno rompe el linker con `LNK1318`/`No space left on device`. Este guard falla
antes de compilar si el volumen del workspace no tiene margen suficiente.

Uso:
    python scripts/check_disk.py --min-free-gb 10
    python scripts/check_disk.py --min-free-gb 5 --json

Salidas:
    0 si hay espacio suficiente; 1 si está por debajo; 2 si los argumentos o la
    ruta son inválidos.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import shutil
import sys

_DEFAULT_MIN_FREE_GB = 10.0


def check(path: pathlib.Path, min_free_gb: float) -> dict:
    """Calcula el espacio libre del volumen que contiene ``path``.

    Args:
        path: Ruta existente dentro del volumen a comprobar.
        min_free_gb: Umbral mínimo en GiB (>= 0).

    Returns:
        Diccionario con ``path``, ``free_gb``, ``min_free_gb`` y ``ok``.

    Raises:
        ValueError: Si el umbral es negativo.
        FileNotFoundError: Si ``path`` no existe.
    """
    if min_free_gb < 0:
        raise ValueError("--min-free-gb no puede ser negativo")
    if not path.exists():
        raise FileNotFoundError(f"ruta inexistente: {path}")
    usage = shutil.disk_usage(path)
    free_gb = usage.free / (1024**3)
    return {
        "path": str(path),
        "free_gb": round(free_gb, 2),
        "min_free_gb": min_free_gb,
        "ok": free_gb >= min_free_gb,
    }


def main(argv: list[str] | None = None) -> int:
    """Punto de entrada del guard.

    Args:
        argv: Argumentos de línea de comandos (sin el nombre del script).

    Returns:
        Código de salida del proceso.
    """
    parser = argparse.ArgumentParser(description="Comprueba espacio libre en disco.")
    parser.add_argument("--min-free-gb", type=float, default=_DEFAULT_MIN_FREE_GB)
    parser.add_argument("--path", default=".")
    parser.add_argument("--json", action="store_true", help="Salida JSON.")
    args = parser.parse_args(argv)

    try:
        report = check(pathlib.Path(args.path), args.min_free_gb)
    except (ValueError, FileNotFoundError) as error:
        print(f"[FAIL] check_disk: {error}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(report, sort_keys=True))
    elif report["ok"]:
        print(
            f"[OK] disco: {report['free_gb']:.2f} GiB libres "
            f">= {report['min_free_gb']:.2f} GiB"
        )
    else:
        print(
            f"[FAIL] disco: {report['free_gb']:.2f} GiB libres "
            f"< {report['min_free_gb']:.2f} GiB; ejecuta `cargo xtask gc --apply --all`",
            file=sys.stderr,
        )
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
