"""Compare the test-only Activity fixture with the frozen source, without writes.

This checks transcription only; it does not establish Windows compatibility,
runtime behavior, safety, or approval to execute the operations.
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE_PATH = "winutil-26.08.19/config/tweaks.json"
ID = "WPFTweaksActivity"
NAMES = ("EnableActivityFeed", "PublishUserActivities", "UploadUserActivities")
KEY = r"SOFTWARE\Policies\Microsoft\Windows\System"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    source_bytes = (ROOT / SOURCE_PATH).read_bytes()
    source = json.loads(source_bytes.decode("utf-8-sig"), strict=False)
    catalog = json.loads((ROOT / "config/catalog.v1.json").read_text(encoding="utf-8"))
    require(catalog["schema_version"] == 1, "unexpected schema version")
    require(catalog["source"]["path"] == SOURCE_PATH, "unexpected source path")
    digest = hashlib.sha256(source_bytes).hexdigest().upper()
    require(catalog["source"]["sha256"].upper() == digest, "source SHA-256 mismatch")
    require(len(catalog["tweaks"]) == 1, "expected exactly one fixture")
    fixture = catalog["tweaks"][0]
    require(fixture["id"] == fixture["source_id"] == ID, "unexpected fixture ID")
    require(fixture["support"] == {"status": "unverified"}, "fixture must remain unverified")
    upstream = source[ID]
    require(fixture["title"] == upstream["Content"], "title mismatch")
    entries = upstream["registry"]
    require(len(entries) == len(NAMES), "unexpected source operation count")
    require(len(fixture["operations"]) == len(NAMES), "unexpected fixture operation count")
    for index, name in enumerate(NAMES):
        entry = entries[index]
        require(entry["Path"] == "HKLM:\\" + KEY and entry["Name"] == name,
                f"source target mismatch for {name}")
        require(entry["Type"] == "DWord" and entry["Value"] == "0",
                f"source type/value mismatch for {name}")
        expected = {
            "operation": "set_registry_value",
            "target": {"hive": "LocalMachine", "view": "Registry64",
                       "key_path": KEY, "value_name": name},
            "value": {"native_type": 4, "bytes": [0, 0, 0, 0]},
        }
        require(fixture["operations"][index] == expected, f"fixture mismatch for {name}")
    print(f"Source SHA-256: {digest}")
    print(f"{ID}: verified transcription of {len(NAMES)} operations; OS support remains unverified")


if __name__ == "__main__":
    try:
        main()
    except (OSError, KeyError, TypeError, ValueError) as error:
        print(f"Catalog fixture verification failed: {error}", file=sys.stderr)
        sys.exit(1)
