"""Gate de cobertura sobre lcov.info (SLO roadmap: core linea >= 80 %, rama >= 70 %).

Uso en CI (job `coverage`):
    cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
    python scripts/coverage_gate.py lcov.info --scope ruscadb-core/ \\
        --min-line 80 --min-branch 70

Reglas:
- Agrega registros DA (lineas) y BRDA (ramas) de los ficheros cuyo path
  contiene `--scope`.
- Sin registros BRDA => rama n/a => pasa (no hay ramas que medir).
- Sin ficheros en el scope o lcov vacio => FAIL ruidoso (exit 2), nunca
  verde silencioso.

Uso interno del loop (SPEC-0061); solo stdlib.
"""

import sys


def parse_lcov_text(text, scope):
    """Núcleo testeable del parseo (sin E/S).

    Args:
        text: Contenido del lcov.info.
        scope: Subcadena de los ficheros a agregar.

    Returns:
        `(lineas_tot, lineas_hit, ramas_tot, ramas_hit, ficheros)`.
    """
    line_tot = line_hit = br_tot = br_hit = 0
    files = set()
    in_scope = False
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("SF:"):
            in_scope = scope in line[3:]
            if in_scope:
                files.add(line[3:])
        elif in_scope and line.startswith("DA:"):
            parts = line[3:].split(",")
            if len(parts) >= 2:
                line_tot += 1
                if parts[1].strip() not in ("0", ""):
                    line_hit += 1
        elif in_scope and line.startswith("BRDA:"):
            parts = line[5:].split(",")
            if len(parts) >= 4:
                br_tot += 1
                if parts[3].strip() not in ("-", "0", ""):
                    br_hit += 1
    return line_tot, line_hit, br_tot, br_hit, len(files)


def main(argv):
    """Punto de entrada del gate.

    Args:
        argv: `[lcov, --scope S, --min-line L, --min-branch B]`.

    Returns:
        Exit code (0 PASS, 1 bajo umbral, 2 error).
    """
    path = "lcov.info"
    scope = "ruscadb-core/"
    min_line = 80.0
    min_branch = 70.0
    positional = [a for a in argv[1:] if not a.startswith("--")]
    if positional:
        path = positional[0]
    for i, arg in enumerate(argv):
        if arg == "--scope" and i + 1 < len(argv):
            scope = argv[i + 1]
        elif arg == "--min-line" and i + 1 < len(argv):
            min_line = float(argv[i + 1])
        elif arg == "--min-branch" and i + 1 < len(argv):
            min_branch = float(argv[i + 1])
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except FileNotFoundError:
        print(f"gate cobertura: {path} no existe (sin corrida llvm-cov)")
        return 2
    line_tot, line_hit, br_tot, br_hit, files = parse_lcov_text(text, scope)
    if files == 0 or line_tot == 0:
        print(f"gate cobertura: sin datos para el scope '{scope}'")
        return 2
    line_cov = 100.0 * line_hit / line_tot
    print(f"gate cobertura [{scope}]: linea={line_cov:.1f}% ({line_hit}/{line_tot})", end="")
    ok = True
    if line_cov < min_line:
        print(f" < {min_line} FAIL", end="")
        ok = False
    else:
        print(f" >= {min_line} OK", end="")
    if br_tot == 0:
        print("; rama=n/a (sin BRDA) OK")
    else:
        br_cov = 100.0 * br_hit / br_tot
        print(f"; rama={br_cov:.1f}% ({br_hit}/{br_tot})", end="")
        if br_cov < min_branch:
            print(f" < {min_branch} FAIL")
            ok = False
        else:
            print(f" >= {min_branch} OK")
    if ok:
        print("gate cobertura: PASS")
        return 0
    print("gate cobertura: FAIL bajo el umbral (subir cobertura del core)")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
