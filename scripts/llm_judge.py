#!/usr/bin/env python3
"""Juez T2 (LLM) sobre un diff para el harness de RuscaDB (SPEC-0100).

Construye el prompt de la puerta T2 —**fidelidad a la spec** (Spec Fidelity
``>= 0.90``) más una **revisión adversarial** que exige **0 hallazgos HIGH**—
y lo delega en ``scripts/llm_provider.py`` (cliente agnóstico de proveedor:
``local`` = llama.cpp en desarrollo, ``openai`` = producción, ``ollama`` =
legacy). El backend se elige por entorno, p. ej. en producción::

    RUSCADB_LLM_PROVIDER=openai OPENAI_API_KEY=sk-... llm_judge.py change.diff

Con ``--show-prompt`` imprime el prompt y **no** toca la red ni resuelve el
proveedor (útil para tests y para auditar la rúbrica).

Uso::

    llm_judge.py change.diff                 # lee el diff del fichero
    git diff | llm_judge.py                  # o de stdin
    llm_judge.py --show-prompt change.diff   # imprime el prompt, sin red

Salida: ``0`` veredicto (o prompt) impreso; ``2`` error de configuración, red
o entrada (nunca verde silencioso). Solo stdlib de Python.
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import llm_provider  # noqa: E402  (import tras fijar sys.path)

#: Umbral mínimo de fidelidad a la spec exigido por la puerta T2.
SPEC_FIDELITY_THRESHOLD = 0.90

T2_SYSTEM = (
    "Eres el juez T2 de un gate de ingeniería de RuscaDB. Evalúas un diff "
    "contra la spec que implementa. Respondes en texto plano y accionable, sin "
    "adornos."
)

USAGE = """usage: llm_judge.py [diff] [--show-prompt] [--provider P] [--model M] [--base-url URL]

Juez T2 (LLM) sobre un diff: fidelidad a la spec (Spec Fidelity >= 0.90) +
revisión adversarial con 0 hallazgos HIGH (SPEC-0100).

  diff            ruta del diff; si se omite, se lee de stdin.
  --show-prompt   imprime el prompt T2 y NO llama a la red.
  --provider P    local (llama.cpp, default) | openai (producción) | ollama.
  --model M       sobreescribe RUSCADB_LLM_MODEL.
  --base-url URL  sobreescribe la base del proveedor.

Salida: 0 veredicto impreso; 2 error de configuración, red o entrada.
"""


class JudgeError(RuntimeError):
    """Error de entrada del juez (diff ausente o ilegible)."""


def build_prompt(diff):
    """Construye el prompt T2 (fidelidad a la spec + revisión adversarial).

    Args:
        diff: Texto del diff unificado a evaluar.

    Returns:
        El prompt completo como cadena de texto.
    """
    return (
        "Evalúa el siguiente diff como juez T2 de RuscaDB.\n\n"
        "Criterios (todos obligatorios):\n"
        f"1. Spec Fidelity >= {SPEC_FIDELITY_THRESHOLD:.2f}: cada requisito de "
        "la spec debe estar implementado y trazado; puntúa la fidelidad de "
        "0.00 a 1.00.\n"
        "2. Revisión adversarial: busca activamente cómo el diff podría "
        "incumplir la spec, ocultar deuda técnica o romper la arquitectura "
        "hexagonal.\n"
        "3. Hallazgos: clasifica cada uno como HIGH/MEDIUM/LOW con "
        "fichero:línea y su justificación. El veredicto es PASS solo con "
        "0 hallazgos HIGH.\n\n"
        "Devuelve: (a) Spec Fidelity con su valor numérico; (b) la lista de "
        "hallazgos; (c) el veredicto PASS/FAIL.\n\n"
        "--- diff ---\n"
        f"{diff}"
    )


def read_diff(source):
    """Lee el diff de un fichero o de stdin.

    Args:
        source: Ruta del diff o ``None`` para leer de stdin.

    Returns:
        El texto del diff.

    Raises:
        JudgeError: Si el fichero no existe, no se puede leer o stdin está
            vacío.
    """
    if source is None:
        diff = sys.stdin.read()
        if not diff.strip():
            raise JudgeError(
                "WHAT: no se recibió ningún diff por stdin. "
                "WHY: el juez T2 necesita un diff que evaluar. "
                "WHERE: pasa una ruta o usa 'git diff | llm_judge.py'."
            )
        return diff
    try:
        with open(source, encoding="utf-8") as handle:
            return handle.read()
    except FileNotFoundError as error:
        raise JudgeError(
            f"WHAT: el diff '{source}' no existe. "
            "WHY: la ruta indicada no es un fichero. "
            "WHERE: verifica la ruta o usa 'git diff' como entrada."
        ) from error
    except OSError as error:
        raise JudgeError(
            f"WHAT: no se pudo leer el diff '{source}'. "
            f"WHY: {error}. WHERE: revisa permisos y la ruta."
        ) from error


def _parse_args(argv):
    """Parsea los argumentos de la CLI del juez.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en ``[0]``).

    Returns:
        Diccionario con ``source``, ``show_prompt``, ``help``, ``provider``,
        ``model`` y ``base_url``.

    Raises:
        JudgeError: Si una opción requiere valor y falta, o si el argumento es
            desconocido.
    """
    options = {
        "source": None,
        "show_prompt": False,
        "help": False,
        "provider": None,
        "model": None,
        "base_url": None,
    }
    args = list(argv[1:])
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--show-prompt":
            options["show_prompt"] = True
        elif arg in ("-h", "--help"):
            options["help"] = True
        elif arg in ("--provider", "--model", "--base-url"):
            if index + 1 >= len(args):
                raise JudgeError(
                    f"WHAT: la opción '{arg}' requiere un valor. "
                    f"WHY: se invocó sin argumento. WHERE: añade '{arg} <valor>'."
                )
            options[arg[2:].replace("-", "_")] = args[index + 1]
            index += 1
        elif arg.startswith("-"):
            raise JudgeError(
                f"WHAT: argumento desconocido '{arg}'. "
                "WHY: no forma parte de la CLI del juez T2. "
                "WHERE: usa [diff], --show-prompt, --provider, --model, --base-url."
            )
        else:
            options["source"] = arg
        index += 1
    return options


def main(argv):
    """Punto de entrada del juez T2.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en ``[0]``).

    Returns:
        Exit code: ``0`` veredicto (o prompt) impreso; ``2`` error de
        configuración, red o entrada.
    """
    llm_provider.ensure_utf8_stdio()
    try:
        options = _parse_args(argv)
    except JudgeError as error:
        print(f"llm-judge: {error}", file=sys.stderr)
        return 2
    if options["help"]:
        print(USAGE)
        return 0
    try:
        diff = read_diff(options["source"])
        prompt = build_prompt(diff)
        if options["show_prompt"]:
            print(prompt)
            return 0
        config = llm_provider.resolve_config(
            options["provider"], options["model"], options["base_url"]
        )
        verdict = llm_provider.complete(prompt, system=T2_SYSTEM, config=config)
    except (
        JudgeError,
        llm_provider.LLMConfigError,
        llm_provider.LLMRequestError,
    ) as error:
        print(f"llm-judge: {error}", file=sys.stderr)
        return 2
    print(verdict)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
