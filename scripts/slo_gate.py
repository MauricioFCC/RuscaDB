"""Gates estadísticos SLO: Mann-Whitney, bootstrap pareado y A/A (SPEC-0082).

Sustituye gates de ratio fijo por tests de hipótesis (frontera ASV/ASE'26):
los runners de GitHub tienen CV ≈ 2.7 %, así que los umbrales fijos producen
falsos positivos. Uso:

    python scripts/slo_gate.py fixture.json
    python scripts/slo_gate.py --criterion-pair <base_dir> <head_dir> --n 30

Formato del fixture JSON (modo directo)::

    {
      "mode": "mann-whitney" | "bootstrap" | "aa",
      "metric": "wal_commit_p50",
      "direction": "lower_is_better" | "higher_is_better",
      "baseline":  [..valores..],
      "candidate": [..valores..]
    }

Modo ``--criterion-pair``: lee ``estimates.json`` de criterion (media,
desviación típica) de dos corridas (base del PR vs HEAD) y aplica un test z
de Welch direccional sobre cada par de benches con nombre coincidente.

Códigos de salida (contrato SPEC-0079/0061):
- ``0`` = sin regresión significativa (p >= 0.002 en todos los tests).
- ``1`` = regresión significativa (nombra la métrica infractora).
- ``2`` = entrada malformada / baseline ausente (fallo ruidoso, nunca verde).

Solo stdlib (statistics, math, random con semilla fija, json).
"""

from __future__ import annotations

import json
import math
import random
import statistics
import sys
from pathlib import Path

# Umbral p < 0.002 ≈ Bonferroni para ~25 métricas (research T6, roadmap §3).
P_THRESHOLD = 0.002
# Remuestreos del bootstrap pareado y semilla fija (reproducibilidad).
BOOTSTRAP_ITERATIONS = 10_000
BOOTSTRAP_SEED = 20_261_013


def _fail_ruidoso(motivo: str) -> "int":
    """Imprime el motivo y devuelve el código de fallo ruidoso.

    Args:
        motivo: Explicación WHAT+WHY+WHERE del problema de entrada.

    Returns:
        El código de salida ``2``.
    """
    print(f"slo_gate: entrada inválida — {motivo}", file=sys.stderr)
    return 2


def mann_whitney_one_sided(
    baseline: list[float], candidate: list[float], direction: str
) -> float:
    """Calcula p unilateral de que el candidato sea PEOR que la base.

    Test de Mann-Whitney U con aproximación normal, continuidad y corrección
    por empates (fórmulas estándar: U sobre el candidato, media n1·n2/2,
    varianza n1·n2·(n+1)/12 con término de empates).

    Args:
        baseline: Muestras de la línea base.
        candidate: Muestras del candidato.
        direction: ``lower_is_better`` (latencia) o ``higher_is_better``
            (recall).

    Returns:
        p unilateral P(candidato peor | H0); pequeña => regresión.
    """
    n1, n2 = len(candidate), len(baseline)
    combined = candidate + baseline
    # Rangos con promedio en empates.
    ordered = sorted(combined)
    ranks: dict[float, float] = {}
    i = 0
    while i < len(ordered):
        j = i
        while j + 1 < len(ordered) and ordered[j + 1] == ordered[i]:
            j += 1
        ranks[ordered[i]] = (i + j) / 2 + 1  # rangos 1-based promediados
        i = j + 1
    r1 = sum(ranks[v] for v in candidate)
    u1 = r1 - n1 * (n1 + 1) / 2

    # Corrección por empates (t = ocurrencias de cada valor repetido).
    tie_term = 0.0
    for value in set(combined):
        t = combined.count(value)
        if t > 1:
            tie_term += t**3 - t
    n = n1 + n2
    var = n1 * n2 / 12 * ((n + 1) - tie_term / (n * (n - 1)) if n > 1 else 0.0)
    if var <= 0:
        return 0.5  # degenerado (todas las muestras idénticas): sin evidencia
    mean_u = n1 * n2 / 2
    # Continuidad en dirección "peor": lower_is_better => peor = valores más
    # altos del candidato => U1 grande es malo; higher_is_better => U1 chico
    # es malo (se refleja el z).
    if direction == "lower_is_better":
        z = (u1 - mean_u - 0.5) / math.sqrt(var)
    else:
        z = (mean_u - u1 - 0.5) / math.sqrt(var)
    # p unilateral en cola "peor".
    return 0.5 * math.erfc(z / math.sqrt(2))


def bootstrap_paired_regression(
    baseline: list[float],
    candidate: list[float],
    direction: str,
    iterations: int = BOOTSTRAP_ITERATIONS,
    seed: int = BOOTSTRAP_SEED,
) -> tuple[bool, float]:
    """Gate de regresión con bootstrap pareado (CI 99.8 % = p ≈ 0.002).

    Remuestrea índices con reemplazo sobre los pares (mismas consultas),
    calcula la media de la diferencia candidato-base y obtiene el intervalo
    por percentiles en 0.002/0.998. Hay regresión si TODO el intervalo cao
    del lado "peor" de cero.

    Args:
        baseline: Métrica base emparejada por consulta.
        candidate: Métrica candidata emparejada (mismo orden).
        direction: ``lower_is_better`` o ``higher_is_better``.
        iterations: Número de remuestreos.
        seed: Semilla fija (reproducibilidad).

    Returns:
        Tupla ``(hay_regresion, p_aproximado)``.
    """
    diffs = [c - b for c, b in zip(candidate, baseline)]
    n = len(diffs)
    rng = random.Random(seed)
    means = []
    for _ in range(iterations):
        total = 0.0
        for _ in range(n):
            total += diffs[rng.randrange(n)]
        means.append(total / n)
    means.sort()
    low = means[int(0.002 * (iterations - 1))]
    high = means[int(0.998 * (iterations - 1))]
    if direction == "lower_is_better":
        regression = low > 0
        # p bootstrap unilateral: fracción de remuestreos sin diferencia
        # "peor" (<= 0), con corrección +1.
        p = (sum(1 for m in means if m <= 0) + 1) / (iterations + 1)
    else:
        regression = high < 0
        p = (sum(1 for m in means if m >= 0) + 1) / (iterations + 1)
    return regression, p


def _cargar_fixture(path: Path) -> dict:
    """Carga y valida un fixture JSON del gate.

    Args:
        path: Ruta al fixture.

    Raises:
        ValueError: Si el JSON caree de claves requeridas, tiene arrays
            vacíos, direcciones desconocidas o modos inválidos.
    """
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"no se pudo leer {path}: {exc}") from exc
    for key in ("mode", "metric", "direction"):
        if key not in data:
            raise ValueError(f"falta la clave '{key}' en {path}")
    if data["mode"] not in ("mann-whitney", "bootstrap", "aa"):
        raise ValueError(f"modo desconocido {data['mode']!r} en {path}")
    if data["direction"] not in ("lower_is_better", "higher_is_better"):
        raise ValueError(f"dirección desconocida {data['direction']!r} en {path}")
    if data["mode"] == "aa":
        # A/A: dos muestras de la MISMA distribución en las dos claves.
        data.setdefault("candidate", data.get("baseline"))
    for key in ("baseline", "candidate"):
        values = data.get(key)
        if not isinstance(values, list) or len(values) < 2:
            raise ValueError(f"'{key}' debe ser una lista con >= 2 valores en {path}")
        if not all(isinstance(v, (int, float)) for v in values):
            raise ValueError(f"'{key}' contiene no-numéricos en {path}")
    return data


def correr_fixture(path: Path) -> int:
    """Ejecuta el gate sobre un fixture JSON directo.

    Args:
        path: Ruta del fixture.

    Returns:
        0 = pasa, 1 = regresión significativa, 2 = entrada malformada.
    """
    try:
        data = _cargar_fixture(path)
    except ValueError as exc:
        return _fail_ruidoso(str(exc))
    metric = data["metric"]
    direction = data["direction"]
    baseline = [float(v) for v in data["baseline"]]
    candidate = [float(v) for v in data["candidate"]]
    mode = data["mode"]

    if mode in ("mann-whitney", "aa"):
        p = mann_whitney_one_sided(baseline, candidate, direction)
        etiqueta = "A/A (calibración)" if mode == "aa" else "Mann-Whitney"
        if p < P_THRESHOLD:
            print(
                f"slo_gate: REGRESIÓN [{etiqueta}] metric={metric} "
                f"p={p:.5f} < {P_THRESHOLD}",
                file=sys.stderr,
            )
            return 1
        print(f"slo_gate: OK [{etiqueta}] metric={metric} p={p:.5f}")
        return 0

    # bootstrap pareado
    hay, p = bootstrap_paired_regression(baseline, candidate, direction)
    if hay:
        print(
            f"slo_gate: REGRESIÓN [bootstrap pareado] metric={metric} "
            f"p≈{p:.5f} < {P_THRESHOLD}",
            file=sys.stderr,
        )
        return 1
    print(f"slo_gate: OK [bootstrap pareado] metric={metric} p≈{p:.5f}")
    return 0


def _leer_estimates(path: Path) -> tuple[float, float]:
    """Extrae (media, desviación típica) de un estimates.json de criterion.

    Args:
        path: Ruta a ``estimates.json``.

    Returns:
        Tupla ``(mean, std_dev)`` con los point estimates.

    Raises:
        ValueError: Si el JSON no trae media o desviación usables.
    """
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        mean = float(data["mean"]["point_estimate"])
        std = float(data["std_dev"]["point_estimate"])
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        raise ValueError(f"estimates ilegibles en {path}: {exc}") from exc
    if std <= 0:
        raise ValueError(f"desviación típica no positiva en {path}")
    return mean, std


def correr_criterion_pair(base_dir: Path, head_dir: Path, n: int) -> int:
    """Compara dos corridas de criterion (base vs HEAD) con test z de Welch.

    Criterion solo persiste resúmenes (media/desviación), no las muestras
    crudas, así que con ``n`` conocido (``--sample-size`` de la corrida) el
    test direccional sobre medias es el equivalente válido (Welch).

    Args:
        base_dir: Directorio con los ``estimates.json`` de la base.
        head_dir: Directorio con los de HEAD.
        n: Tamaño de muestra por corrida (``--sample-size``).

    Returns:
        0 = sin regresión, 1 = regresión (nombra el bench), 2 = faltan datos.
    """
    if n < 2:
        return _fail_ruidoso("--n debe ser >= 2")
    base_files = sorted(base_dir.rglob("estimates.json"))
    if not base_files:
        return _fail_ruidoso(f"sin estimates.json bajo {base_dir} (¿corrida base?)")
    comparados = 0
    for base_file in base_files:
        rel = base_file.relative_to(base_dir)
        head_file = head_dir / rel
        if not head_file.exists():
            return _fail_ruidoso(f"falta {head_file} (par incompleto)")
        try:
            b_mean, b_std = _leer_estimates(base_file)
            h_mean, h_std = _leer_estimates(head_file)
        except ValueError as exc:
            return _fail_ruidoso(str(exc))
        # Test z de Welch unilateral: la latencia (lower_is_better) empeora
        # cuando HEAD > base.
        se = math.sqrt(b_std**2 / n + h_std**2 / n)
        if se <= 0:
            continue
        z = (h_mean - b_mean) / se
        p = 0.5 * math.erfc(z / math.sqrt(2))
        bench = str(rel.parent)
        if p < P_THRESHOLD:
            print(
                f"slo_gate: REGRESIÓN [welch] bench={bench} "
                f"base={b_mean:.4g} head={h_mean:.4g} p={p:.5f}",
                file=sys.stderr,
            )
            return 1
        print(f"slo_gate: OK [welch] bench={bench} p={p:.5f}")
        comparados += 1
    if comparados == 0:
        return _fail_ruidoso("ningún par comparable")
    return 0


def main(argv: list[str]) -> int:
    """Punto de entrada CLI.

    Fuerza UTF-8 en stdout/stderr (código ES-UTF8): sin esto, en Windows con
    consola cp1252 el propio ``print`` lanza ``UnicodeEncodeError`` y el gate
    sale con código distinto al previsto (falso regresión).

    Args:
        argv: Argumentos sin el nombre del programa.

    Returns:
        Código de salida 0/1/2 según el contrato del gate.
    """
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    if argv[0] == "--help":
        print(__doc__)
        return 0
    if argv[0] == "--criterion-pair":
        if len(argv) < 4:
            return _fail_ruidoso(
                "uso: --criterion-pair <base_dir> <head_dir> --n <muestra>"
            )
        try:
            n_arg = argv[argv.index("--n") + 1] if "--n" in argv else ""
            n = int(n_arg)
        except (ValueError, IndexError):
            return _fail_ruidoso("--n inválido")
        return correr_criterion_pair(Path(argv[1]), Path(argv[2]), n)
    return correr_fixture(Path(argv[0]))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
