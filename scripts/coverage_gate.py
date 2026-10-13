"""Gate de cobertura multi-scope sobre lcov.info (SPEC-0079; base SPEC-0061).

Uso en CI (job `coverage`):

    cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
    python scripts/coverage_gate.py lcov.info \\
        --scope ruscadb-core/ --min-line 80 --min-branch 70 \\
        --scope ruscadb-txn/  --min-line <X> --min-branch <Y> \\
        --scope ruscadb-wal/  --min-line <X> --min-branch <Y>

Cada `--scope` abre un scope nuevo con su propio par `--min-line`/`--min-branch`
(los flags de umbral afectan al último `--scope` declarado). Con un solo scope la
invocación es idéntica a SPEC-0061:

    python scripts/coverage_gate.py lcov.info --scope ruscadb-core/ \\
        --min-line 80 --min-branch 70

Reglas:

- Agrega registros DA (líneas) y BRDA (ramas) por ruta de fichero y reporta el
  % de línea/rama **por crate** dentro de cada scope, para ver qué crate
  arrastra la cobertura.
- Sin registros BRDA => rama n/a => pasa (no hay ramas que medir).
- Scope sin ficheros o lcov vacío => FAIL ruidoso (exit 2), nunca verde
  silencioso.

Códigos de salida:

- ``0`` = todos los scopes cumplen sus umbrales.
- ``1`` = algún scope incumple su umbral (nombra el scope infractor).
- ``2`` = error de entrada (lcov inexistente/vacío o scope sin ficheros).

Solo stdlib de Python.
"""

import sys

DEFAULT_SCOPE = "ruscadb-core/"
DEFAULT_MIN_LINE = 80.0
DEFAULT_MIN_BRANCH = 70.0
CRATE_MARKER = "crates/"

USAGE = """usage: coverage_gate.py [lcov.info] [--scope S --min-line L --min-branch B]...

Gate de cobertura multi-scope sobre lcov.info (SPEC-0079).

Cada `--scope` declara un scope con su propio umbral. Los flags `--min-line` y
`--min-branch` afectan al último `--scope` declarado; sin `--scope` se usa el
default `ruscadb-core/` con 80/70.

Ejemplos:
  coverage_gate.py lcov.info --scope ruscadb-core/ --min-line 80 --min-branch 70
  coverage_gate.py lcov.info \\
      --scope ruscadb-core/ --min-line 80 --min-branch 70 \\
      --scope ruscadb-txn/  --min-line 60 --min-branch 50 \\
      --scope ruscadb-wal/  --min-line 60 --min-branch 50

Salida: exit 0 todos cumplen; exit 1 algún scope bajo umbral (nombra el scope);
exit 2 lcov inexistente/vacío o scope sin ficheros (fallo ruidoso).
"""


def parse_lcov_records(text):
    """Agrega registros DA/BRDA por ruta de fichero (núcleo testeable, sin E/S).

    Args:
        text: Contenido del lcov.info.

    Returns:
        `dict` ruta -> `[line_tot, line_hit, br_tot, br_hit]`. Los DA cuentan
        líneas; los BRDA cuentan ramas (una rama cuenta como cubierta si el
        campo *taken* no es ``-`` ni ``0``).
    """
    records = {}
    current = None
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("SF:"):
            current = line[3:]
            records.setdefault(current, [0, 0, 0, 0])
        elif current is not None and line.startswith("DA:"):
            parts = line[3:].split(",")
            if len(parts) >= 2:
                record = records[current]
                record[0] += 1
                if parts[1].strip() not in ("0", ""):
                    record[1] += 1
        elif current is not None and line.startswith("BRDA:"):
            parts = line[5:].split(",")
            if len(parts) >= 4:
                record = records[current]
                record[2] += 1
                if parts[3].strip() not in ("-", "0", ""):
                    record[3] += 1
    return records


def crate_of(path, scope):
    """Deriva el nombre del crate de una ruta lcov.

    Args:
        path: Ruta del fichero tal como aparece en `SF:`.
        scope: Scope en curso (fallback si la ruta no sigue el layout).

    Returns:
        Nombre del crate (segmento tras `crates/`), o el scope normalizado si
        la ruta no contiene `crates/`.
    """
    normalized = path.replace("\\", "/")
    index = normalized.rfind(CRATE_MARKER)
    if index == -1:
        return scope.strip("/") or "?"
    rest = normalized[index + len(CRATE_MARKER):]
    name = rest.split("/", 1)[0]
    return name or "?"


def select_scope(records, scope):
    """Filtra los ficheros cuyo path contiene el scope.

    Args:
        records: Registros lcov por ruta.
        scope: Subcadena que deben contener los paths del scope.

    Returns:
        Subconjunto de `records` perteneciente al scope.
    """
    return {path: rec for path, rec in records.items() if scope in path}


def aggregate(group):
    """Suma `[line_tot, line_hit, br_tot, br_hit]` de un grupo de registros.

    Args:
        group: Mapa ruta -> `[line_tot, line_hit, br_tot, br_hit]`.

    Returns:
        Lista `[line_tot, line_hit, br_tot, br_hit]` agregada.
    """
    totals = [0, 0, 0, 0]
    for record in group.values():
        for index in range(4):
            totals[index] += record[index]
    return totals


def group_by_crate(records, scope):
    """Agrupa los registros de un scope por nombre de crate.

    Args:
        records: Registros de un único scope.
        scope: Scope en curso (para derivar el crate de rutas atípicas).

    Returns:
        `dict` nombre de crate -> `[line_tot, line_hit, br_tot, br_hit]`.
    """
    groups = {}
    for path, record in records.items():
        name = crate_of(path, scope)
        bucket = groups.setdefault(name, [0, 0, 0, 0])
        for index in range(4):
            bucket[index] += record[index]
    return groups


def percent(hit, total):
    """Porcentaje 0-100 de aciertos.

    Args:
        hit: Aciertos (líneas o ramas cubiertas).
        total: Total medido.

    Returns:
        `100.0` si no hay total (nada que medir), o `100 * hit / total`.
    """
    return 100.0 if total == 0 else 100.0 * hit / total


def format_coverage(totals):
    """Formatea línea y rama de un agregado `[lt, lh, bt, bh]`.

    Args:
        totals: `[line_tot, line_hit, br_tot, br_hit]`.

    Returns:
        Texto legible, con `rama=n/a (sin BRDA)` cuando no hay ramas.
    """
    line_tot, line_hit, br_tot, br_hit = totals
    text = f"linea={percent(line_hit, line_tot):.1f}% ({line_hit}/{line_tot})"
    if br_tot == 0:
        return text + "; rama=n/a (sin BRDA)"
    return text + f"; rama={percent(br_hit, br_tot):.1f}% ({br_hit}/{br_tot})"


def parse_args(argv):
    """Parsea los argumentos del gate.

    Args:
        argv: Lista de argumentos (incluye el nombre del script en `[0]`).

    Returns:
        `(path, scopes)`: ruta del lcov y lista de dicts con
        `{"scope", "min_line", "min_branch"}`. Sin `--scope` se usa el default
        mono-scope de SPEC-0061 (`ruscadb-core/` con 80/70).
    """
    path = "lcov.info"
    scopes = []
    pending_line = None
    pending_branch = None
    positional = [arg for arg in argv[1:] if not arg.startswith("--")]
    if positional:
        path = positional[0]
    index = 1
    while index < len(argv):
        arg = argv[index]
        if arg == "--scope" and index + 1 < len(argv):
            scopes.append(
                {
                    "scope": argv[index + 1],
                    "min_line": DEFAULT_MIN_LINE,
                    "min_branch": DEFAULT_MIN_BRANCH,
                }
            )
            index += 2
        elif arg == "--min-line" and index + 1 < len(argv):
            value = float(argv[index + 1])
            if scopes:
                scopes[-1]["min_line"] = value
            else:
                pending_line = value
            index += 2
        elif arg == "--min-branch" and index + 1 < len(argv):
            value = float(argv[index + 1])
            if scopes:
                scopes[-1]["min_branch"] = value
            else:
                pending_branch = value
            index += 2
        else:
            index += 1
    if not scopes:
        scopes.append(
            {
                "scope": DEFAULT_SCOPE,
                "min_line": pending_line if pending_line is not None else DEFAULT_MIN_LINE,
                "min_branch": (
                    pending_branch if pending_branch is not None else DEFAULT_MIN_BRANCH
                ),
            }
        )
    return path, scopes


def evaluate_scope(entry, records):
    """Evalúa un scope: imprime su cobertura per-crate y decide si cumple.

    Args:
        entry: Dict con `scope`, `min_line` y `min_branch`.
        records: Registros lcov agregados por ruta.

    Returns:
        `(ok, empty)`: `ok` es `True` si el scope cumple sus umbrales; `empty`
        es `True` si el scope no tiene ficheros o líneas (fallo ruidoso).
    """
    scope = entry["scope"]
    scoped = select_scope(records, scope)
    totals = aggregate(scoped)
    if not scoped or totals[0] == 0:
        return True, True
    print(f"gate cobertura [{scope}]: {format_coverage(totals)}")
    for name, bucket in sorted(group_by_crate(scoped, scope).items()):
        print(f"    - crate {name}: {format_coverage(bucket)}")
    line_cov = percent(totals[1], totals[0])
    if totals[2] == 0:
        branch_ok = True
        branch_note = "rama=n/a"
    else:
        branch_cov = percent(totals[3], totals[2])
        branch_ok = branch_cov >= entry["min_branch"]
        branch_note = f"rama={branch_cov:.1f}%/{entry['min_branch']}"
    ok = line_cov >= entry["min_line"] and branch_ok
    if not ok:
        print(
            f"gate cobertura: scope '{scope}' bajo umbral "
            f"(linea={line_cov:.1f}%/{entry['min_line']}, {branch_note}) FAIL"
        )
    return ok, False


def main(argv):
    """Punto de entrada del gate.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en `[0]`).

    Returns:
        Exit code: `0` PASS, `1` algún scope bajo umbral, `2` error de entrada.
    """
    if "--help" in argv or "-h" in argv:
        print(USAGE)
        return 0
    path, scopes = parse_args(argv)
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except FileNotFoundError:
        print(f"gate cobertura: {path} no existe (sin corrida llvm-cov)")
        return 2
    records = parse_lcov_records(text)
    all_ok = True
    for entry in scopes:
        ok, empty = evaluate_scope(entry, records)
        if empty:
            print(
                f"gate cobertura: sin datos para el scope '{entry['scope']}' "
                f"(lcov vacio o scope sin ficheros)"
            )
            return 2
        all_ok = all_ok and ok
    if all_ok:
        print("gate cobertura: PASS")
        return 0
    print("gate cobertura: FAIL (al menos un scope bajo su umbral)")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
