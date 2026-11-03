"""Conformance del fixture Rust desde Python (SPEC-0196).

Ejecuta `crates/ruscadb/tests/conformance/basic.json` sobre el C-ABI y
compara el resultado canónico. Compila antes con:

    cargo build -p ruscadb-ffi
    python bindings/python/conformance.py
"""

from __future__ import annotations

import hashlib
import json
import math
import pathlib
import sys
import tempfile

import ruscadb


def canonical_number(value):
    """Normaliza números como RFC 8785 para flotantes integrales."""
    if isinstance(value, float):
        if not math.isfinite(value):
            return None
        if value.is_integer() and abs(value) <= 2**53:
            return int(value)
    return value


def normalize(value):
    """Convierte la forma externa de ScalarValue a JSON plano."""
    if value == "Null":
        # Variante unitaria externa de ScalarValue::Null (Text("Null") llega
        # como {"Text":"Null"} y no se confunde).
        return None
    if isinstance(value, dict) and len(value) == 1:
        key, inner = next(iter(value.items()))
        if key in {"Int", "UInt", "TimestampMillis"}:
            return inner
        if key == "Float":
            return canonical_number(inner)
        if key in {"Text", "Bool"}:
            return inner
        if key == "Null":
            return None
        if key == "Bytes":
            return inner
    if isinstance(value, list):
        return [normalize(item) for item in value]
    if isinstance(value, dict):
        return {key: normalize(item) for key, item in value.items()}
    return canonical_number(value)


def canonical_rows(rows):
    """Orden estable de filas y claves para el hash."""
    normalized = [normalize(row) for row in rows]
    return sorted(
        normalized,
        key=lambda row: json.dumps(row, sort_keys=True, separators=(",", ":")),
    )


def seed(handle):
    """Crea y puebla las tablas `t` y `typed` del fixture."""
    ruscadb.create_table(handle, "t", [{"name": "x", "type": "Int"}])
    for value in (1, 2, 3):
        ruscadb.insert_record(handle, "t", {"scalars": {"x": {"Int": value}}})

    ruscadb.create_table(
        handle,
        "typed",
        [
            {"name": "f", "type": "Float"},
            {"name": "s", "type": "Text"},
            {"name": "b", "type": "Bool"},
            {"name": "n", "type": "Int"},
        ],
    )
    for f, s, b in ((1.5, "uno", False), (2.0, "dos", True)):
        ruscadb.insert_record(
            handle,
            "typed",
            {
                "scalars": {
                    "f": {"Float": f},
                    "s": {"Text": s},
                    "b": {"Bool": b},
                    "n": {"Null": None},
                }
            },
        )


def main() -> int:
    fixture_path = pathlib.Path(__file__).resolve().parents[2] / (
        "crates/ruscadb/tests/conformance/basic.json"
    )
    fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
    digest = hashlib.sha256()
    with tempfile.TemporaryDirectory() as tmp:
        path = pathlib.Path(tmp) / "conformance.db"
        with ruscadb.open_database(path, pool_capacity=64) as handle:
            seed(handle)
            for index, case in enumerate(fixture["cases"]):
                if case.get("expect_error"):
                    try:
                        ruscadb.execute(handle, case["sql"])
                    except Exception:  # noqa: BLE001 - el fixture espera error
                        digest.update(case["name"].encode("utf-8"))
                        digest.update(b"error")
                        continue
                    print(f"FAIL python/{case['name']}: se esperaba error")
                    return 1
                rows = json.loads(ruscadb.execute(handle, case["sql"]))
                got = canonical_rows(rows)
                if "--tamper" in sys.argv and index == 0 and got:
                    for key, value in list(got[0].items()):
                        if isinstance(value, int):
                            got[0][key] = value + 1
                            break
                expected = canonical_rows(case["expect"])
                if got != expected:
                    print(f"FAIL python/{case['name']}: got={got!r} expected={expected!r}")
                    return 1
                digest.update(case["name"].encode("utf-8"))
                digest.update(json.dumps(got, sort_keys=True, separators=(",", ":")).encode("utf-8"))
    print(f"python_hash={digest.hexdigest()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
