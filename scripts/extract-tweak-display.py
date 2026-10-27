"""Extract display-only catalog metadata; never writes to the upstream tree."""
import json
from pathlib import Path

root = Path(__file__).resolve().parent.parent
source = root / "winutil-26.08.19" / "config" / "tweaks.json"
destination = root / "ui" / "src" / "data" / "tweaks.json"
entries = json.loads(source.read_text(encoding="utf-8-sig"), strict=False)
items = []
for identifier, entry in entries.items():
    if not isinstance(entry.get("Content"), str) or not isinstance(entry.get("category"), str):
        raise ValueError(f"Invalid display metadata: {identifier}")
    description = entry.get("Description", "")
    if not isinstance(description, str):
        raise ValueError(f"Invalid description: {identifier}")
    items.append({"id": identifier, "name": entry["Content"], "category": entry["category"], "description": description})
destination.write_text(json.dumps(items, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"Wrote {len(items)} display entries to {destination.relative_to(root)}")
