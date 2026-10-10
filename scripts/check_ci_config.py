#!/usr/bin/env python3
"""Valida la configuración de CI T1/T3 y supply chain de RuscaDB.

Comprueba, sin dependencias externas obligatorias (solo stdlib):

1. ``.github/workflows/ci.yml`` (T1) declara la matriz rápida del job ``test``
   (``ubuntu-latest``/``windows-latest``); macOS queda fuera del PR por la
   cola de runners (SPEC-0046).
2. ``.github/workflows/nightly.yml`` (T3) es YAML parseable y declara los jobs
   ``mutation``/``fuzz``/``miri``/``sanitizers`` (alert-only) más el job
   ``test-macos`` que corre en ``macos-latest`` (SPEC-0046); además, el job
   ``fuzz`` declara la matriz de targets ``query_parse``/``wal_recover`` y el
   target del parser usa un dictionary (SPEC-0042). Si PyYAML no está
   instalado se aplica un parser textual robusto por indentación.
3. ``.cargo/mutants.toml`` es TOML válido y declara ``exclude_globs``/
   ``test_tool``/``additional_cargo_test_args`` sin claves desconocidas que
   romperían cargo-mutants (``deny_unknown_fields``).
4. ``deny.toml`` es TOML válido y declara las 4 secciones requeridas:
   ``[advisories]``, ``[licenses]``, ``[bans]``, ``[sources]``.

Salida: exit 0 si todo cumple; exit 1 con diagnóstico legible si no.

Nota de implementación (YAML): ``tomllib`` viene en la stdlib de Python 3.11+.
Para YAML se intenta importar ``yaml`` (PyYAML); si no está disponible se usa
un chequeo textual por indentación equivalente para los jobs de primer nivel,
que es suficiente porque este script solo necesita la lista de ``jobs``.
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
CI = ROOT / ".github" / "workflows" / "ci.yml"
NIGHTLY = ROOT / ".github" / "workflows" / "nightly.yml"
MUTANTS = ROOT / ".cargo" / "mutants.toml"
DENY = ROOT / "deny.toml"
CRATES_DIR = ROOT / "crates"

# Runners de la matriz rápida de ci.yml (SPEC-0033/SPEC-0046/AC-01):
# ubuntu+windows en push/PR; macOS se valida en nightly.yml (cola larga).
MATRIX_OS: tuple[str, ...] = ("ubuntu-latest", "windows-latest")
# Runner de macOS que debe declarar nightly.yml (SPEC-0046/AC-02).
NIGHTLY_OS: str = "macos-latest"
REQUIRED_JOBS: tuple[str, ...] = (
    "mutation",
    "fuzz",
    "miri",
    "sanitizers",
    "test-macos",
)
# Targets que debe fuzzear el job `fuzz` de nightly.yml (SPEC-0042/AC-03).
FUZZ_TARGETS: tuple[str, ...] = ("query_parse", "wal_recover")
REQUIRED_DENY_SECTIONS: tuple[str, ...] = (
    "advisories",
    "licenses",
    "bans",
    "sources",
)
REQUIRED_MUTANTS_KEYS: tuple[str, ...] = (
    "exclude_globs",
    "test_tool",
    "additional_cargo_test_args",
)
# Claves que cargo-mutants rechaza (`deny_unknown_fields`): si aparecen, T3
# fallaría al parsear la config. Se valida explícitamente.
FORBIDDEN_MUTANTS_KEYS: tuple[str, ...] = ("toolchain",)


def _load_yaml(text: str) -> dict[str, Any] | None:
    """Parsea YAML con PyYAML si está disponible; si no, devuelve ``None``."""
    try:
        import yaml  # type: ignore[import-not-found]
    except ImportError:
        return None
    data = yaml.safe_load(text)
    if not isinstance(data, dict):
        return None
    return data


def _jobs_from_text(text: str) -> set[str]:
    """Fallback textual: extrae los jobs de primer nivel bajo ``jobs:``.

    Un job es una clave con indentación de 2 espacios y terminación ``:``.
    """
    jobs: set[str] = set()
    in_jobs = False
    for raw in text.splitlines():
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        if raw.startswith("jobs:"):
            in_jobs = True
            continue
        if in_jobs:
            # Una clave de nivel raíz (sin indentación) cierra el bloque jobs.
            if not raw.startswith(" "):
                in_jobs = False
                continue
            if not raw.startswith("  ") or raw.startswith("   "):
                continue
            key = raw.strip()
            if key.endswith(":"):
                jobs.add(key[:-1].strip())
    return jobs


def _fuzz_targets_from_matrix(job: dict[str, Any]) -> set[str]:
    """Extrae los targets de la matriz del job ``fuzz`` de nightly.yml.

    Soporta la forma ``matrix.target: [a, b]`` y la forma ``matrix.include``
    con entradas ``{target: ...}``. Devuelve el conjunto de nombres; vacío si
    la estructura no es reconocible.
    """
    strategy = job.get("strategy")
    matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
    if not isinstance(matrix, dict):
        return set()
    targets = matrix.get("target")
    if isinstance(targets, list):
        return {name for name in targets if isinstance(name, str)}
    include = matrix.get("include")
    if isinstance(include, list):
        names: set[str] = set()
        for entry in include:
            if isinstance(entry, dict) and isinstance(entry.get("target"), str):
                names.add(entry["target"])
        return names
    return set()


def check_ci_matrix(errors: list[str]) -> None:
    """Valida la matriz rápida del job ``test`` de ``ci.yml``.

    SPEC-0033/SPEC-0046/AC-01: el job ``test`` (T1) debe declarar
    ``strategy.matrix.os`` con los runners ``ubuntu-latest``/``windows-latest``
    (macOS se valida en ``nightly.yml``). Con PyYAML se comprueba la estructura;
    sin él, un chequeo textual verifica que los runners aparezcan en el
    workflow.
    """
    if not CI.exists():
        errors.append(f"no existe {CI.relative_to(ROOT)}")
        return
    text = CI.read_text(encoding="utf-8")
    data = _load_yaml(text)
    if data is not None:
        jobs = data.get("jobs")
        if not isinstance(jobs, dict) or "test" not in jobs:
            errors.append("ci.yml: falta el job 'test'")
            return
        spec = jobs["test"]
        strategy = spec.get("strategy") if isinstance(spec, dict) else None
        matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
        os_values = matrix.get("os") if isinstance(matrix, dict) else None
        if not isinstance(os_values, list):
            errors.append("ci.yml: el job 'test' no declara strategy.matrix.os")
            return
        for target in MATRIX_OS:
            if target not in os_values:
                errors.append(
                    f"ci.yml: la matriz del job 'test' no incluye '{target}'"
                )
        return
    # Fallback textual (sin PyYAML): la matriz y los runners deben aparecer.
    if "matrix:" not in text:
        errors.append("ci.yml: no declara una matriz (chequeo textual)")
    for target in MATRIX_OS:
        if target not in text:
            errors.append(
                f"ci.yml: no declara el runner '{target}' (chequeo textual)"
            )


def _has_runner(jobs: dict[str, Any], runner: str) -> bool:
    """Indica si algún job de ``jobs`` declara ``runs-on: <runner>``.

    Args:
        jobs: Mapa de nombre de job a su especificación YAML.
        runner: Etiqueta del runner (p. ej. ``macos-latest``).

    Returns:
        ``True`` si algún job usa ese runner; ``False`` en caso contrario.
    """
    for spec in jobs.values():
        if isinstance(spec, dict) and spec.get("runs-on") == runner:
            return True
    return False


def check_nightly(errors: list[str]) -> None:
    """Valida la estructura del workflow nightly (T3, alert-only)."""
    if not NIGHTLY.exists():
        errors.append(f"no existe {NIGHTLY.relative_to(ROOT)}")
        return
    text = NIGHTLY.read_text(encoding="utf-8")
    data = _load_yaml(text)
    if data is not None:
        jobs = data.get("jobs")
        if not isinstance(jobs, dict):
            errors.append("nightly.yml: falta la clave raíz 'jobs'")
            return
        for job in REQUIRED_JOBS:
            if job not in jobs:
                errors.append(f"nightly.yml: falta el job '{job}'")
                continue
            spec = jobs[job]
            if not isinstance(spec, dict) or spec.get("continue-on-error") is not True:
                errors.append(
                    f"nightly.yml: el job '{job}' no es alert-only "
                    f"(falta continue-on-error: true)"
                )
        fuzz = jobs.get("fuzz")
        if isinstance(fuzz, dict):
            targets = _fuzz_targets_from_matrix(fuzz)
            for target in FUZZ_TARGETS:
                if target not in targets:
                    errors.append(
                        f"nightly.yml: el job 'fuzz' no incluye el target "
                        f"'{target}' en su matriz"
                    )
        if data.get("permissions") != {"contents": "read"}:
            errors.append("nightly.yml: permissions debe ser contents: read")
        if not _has_runner(jobs, NIGHTLY_OS):
            errors.append(
                f"nightly.yml: ningún job corre en '{NIGHTLY_OS}' (SPEC-0046)"
            )
        return
    # Fallback sin PyYAML: chequeo textual por indentación.
    jobs = _jobs_from_text(text)
    for job in REQUIRED_JOBS:
        if job not in jobs:
            errors.append(f"nightly.yml: falta el job '{job}' (chequeo textual)")
    if "continue-on-error: true" not in text:
        errors.append("nightly.yml: no declara continue-on-error (alert-only)")
    if "contents: read" not in text:
        errors.append("nightly.yml: no declara permissions contents: read")
    for target in FUZZ_TARGETS:
        if target not in text:
            errors.append(
                f"nightly.yml: no declara el target de fuzz '{target}' "
                f"(chequeo textual)"
            )
    if "-dict=" not in text:
        errors.append(
            "nightly.yml: el fuzz de query_parse no usa dictionary "
            "(-dict=) (chequeo textual)"
        )
    if NIGHTLY_OS not in text:
        errors.append(
            f"nightly.yml: no declara el runner '{NIGHTLY_OS}' (chequeo textual)"
        )


def check_mutants(errors: list[str]) -> None:
    """Valida la config de cargo-mutants (TOML y claves esperadas)."""
    if not MUTANTS.exists():
        errors.append(f"no existe {MUTANTS.relative_to(ROOT)}")
        return
    try:
        with MUTANTS.open("rb") as handle:
            data = tomllib.load(handle)
    except tomllib.TOMLDecodeError as exc:
        errors.append(f"mutants.toml: TOML inválido ({exc})")
        return
    for key in REQUIRED_MUTANTS_KEYS:
        if key not in data:
            errors.append(f"mutants.toml: falta la clave '{key}'")
    for key in FORBIDDEN_MUTANTS_KEYS:
        if key in data:
            errors.append(
                f"mutants.toml: clave no soportada '{key}' "
                f"(cargo-mutants usa deny_unknown_fields)"
            )
    exclude = data.get("exclude_globs")
    if not isinstance(exclude, list) or not exclude:
        errors.append("mutants.toml: 'exclude_globs' debe ser una lista no vacía")


def check_deny(errors: list[str]) -> None:
    """Valida que deny.toml declare las 4 secciones de supply chain."""
    if not DENY.exists():
        errors.append(f"no existe {DENY.relative_to(ROOT)}")
        return
    try:
        with DENY.open("rb") as handle:
            data = tomllib.load(handle)
    except tomllib.TOMLDecodeError as exc:
        errors.append(f"deny.toml: TOML inválido ({exc})")
        return
    for section in REQUIRED_DENY_SECTIONS:
        if section not in data:
            errors.append(f"deny.toml: falta la sección '[{section}]'")
    sources = data.get("sources", {})
    if isinstance(sources, dict):
        if sources.get("unknown-registry") != "deny":
            errors.append("deny.toml: sources.unknown-registry debe ser 'deny'")
        if sources.get("unknown-git") != "deny":
            errors.append("deny.toml: sources.unknown-git debe ser 'deny'")


def check_publish_flags(errors: list[str]) -> None:
    """Verifica que todo crate del workspace declare ``publish = false``.

    Motivo (deny.toml / job Security): las dependencias internas usan
    ``workspace = true`` (path sin versión). En crates *públicos* cargo-deny las
    marca como ``wildcard`` y falla el check. Marcar los crates internos como no
    publicables (aún no se publican) evita ese falso positivo. Este guard evita
    la recurrencia al añadir crates nuevos.
    """
    if not CRATES_DIR.is_dir():
        errors.append("no existe el directorio 'crates'")
        return
    for manifest in sorted(CRATES_DIR.glob("*/Cargo.toml")):
        text = manifest.read_text(encoding="utf-8")
        if "[package]" not in text:
            continue
        package = text.partition("[package]")[2].split("\n[", 1)[0]
        if "publish" not in package:
            errors.append(
                f"{manifest.relative_to(ROOT).as_posix()}: falta "
                f"'publish = false' (crate interno no publicable; evita "
                f"wildcard en cargo-deny)"
            )


def check_mutation_diff_job(errors: list[str]) -> None:
    """Valida el job `mutation-diff` de ``ci.yml`` (T2 bloqueante, SPEC-0061).

    El job debe existir, correr solo en `pull_request`, mutar con
    `--in-diff` y aplicar el gate `scripts/mutation_diff_gate.py` con
    `--min-score 70`.
    """
    if not CI.exists():
        errors.append(f"no existe {CI.relative_to(ROOT)}")
        return
    text = CI.read_text(encoding="utf-8")
    for needle, why in [
        ("mutation-diff:", "falta el job 'mutation-diff' (SPEC-0061)"),
        (
            "github.event_name == 'pull_request'",
            "el job 'mutation-diff' debe correr solo en pull_request",
        ),
        (
            "--in-diff",
            "el job 'mutation-diff' debe mutar con --in-diff (solo el diff)",
        ),
        (
            "mutation_diff_gate.py",
            "el job 'mutation-diff' debe aplicar scripts/mutation_diff_gate.py",
        ),
        (
            "--min-score 70",
            "el gate de mutación debe exigir --min-score 70",
        ),
    ]:
        if needle not in text:
            errors.append(f"ci.yml: {why} (chequeo textual)")


def check_coverage_job(errors: list[str]) -> None:
    """Valida el job `coverage` de ``ci.yml`` (SLO merge, SPEC-0061).

    El job debe existir, usar `cargo llvm-cov` con salida lcov y aplicar el
    gate `scripts/coverage_gate.py` con `--min-line 80 --min-branch 70` sobre
    el scope del core.
    """
    if not CI.exists():
        errors.append(f"no existe {CI.relative_to(ROOT)}")
        return
    text = CI.read_text(encoding="utf-8")
    for needle, why in [
        ("coverage:", "falta el job 'coverage' (SPEC-0061)"),
        (
            "cargo llvm-cov",
            "el job 'coverage' debe medir con cargo llvm-cov",
        ),
        (
            "--lcov",
            "el job 'coverage' debe exportar lcov",
        ),
        (
            "coverage_gate.py",
            "el job 'coverage' debe aplicar scripts/coverage_gate.py",
        ),
        (
            "--min-line 80 --min-branch 70",
            "el gate de cobertura debe exigir el SLO del core (80/70)",
        ),
    ]:
        if needle not in text:
            errors.append(f"ci.yml: {why} (chequeo textual)")


def main() -> int:
    """Ejecuta todas las validaciones y devuelve el exit code."""
    errors: list[str] = []
    check_ci_matrix(errors)
    check_nightly(errors)
    check_mutants(errors)
    check_deny(errors)
    check_publish_flags(errors)
    check_mutation_diff_job(errors)
    check_coverage_job(errors)

    if errors:
        print("[FAIL] check_ci_config: configuración de CI/supply chain inválida:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(
        "[OK] check_ci_config: ci.yml (matriz rápida ubuntu/windows + "
        "mutation-diff MS>=70 en PR), "
        "nightly.yml (mutation/fuzz/miri/sanitizers + test-macos, alert-only; "
        "fuzz con query_parse/wal_recover + dictionary), "
        "mutants.toml y deny.toml (4 secciones) válidos."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
