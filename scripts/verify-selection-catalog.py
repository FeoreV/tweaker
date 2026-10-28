#!/usr/bin/env python3
"""Verify the embedded selection ID fixture against frozen WinUtil sources."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "winutil-26.08.19" / "config"
FIXTURE = ROOT / "config" / "selection-catalog.v1.json"


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    folded: set[str] = set()
    for key, value in pairs:
        if key.lower() in folded:
            raise ValueError(f"duplicate case-insensitive JSON key: {key}")
        folded.add(key.lower())
        result[key] = value
    return result


def load_json(name: str, *, allow_literal_newlines: bool = False) -> Any:
    text = (SOURCE / name).read_text(encoding="utf-8")
    return json.loads(
        text,
        strict=not allow_literal_newlines,
        object_pairs_hook=reject_duplicate_keys,
    )


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def main() -> int:
    fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
    require(fixture.get("schema_version") == 1, "unsupported fixture schema")
    require(
        "ID-selection metadata only" in fixture["provenance"]["notice"],
        "fixture must retain the non-authorization notice",
    )

    source_names = (
        "applications.json",
        "tweaks.json",
        "feature.json",
        "appx.json",
        "preset.json",
    )
    documents: dict[str, Any] = {}
    for name in source_names:
        raw = (SOURCE / name).read_bytes()
        digest = hashlib.sha256(raw).hexdigest().upper()
        require(
            fixture["source_sha256"].get(name) == digest,
            f"source hash mismatch: {name}",
        )
        documents[name] = load_json(
            name, allow_literal_newlines=name in {"tweaks.json", "feature.json"}
        )

    expected = {
        "apps": [f"WPFInstall{key}" for key in documents["applications.json"]],
        "tweaks": [key for key in documents["tweaks.json"] if key.startswith("WPFTweaks")],
        "toggles": [key for key in documents["tweaks.json"] if key.startswith("WPFToggle")],
        "features": [key for key in documents["feature.json"] if key.startswith("WPFFeature")],
        "appx": [key for key in documents["appx.json"] if key.startswith("WPFAppx")],
    }
    actual = fixture.get("categories", {})
    require(set(actual) == set(expected), "selection category names do not match")

    seen: set[str] = set()
    for family, source_ids in expected.items():
        fixture_ids = actual[family]
        require(isinstance(fixture_ids, list), f"{family} catalog must be an array")
        require(
            {key.lower() for key in fixture_ids} == {key.lower() for key in source_ids},
            f"selection ID mismatch in {family}",
        )
        require(
            len({key.lower() for key in fixture_ids}) == len(fixture_ids),
            f"duplicate selection IDs in {family}",
        )
        require(
            not seen.intersection(key.lower() for key in fixture_ids),
            f"selection IDs overlap categories at {family}",
        )
        seen.update(key.lower() for key in fixture_ids)

    source_presets = documents["preset.json"]
    require(
        fixture.get("named_presets") == source_presets,
        "named preset contents/order do not match upstream",
    )
    for name, keys in source_presets.items():
        require(isinstance(keys, list) and keys, f"preset {name} must be non-empty")
        for key in keys:
            matches = sum(
                key.lower() in {item.lower() for item in ids}
                for ids in expected.values()
            )
            require(matches == 1, f"preset {name} key is unknown or ambiguous: {key}")

    counts = " ".join(f"{family}={len(ids)}" for family, ids in expected.items())
    preset_counts = " ".join(f"{name}={len(keys)}" for name, keys in source_presets.items())
    print(f"Selection catalog verified against 5 frozen sources: {counts}; presets: {preset_counts}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"selection catalog verification failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
