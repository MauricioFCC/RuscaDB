"""Retrofit: inserta `// @spec AC-XXXX-NN` sobre cada `fn test_ac_XXXX_NN_*` sin marcador.

Uso: python scripts/retrofit_spec_markers.py [--check]
Uso interno del loop (SPEC-0061); idempotente.
"""

import pathlib
import re
import sys

RX = re.compile(r"^(\s*)fn (test_ac_(\d+)_(\d+)_[A-Za-z0-9_]+)")
ROOT = pathlib.Path(__file__).resolve().parent.parent


def process(check: bool = False) -> int:
    """Inserta marcadores ausentes.

    Args:
        check: Si es True, no escribe y devuelve el conteo de ausentes.

    Returns:
        Número de marcadores ausentes (insertados o por insertar).
    """
    missing = 0
    for path in sorted(ROOT.joinpath("crates").rglob("*.rs")):
        lines = path.read_text(encoding="utf-8").split("\n")
        out: list[str] = []
        dirty = False
        for i, line in enumerate(lines):
            match = RX.match(line)
            if match:
                context = "\n".join(lines[max(0, i - 4) : i])
                if "@spec" not in context:
                    marker = f"{match.group(1)}// @spec AC-{match.group(3)}-{match.group(4)}"
                    out.append(marker)
                    dirty = True
                    missing += 1
            out.append(line)
        if dirty and not check:
            path.write_text("\n".join(out), encoding="utf-8")
    return missing


if __name__ == "__main__":
    check_mode = "--check" in sys.argv[1:]
    count = process(check_mode)
    print(f"spec_markers_missing={count}")
    if check_mode and count > 0:
        sys.exit(1)
