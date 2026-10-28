"""Verify a data-only registry apply review against the frozen WinUtil source.

This fixture records declarations, not executable or supported actions. Upstream
PowerShell uses the process registry view; Registry64 is rustweaker's review
policy and is not proof of equivalent WoW64 behavior or OS support. HKCU entries
are bound to the caller's user context, which this fixture does not identify;
HKCU/HKU entries have no application mutation path. No Windows 7 support is
asserted. Upstream OriginalValue fields are fixed catalog values, not observed
pre-state or backup data. The allowlists exclude script, service, AppX,
Values-combo, and other non-registry declarations.
"""

import hashlib
import json
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = "winutil-26.08.19/config/tweaks.json"
SOURCE_SHA256 = "CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9"
CATALOG = "config/catalog.registry-review.v1.json"
IDS = (
    "WPFTweaksActivity", "WPFTweaksRevertStartMenu", "WPFTweaksBraveDebloat",
    "WPFTweaksConsumerFeatures", "WPFTweaksDeliveryOptimization", "WPFTweaksWPBT",
    "WPFTweaksPreventDeviceMetadataFromNetwork", "WPFTweaksIPv46",
    "WPFToggleDetailedBSoD", "WPFToggleVerboseLogon", "WPFToggleS3Sleep",
    "WPFToggleLoginBlur", "WPFTweaksDisableLockscreen", "WPFToggleLongPaths",
    "WPFTweaksDisableWarningForUnsignedRdp", "WPFTweaksEdgeDebloat", "WPFTweaksUTC",
    "WPFTweaksRemoveHomeAndGallery", "WPFTweaksEndTaskOnTaskbar", "WPFTweaksStorage",
    "WPFTweaksDisableNotifications", "WPFTweaksDisableBGapps", "WPFToggleBatteryPercentage",
    "WPFToggleNewOutlook", "WPFToggleScrollbars", "WPFToggleMouseAcceleration",
    "WPFToggleNumLock", "WPFToggleWindowSnapping", "WPFToggleStandbyFix",
    "WPFToggleHideSettingsHome", "WPFToggleBingSearch", "WPFToggleStickyKeys",
    "WPFToggleTaskbarSearch", "WPFToggleTaskView", "WPFToggleGameMode",
)
SUMMARY = "Upstream registry apply declarations; Windows support is unverified."
UPSTREAM_KEYS = {"Content", "Description", "category", "panel", "Type", "registry", "link"}
REGISTRY_KEYS = {"Path", "Name", "Type", "Value", "OriginalValue", "DefaultState"}
HIVE_PREFIXES = {"HKLM:\\": "LocalMachine", "HKCU:\\": "CurrentUser", "HKU:\\": "Users"}
VALUE_TYPES = {
    "DWord": (4, 4),
    "QWord": (11, 8),
    "String": (1, None),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def main():
    source_bytes = (ROOT / SOURCE).read_bytes()
    source = json.loads(source_bytes.decode("utf-8-sig"), strict=False,
                        object_pairs_hook=unique_object)
    catalog = json.loads((ROOT / CATALOG).read_text(encoding="utf-8"),
                         object_pairs_hook=unique_object)
    require(set(catalog) == {"schema_version", "source", "tweaks"}, "catalog fields")
    require(type(catalog["schema_version"]) is int and catalog["schema_version"] == 1,
            "schema version")
    digest = hashlib.sha256(source_bytes).hexdigest().upper()
    require(digest == SOURCE_SHA256, "frozen source SHA-256 mismatch")
    require(catalog["source"] == {"path": SOURCE, "sha256": digest}, "source provenance")
    tweaks = catalog["tweaks"]
    require(isinstance(tweaks, list) and len(tweaks) == len(IDS), "tweak count")
    require([t["id"] for t in tweaks] == list(IDS), "tweak IDs or order")
    counts = {"DWord": 0, "QWord": 0, "String": 0}
    hives = Counter()
    for tweak, identifier in zip(tweaks, IDS):
        entry = source[identifier]
        # An allowlist also rejects future action fields instead of silently dropping them.
        require(set(entry) <= UPSTREAM_KEYS and "registry" in entry,
                f"{identifier}: unsupported source fields")
        require(set(tweak) == {"id", "title", "summary", "source_id", "support", "operations"},
                f"{identifier}: fixture fields")
        require(tweak["source_id"] == identifier and tweak["title"] == entry["Content"]
                and tweak["summary"] == SUMMARY and tweak["support"] == {"status": "unverified"},
                f"{identifier}: metadata")
        records = entry["registry"]
        operations = tweak["operations"]
        require(isinstance(records, list) and records and isinstance(operations, list)
                and len(records) == len(operations), f"{identifier}: operation count")
        for index, (record, operation) in enumerate(zip(records, operations)):
            where = f"{identifier}[{index}]"
            require(set(record) <= REGISTRY_KEYS and {"Path", "Name", "Type", "Value"} <= set(record),
                    f"{where}: unsupported declaration fields")
            path, name, value = record["Path"], record["Name"], record["Value"]
            require(isinstance(path, str) and len(path) > 6 and "\0" not in path,
                    f"{where}: path")
            require(isinstance(name, str) and "\0" not in name and "\0" not in path,
                    f"{where}: name/path")
            prefix = next((candidate for candidate in HIVE_PREFIXES if path.startswith(candidate)), None)
            require(prefix is not None, f"{where}: unsupported hive/path")
            value_type = record["Type"]
            require(value_type in VALUE_TYPES, f"{where}: unsupported registry type")
            native_type, width = VALUE_TYPES[value_type]
            if value_type in ("DWord", "QWord"):
                require(isinstance(value, str) and re.fullmatch(r"[0-9]+", value) is not None,
                        f"{where}: {value_type} must be a decimal string")
                integer = int(value)
                require(integer < 1 << (width * 8), f"{where}: {value_type} overflow")
                raw = integer.to_bytes(width, "little")
            else:
                require(isinstance(value, str) and "\0" not in value,
                        f"{where}: REG_SZ must be a string without embedded NUL")
                raw = value.encode("utf-16le") + b"\0\0"
            expected = {"operation": "set_registry_value",
                        "target": {"hive": HIVE_PREFIXES[prefix], "view": "Registry64",
                                   "key_path": path[len(prefix):], "value_name": name},
                        "value": {"native_type": native_type, "bytes": list(raw)}}
            # Python considers bool equal to int; exclude it explicitly from numeric fields.
            require(type(operation.get("value", {}).get("native_type")) is int
                    and isinstance(operation.get("value", {}).get("bytes"), list)
                    and all(type(b) is int for b in operation["value"]["bytes"])
                    and operation == expected, f"{where}: operation mismatch")
            counts[value_type] += 1
            hives[HIVE_PREFIXES[prefix]] += 1
    require(counts == {"DWord": 70, "QWord": 1, "String": 5}, "unexpected operation type/count")
    require(hives == {"LocalMachine": 47, "CurrentUser": 28, "Users": 1},
            "unexpected registry hive/count")
    print(f"Source SHA-256: {digest}")
    print(f"Verified {len(IDS)} registry-only apply entries and {sum(counts.values())} operations "
          f"(DWORD {counts['DWord']}, QWORD {counts['QWord']}, REG_SZ {counts['String']}); "
          f"hives HKLM {hives['LocalMachine']}, HKCU {hives['CurrentUser']}, "
          f"HKU {hives['Users']}; review data only: support unverified, HKCU context/apply path unavailable, "
          "Registry64 policy differs from upstream process view, OriginalValue is not backup")


if __name__ == "__main__":
    try:
        main()
    except (OSError, KeyError, TypeError, ValueError, AttributeError) as error:
        print(f"Registry review verification failed: {error}", file=sys.stderr)
        sys.exit(1)
