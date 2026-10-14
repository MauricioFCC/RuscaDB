"""Gate de mutación sobre el diff del PR (MS_diff >= 70 %, SPEC-0061).

Uso en CI (job `mutation-diff`):
    cargo mutants --in-diff pr.diff --in-place --baseline=skip -o mutants.out
    python scripts/mutation_diff_gate.py mutants.out/outcomes.json --min-score 70

Reglas:
- Score = 100 * (Caught + Timeout) / (Caught + Timeout + Missed).
  Timeout cuenta como matado (el test colgó: el mutante es detectable).
  Unviable se excluye (no es código testeable).
- Sin mutantes puntuables (diff sin Rust o todo unviable) => PASS con nota.
- Esquema desconocido => FAIL ruidoso (exit 2) para actualizar el script,
  nunca verde silencioso.

Uso interno del loop (SPEC-0061); solo stdlib.
"""

import json
import sys

# Estados conocidos de cargo-mutants (outcomes.json -> summary).
CAUGHT = {"caught"}
TIMEOUT = {"timeout"}
MISSED = {"missed"}
EXCLUDED = {"unviable", "ignored", "skipped", "success"}
KNOWN = CAUGHT | TIMEOUT | MISSED | EXCLUDED
# cargo-mutants >= v25 serializa `summary` como cadena (p. ej. "CaughtMutant").
CAUGHT_PREFIX = "caught"
TIMEOUT_PREFIX = "timeout"
MISSED_PREFIX = "missed"
EXCLUDED_PREFIXES = ("unviable", "ignored", "skipped", "success")


def collect_summaries(node, found):
    """Recolecta los `summary` del JSON (dict legacy o cadena v25+).

    Args:
        node: Subárbol JSON actual.
        found: Lista donde se acumulan los `summary` hallados.
    """
    if isinstance(node, dict):
        if "summary" in node:
            found.append(node["summary"])
        for value in node.values():
            collect_summaries(value, found)
    elif isinstance(node, list):
        for value in node:
            collect_summaries(value, found)


def classify(summary):
    """Clasifica un `summary` de cargo-mutants.

    Args:
        summary: Valor de `summary`: dict legacy (`{"caught": N, ...}`) o
            cadena (`"CaughtMutant"`, `"MissedMutant"`, `"Timeout"`, ...).

    Returns:
        `(caught, missed, unknown)`: conteos y estados no reconocidos.
    """
    if isinstance(summary, dict):
        caught = missed = 0
        unknown = []
        for status, count in summary.items():
            key = str(status).lower()
            if key in CAUGHT or key in TIMEOUT:
                caught += int(count)
            elif key in MISSED:
                missed += int(count)
            elif key in EXCLUDED:
                continue
            else:
                unknown.append(str(status))
        return caught, missed, unknown
    if isinstance(summary, str):
        key = summary.strip().lower()
        if key.startswith(CAUGHT_PREFIX) or key.startswith(TIMEOUT_PREFIX):
            return 1, 0, []
        if key.startswith(MISSED_PREFIX):
            return 0, 1, []
        if any(key.startswith(prefix) for prefix in EXCLUDED_PREFIXES):
            return 0, 0, []
        return 0, 0, [summary]
    return 0, 0, [repr(summary)]


def score_outcomes(outcomes_path):
    """Calcula el mutation score del diff.

    Args:
        outcomes_path: Ruta a `outcomes.json` de cargo-mutants.

    Returns:
        `(scored, caught, missed, unknown_keys)`: mutantes puntuables,
        matados, vivos y estados no reconocidos.
    """
    with open(outcomes_path, encoding="utf-8") as handle:
        data = json.load(handle)
    summaries: list = []
    collect_summaries(data, summaries)
    caught = missed = 0
    unknown = set()
    for summary in summaries:
        item_caught, item_missed, item_unknown = classify(summary)
        caught += item_caught
        missed += item_missed
        unknown.update(item_unknown)
    return caught + missed, caught, missed, sorted(unknown)


def main(argv):
    """Punto de entrada del gate.

    Args:
        argv: `[outcomes.json, --min-score N]` (N = 70 por defecto).

    Returns:
        Exit code (0 PASS, 1 FAIL bajo umbral, 2 error/esquema).
    """
    path = argv[1] if len(argv) > 1 else "mutants.out/outcomes.json"
    min_score = 70
    for i, arg in enumerate(argv):
        if arg == "--min-score" and i + 1 < len(argv):
            min_score = int(argv[i + 1])
    try:
        scored, caught, missed, unknown = score_outcomes(path)
    except FileNotFoundError:
        print(f"gate mutacion: {path} no existe (sin corrida de mutantes)")
        return 2
    except json.JSONDecodeError as error:
        print(f"gate mutacion: JSON invalido en {path}: {error}")
        return 2
    if unknown:
        print(f"gate mutacion: estados no reconocidos {unknown} (actualizar script)")
        return 2
    if scored == 0:
        print("gate mutacion: PASS sin mutantes puntuables en el diff")
        return 0
    score = 100.0 * caught / scored
    print(f"gate mutacion: MS_diff={score:.1f} ({caught}/{scored}, minimo {min_score})")
    if score < min_score:
        print("gate mutacion: FAIL bajo el umbral (endurecer tests del diff)")
        return 1
    print("gate mutacion: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
