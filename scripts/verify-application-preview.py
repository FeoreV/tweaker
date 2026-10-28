from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "winutil-26.08.19" / "config" / "applications.json"
PREVIEW = ROOT / "ui" / "src" / "data" / "applications.json"
EXPECTED_SHA256 = "46AD39984E5E7D1F38EFFC657467B5F1A63B9BFAD777CBEC870CFD2910B9F30F"
REQUIRED_FIELDS = {"category", "content", "description", "link", "winget", "foss"}
ALLOWED_FIELDS = REQUIRED_FIELDS | {"choco"}


def load_unique_json(path: Path) -> Any:
    def reject_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key {key!r} in {path}")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8-sig"), object_pairs_hook=reject_duplicates)


def fail(message: str) -> None:
    raise SystemExit(f"FAIL: {message}")


def main() -> None:
    actual_hash = hashlib.sha256(SOURCE.read_bytes()).hexdigest().upper()
    if actual_hash != EXPECTED_SHA256:
        fail(f"upstream SHA-256 changed: expected {EXPECTED_SHA256}, got {actual_hash}")

    source = load_unique_json(SOURCE)
    preview = load_unique_json(PREVIEW)
    if not isinstance(source, dict) or not isinstance(preview, dict):
        fail("both catalogs must be JSON objects keyed by upstream application ID")

    source_ids, preview_ids = set(source), set(preview)
    if source_ids != preview_ids:
        missing = sorted(source_ids - preview_ids)
        extra = sorted(preview_ids - source_ids)
        fail(f"ID set differs; missing={missing[:10]}, extra={extra[:10]}")

    expected: dict[str, dict[str, Any]] = {}
    choco_present = 0
    categories: dict[str, int] = {}
    for app_id, entry in source.items():
        if not isinstance(entry, dict):
            fail(f"upstream entry {app_id!r} is not an object")
        missing_fields = REQUIRED_FIELDS - entry.keys()
        if missing_fields:
            fail(f"upstream entry {app_id!r} lacks {sorted(missing_fields)}")
        if not isinstance(entry["foss"], bool):
            fail(f"upstream entry {app_id!r} has non-boolean foss")
        for field in REQUIRED_FIELDS - {"foss"}:
            if not isinstance(entry[field], str):
                fail(f"upstream entry {app_id!r} has non-string {field}")
        if "choco" in entry:
            if not isinstance(entry["choco"], str):
                fail(f"upstream entry {app_id!r} has non-string choco")
            choco_present += 1
        categories[entry["category"]] = categories.get(entry["category"], 0) + 1
        expected[app_id] = {field: entry[field] for field in ALLOWED_FIELDS if field in entry}

    if preview != expected:
        for app_id in sorted(source_ids):
            if preview[app_id] != expected[app_id]:
                fail(f"metadata mismatch for {app_id!r}; fields must match source including optional choco presence")
        fail("preview metadata differs from upstream")

    print(f"PASS: {len(source)} application IDs and projected fields exactly match upstream")
    print(f"PASS: choco preserved on {choco_present}/{len(source)} entries; omitted only where upstream omits it")
    print(f"PASS: source SHA-256 {actual_hash}")
    print(f"Categories: {len(categories)}; entries: {', '.join(f'{name}={count}' for name, count in sorted(categories.items()))}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
