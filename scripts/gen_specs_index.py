import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"
OUT = ROOT / "docs" / "specs-index.md"

rows = []
for path in sorted(SPECS.glob("*.md")):
    if path.name.startswith("_"):
        continue
    text = path.read_text(encoding="utf-8")
    front = re.match(r"\A---\r?\n(.*?)\r?\n---", text, re.DOTALL)
    if not front:
        continue
    header = front.group(1)

    def field(name: str) -> str:
        match = re.search(rf"(?m)^{name}:\s*(\S+)\s*$", header)
        return match.group(1) if match else "-"

    rows.append(
        (
            field("id"),
            field("feature"),
            field("status"),
            field("maturity"),
            path.name,
        )
    )

rows.sort(key=lambda row: row[0])
lines = [
    "# Indice de specs (generado)",
    "",
    "> Generado desde el frontmatter de `specs/*.md` por `scripts/gen_specs_index.py`.",
    "> No editar a mano: regenerar tras cambiar una spec.",
    "",
    "| Spec | Feature | Status | Madurez | Archivo |",
    "|---|---|---|---|---|",
]
for spec_id, feature, status, maturity, name in rows:
    lines.append(f"| {spec_id} | `{feature}` | {status} | {maturity} | `{name}` |")
lines.append("")
OUT.write_text("\n".join(lines), encoding="utf-8")
print(f"index: {len(rows)} specs -> {OUT.relative_to(ROOT)}")
