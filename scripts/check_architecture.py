#!/usr/bin/env python3
"""Guard de arquitectura hexagonal de RuscaDB (gate T1).

Lee el grafo real de dependencias del workspace desde ``cargo metadata`` y
verifica tres propiedades no negociables de la arquitectura
(``docs/RuscaDB-roadmap.md`` §4.3 y fitness functions FF-01/FF-02):

1. No existen dependencias circulares entre las crates del workspace.
2. Cada crate respeta las aristas permitidas: el dominio ``ruscadb-core`` no
   depende de ningun adapter, y los adapters solo dependen de ``core``.
3. Toda crate del workspace aparece en ``ALLOWED`` (obliga a actualizar el mapa
   al anadir una crate nueva).

Salida: exit 0 si cumple; exit 1 con diagnostico legible si no.
"""

from __future__ import annotations

import json
import subprocess
import sys
from collections import defaultdict

# Aristas internas permitidas: crate -> crates de las que PUEDE depender.
ALLOWED: dict[str, set[str]] = {
    "ruscadb-core": set(),
    "ruscadb-wal": {"ruscadb-core"},
    "ruscadb-storage": {"ruscadb-core"},
    "ruscadb-query": {"ruscadb-core"},
    "ruscadb-vector": {"ruscadb-core"},
    "ruscadb-graph": {"ruscadb-core"},
    "ruscadb-multimodal": {"ruscadb-core", "ruscadb-storage"},
    "ruscadb-ai": {"ruscadb-core"},
    "ruscadb-ffi": {"ruscadb-core"},
    "ruscadb-py": {"ruscadb-ffi"},
    "ruscadb-node": {"ruscadb-ffi"},
    "ruscadb": {
        "ruscadb-core",
        "ruscadb-storage",
        "ruscadb-query",
        "ruscadb-vector",
        "ruscadb-graph",
        "ruscadb-multimodal",
        "ruscadb-ai",
        "ruscadb-wal",
        "ruscadb-ffi",
    },
    "ruscadb-testkit": {"ruscadb-core"},
}


def load_graph() -> dict[str, set[str]]:
    """Devuelve el grafo interno ``crate -> dependencias internas``."""
    proc = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=True,
    )
    metadata = json.loads(proc.stdout)
    names = {pkg["name"] for pkg in metadata["packages"]}
    graph: dict[str, set[str]] = {}
    for pkg in metadata["packages"]:
        internal: set[str] = set()
        for dep in pkg.get("dependencies", []):
            # Solo dependencias normales o de build (dev-deps no cuentan como
            # aristas arquitectonicas).
            if dep["name"] in names and dep.get("kind") in (None, "build"):
                internal.add(dep["name"])
        graph[pkg["name"]] = internal
    return graph


def find_cycles(graph: dict[str, set[str]]) -> list[list[str]]:
    """Detecta ciclos con DFS tricolor. Devuelve la lista de ciclos."""
    white, gray, black = 0, 1, 2
    color: dict[str, int] = defaultdict(int)
    stack: list[str] = []
    cycles: list[list[str]] = []

    def visit(node: str) -> None:
        color[node] = gray
        stack.append(node)
        for nxt in sorted(graph.get(node, ())):
            if color[nxt] == gray:
                start = stack.index(nxt)
                cycles.append([*stack[start:], nxt])
            elif color[nxt] == white:
                visit(nxt)
        stack.pop()
        color[node] = black

    for node in sorted(graph):
        if color[node] == white:
            visit(node)
    return cycles


def main() -> int:
    """Ejecuta las verificaciones y devuelve el exit code."""
    graph = load_graph()
    errors: list[str] = []

    # 3. Toda crate conocida.
    for crate in sorted(graph):
        if crate not in ALLOWED:
            errors.append(
                f"crate desconocida '{crate}': agregala a ALLOWED en "
                f"scripts/check_architecture.py"
            )

    # 2. Aristas permitidas.
    for crate, deps in sorted(graph.items()):
        if crate not in ALLOWED:
            continue
        forbidden = deps - ALLOWED[crate]
        for dep in sorted(forbidden):
            errors.append(
                f"arista prohibida: '{crate}' -> '{dep}' "
                f"(permitidas: {sorted(ALLOWED[crate]) or 'ninguna'})"
            )

    # 1. Ciclos.
    for cycle in find_cycles(graph):
        errors.append("ciclo de dependencias: " + " -> ".join(cycle))

    if errors:
        print("[FAIL] check_architecture: arquitectura hexagonal violada:")
        for err in errors:
            print(f"  - {err}")
        return 1

    print(f"[OK] check_architecture: {len(graph)} crates, 0 ciclos, 0 aristas prohibidas.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
