"""Read-only comparison of the frozen catalogs with the draft inventory."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SOURCE_ROOT = ROOT / "winutil-26.08.19" / "config"
TWEAKS_PATH = SOURCE_ROOT / "tweaks.json"
APPLICATIONS_PATH = SOURCE_ROOT / "applications.json"
INVENTORY_PATH = ROOT / "TWEAKS_INVENTORY.md"
FACTS_PATH = ROOT / "docs" / "upstream-tweaks-facts.json"
COMPARISON_PATH = ROOT / "docs" / "inventory-source-comparison.json"


def read_json(path: Path):
    # The frozen tweak catalog contains an unescaped newline in a script.
    return json.loads(path.read_text(encoding="utf-8-sig"), strict=False)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest().upper()


def inventory_rows(path: Path) -> tuple[dict[str, dict], list[str]]:
    result = {}
    duplicates = []
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        if not line.startswith("|") or line.startswith("|:") or "Tweak ID" in line:
            continue
        columns = [column.strip() for column in line.strip("|").split("|")]
        if len(columns) >= 11 and re.fullmatch(r"\d+", columns[0]):
            identifier = columns[1]
            if len(identifier) >= 2 and identifier[0] == "`" and identifier[-1] == "`":
                identifier = identifier[1:-1]
                if identifier in result:
                    duplicates.append(identifier)
                result[identifier] = {
                    "row_number": int(columns[0]),
                    "display_name": columns[2],
                    "category": columns[3],
                    "draft_type": columns[4],
                    "draft_operations": columns[5],
                    "draft_apply_value": columns[6],
                    "draft_undo_value": columns[7],
                    "draft_compatibility_claim": columns[8],
                    "draft_undo_claim": columns[10],
                }
    return result, duplicates


def field_count(entries: dict, field: str) -> int:
    return sum(field in entry for entry in entries.values())


def action_facts(value) -> list[dict]:
    """Record script identity without copying executable source into the report."""
    if value is None:
        return []
    values = value if isinstance(value, list) else [value]
    facts = []
    for item in values:
        encoded = str(item).encode("utf-8")
        facts.append({"sha256": hashlib.sha256(encoded).hexdigest().upper(), "utf8_bytes": len(encoded)})
    return facts


def source_facts(tweaks: dict) -> dict:
    entries = {}
    for identifier, entry in tweaks.items():
        entries[identifier] = {
            "content": entry.get("Content"),
            "description": entry.get("Description"),
            "category": entry.get("category"),
            "registry": entry.get("registry", []),
            "service": entry.get("service", []),
            "invoke_scripts": action_facts(entry.get("InvokeScript")),
            "undo_scripts": action_facts(entry.get("UndoScript")),
            "appx": entry.get("appx", []),
            "combo_items": entry.get("ComboItems", []),
            "other_fields": sorted(
                key for key in entry
                if key not in {"Content", "Description", "category", "registry", "service", "InvokeScript", "UndoScript", "appx", "ComboItems"}
            ),
        }
    return {
        "source_file": "winutil-26.08.19/config/tweaks.json",
        "source_sha256": sha256(TWEAKS_PATH),
        "meaning": "Extracted source facts only; no compatibility, safety, reversibility, or implementation approval is implied.",
        "entries": entries,
    }


def inventory_comparison(tweaks: dict, draft: dict[str, dict]) -> dict:
    source_ids = set(tweaks)
    draft_ids = set(draft)
    rows = {}
    for identifier in sorted(source_ids | draft_ids):
        source = tweaks.get(identifier)
        rows[identifier] = {
            "match_status": "exact_id_match" if source and identifier in draft else (
                "source_only" if source else "draft_only"
            ),
            "upstream": None if source is None else {
                "content": source.get("Content"),
                "category": source.get("category"),
                "registry_entry_count": len(source.get("registry", [])),
                "service_entry_count": len(source.get("service", [])),
                "invoke_script_count": len(source.get("InvokeScript", [])),
                "undo_script_count": len(source.get("UndoScript", [])),
                "appx_entry_count": len(source.get("appx", [])),
            },
            "draft": draft.get(identifier),
        }
    return {
        "source_file": "winutil-26.08.19/config/tweaks.json",
        "draft_file": "TWEAKS_INVENTORY.md",
        "source_sha256": sha256(TWEAKS_PATH),
        "meaning": "Exact-ID comparison of labels and draft claims with source declaration counts; values are not semantically validated. Similar IDs are not aliases.",
        "exact_id_matches": len(source_ids & draft_ids),
        "source_only_ids": sorted(source_ids - draft_ids),
        "draft_only_ids": sorted(draft_ids - source_ids),
        "entries": rows,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write-facts",
        action="store_true",
        help=f"write deterministic extracted source facts to {FACTS_PATH.relative_to(ROOT)}",
    )
    args = parser.parse_args()
    tweaks = read_json(TWEAKS_PATH)
    applications = read_json(APPLICATIONS_PATH)
    draft, duplicate_ids = inventory_rows(INVENTORY_PATH)
    draft_ids = list(draft)
    draft_set = set(draft_ids)
    source_set = set(tweaks)

    if duplicate_ids:
        print(f"Duplicate draft IDs: {sorted(set(duplicate_ids))}")

    print(f"Tweak source SHA-256: {sha256(TWEAKS_PATH)}")
    if args.write_facts:
        FACTS_PATH.parent.mkdir(parents=True, exist_ok=True)
        FACTS_PATH.write_text(json.dumps(source_facts(tweaks), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"Wrote extracted per-entry source facts: {FACTS_PATH.relative_to(ROOT)} ({len(tweaks)} entries)")
        COMPARISON_PATH.write_text(
            json.dumps(inventory_comparison(tweaks, draft), ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(f"Wrote exact-ID draft comparison: {COMPARISON_PATH.relative_to(ROOT)} ({len(draft)} draft rows)")
    print(f"Application source SHA-256: {sha256(APPLICATIONS_PATH)}")
    print(f"Tweak IDs: source={len(source_set)} draft={len(draft_ids)} exact_matches={len(source_set & draft_set)}")
    for field in ("registry", "service", "InvokeScript", "UndoScript", "Type", "ComboItems"):
        print(f"tweaks with {field}: {field_count(tweaks, field)}")
    print(f"Application IDs: {len(applications)}")
    for field in ("winget", "choco"):
        print(f"applications with {field}: {field_count(applications, field)}")

    print("Source IDs missing from draft inventory:")
    for identifier in tweaks:
        if identifier not in draft_set:
            print(f"  {identifier}")

    print("Draft IDs absent from frozen source:")
    for identifier in draft_ids:
        if identifier not in source_set:
            print(f"  {identifier}")

    for identifier in ("WPFTweaksActivity", "WPFTweaksHiber"):
        entry = tweaks[identifier]
        original_values = [item.get("OriginalValue") for item in entry.get("registry", [])]
        print(
            f"{identifier}: registry={len(entry.get('registry', []))} "
            f"service={len(entry.get('service', []))} "
            f"invoke={len(entry.get('InvokeScript', []))} "
            f"undo={len(entry.get('UndoScript', []))} "
            f"original_values={original_values}"
        )


if __name__ == "__main__":
    main()
