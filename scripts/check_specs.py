#!/usr/bin/env python3
"""Guard de specs SDD de RuscaDB.

Verifica invariantes baratas y deterministas sobre ``specs/*.md``:

1. Frontmatter presente con ``id`` (``SPEC-NNNN``), ``feature`` y ``status``.
2. ``id`` unico en todo el repositorio.
3. ``status`` en el conjunto permitido y ``maturity`` (si existe) tambien.
4. Los ``AC-NNNN-NN`` citados pertenecen al ``id`` de la spec.
5. ``status: implemented`` declara al menos un AC en ``acceptance_criteria``.
6. Un AC no se declara dos veces en ``acceptance_criteria``.

Salida: exit 0 si cumple; exit 1 con diagnostico legible.
"""

from __future__ import annotations

import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"

ALLOWED_STATUS = {"proposed", "accepted", "implemented", "superseded", "deferred"}
ALLOWED_MATURITY = {"real", "stub", "placeholder"}
FRONTMATTER = re.compile(r"\A---\r?\n(.*?)\r?\n---", re.DOTALL)
ID = re.compile(r"(?m)^id:\s*(SPEC-\d{4})\s*$")
FEATURE = re.compile(r"(?m)^feature:\s*(\S+)\s*$")
STATUS = re.compile(r"(?m)^status:\s*(\w+)\s*$")
MATURITY = re.compile(r"(?m)^maturity:\s*(\w+)\s*$")
DECLARED_AC = re.compile(r"(?m)^\s*-\s*id:\s*(AC-\d{4}-\d{2})\s*$")


def main() -> int:
    """Ejecuta las verificaciones y devuelve el exit code."""
    errors: list[str] = []
    seen_ids: dict[str, str] = {}
    status_counts: dict[str, int] = defaultdict(int)
    maturity_counts: dict[str, int] = defaultdict(int)

    for path in sorted(SPECS.glob("*.md")):
        name = path.name
        if name.startswith("_"):
            continue  # plantilla
        text = path.read_text(encoding="utf-8")
        front = FRONTMATTER.match(text)
        if not front:
            errors.append(f"{name}: falta frontmatter (--- ... ---)")
            continue
        header = front.group(1)
        id_match = ID.search(header)
        if not id_match:
            errors.append(f"{name}: falta 'id: SPEC-NNNN'")
            continue
        spec_id = id_match.group(1)
        number = spec_id.split("-")[1]

        feature_match = FEATURE.search(header)
        status_match = STATUS.search(header)
        if not feature_match:
            errors.append(f"{spec_id} ({name}): falta 'feature'")
        if not status_match:
            errors.append(f"{spec_id} ({name}): falta 'status'")
        else:
            status = status_match.group(1)
            status_counts[status] += 1
            if status not in ALLOWED_STATUS:
                errors.append(f"{spec_id} ({name}): status '{status}' no permitido")

        maturity_match = MATURITY.search(header)
        if maturity_match:
            maturity = maturity_match.group(1)
            maturity_counts[maturity] += 1
            if maturity not in ALLOWED_MATURITY:
                errors.append(
                    f"{spec_id} ({name}): maturity '{maturity}' no permitido"
                )

        if spec_id in seen_ids:
            errors.append(
                f"{spec_id}: id duplicado en {seen_ids[spec_id]} y {name}"
            )
        else:
            seen_ids[spec_id] = name

        # Las citas del cuerpo pueden referenciar otras specs (dependencias);
        # solo se valida el prefijo de los AC declarados en el frontmatter.
        declared = DECLARED_AC.findall(header)
        for ac_id in declared:
            if ac_id.split("-")[1] != number:
                errors.append(
                    f"{spec_id} ({name}): {ac_id} declarado no pertenece a la spec"
                )
                break
        for ac_id in sorted({ac for ac in declared if declared.count(ac) > 1}):
            errors.append(f"{spec_id} ({name}): {ac_id} declarado dos veces")
        if status_match and status_match.group(1) == "implemented" and not declared:
            errors.append(f"{spec_id} ({name}): implemented sin ACs declarados")

    if errors:
        print("[FAIL] check_specs: contratos de spec violados:")
        for error in errors:
            print(f"  - {error}")
        return 1

    print(
        f"[OK] check_specs: {len(seen_ids)} specs "
        f"(status: {dict(sorted(status_counts.items()))}; "
        f"maturity: {dict(sorted(maturity_counts.items()))})."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
