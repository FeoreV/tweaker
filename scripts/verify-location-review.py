"""Verify the data-only WPFTweaksLocation review fixture.

This is not a production catalog loader and never executes an operation. The
review records upstream declarations and typed intended values only. Original*
fields are source data, not observed state or undo instructions. Registry64 is
the review mapping policy; upstream PowerShell's process view, user context,
and target OS are not established. The mockable plan implements the service
guard, but this fixture is not wired to Windows or CLI execution; external SCM
changes can still race the write.
"""

import copy
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = "winutil-26.08.19/config/tweaks.json"
SOURCE_SHA256 = "CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9"
FIXTURE = "config/catalog.location-review.v1.json"
TWEAK_ID = "WPFTweaksLocation"
SOURCE_TOP_KEYS = {"Content", "Description", "category", "panel", "service", "registry", "link"}
SERVICE_FIELDS = {"Name", "StartupType", "OriginalType"}
REGISTRY_FIELDS = {"Path", "Name", "Type", "Value", "OriginalValue"}
HIVE_PREFIX = "HKLM:\\"
START_TYPES = {"Boot": 0, "System": 1, "Automatic": 2, "Manual": 3, "Disabled": 4}
NOTES = [
    "The upstream handler processes service declarations before registry declarations.",
    "The mockable plan implements the upstream apply guard using OriginalType as the expected service start type and skips the service write on mismatch. This review fixture is not wired to Windows or CLI execution; an external SCM change can still race the write.",
    "OriginalType supplies the apply guard and OriginalValue is retained as source data. Neither is an undo value, observed pre-state, or backup record.",
    "Registry64 is the review mapping policy. The upstream PowerShell process registry view and equivalence are not verified; the user, registry context, and target OS are not verified.",
    "This review fixture is not a production catalog, grants no authorization, and contains no execution or recovery path.",
]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def encode_registry_value(record, where):
    kind = record["Type"]
    value = record["Value"]
    if kind == "DWord":
        require(isinstance(value, str) and re.fullmatch(r"[0-9]+", value) is not None,
                f"{where}: DWORD must be a decimal source string")
        integer = int(value)
        require(integer < 1 << 32, f"{where}: DWORD overflow")
        return 4, list(integer.to_bytes(4, "little"))
    if kind == "String":
        require(isinstance(value, str) and "\0" not in value,
                f"{where}: REG_SZ must not contain embedded NUL")
        return 1, list(value.encode("utf-16le") + b"\0\0")
    raise ValueError(f"{where}: unsupported source registry type {kind!r}")


def expected_registry_operation(record, index):
    where = f"{TWEAK_ID}.registry[{index}]"
    require(set(record) == REGISTRY_FIELDS, f"{where}: unsupported/missing source fields")
    path = record["Path"]
    name = record["Name"]
    require(isinstance(path, str) and path.startswith(HIVE_PREFIX) and "\0" not in path,
            f"{where}: unsupported registry path")
    require(isinstance(name, str) and "\0" not in name, f"{where}: invalid value name")
    native_type, raw = encode_registry_value(record, where)
    return {
        "operation": "set_registry_value",
        "target": {
            "hive": "LocalMachine",
            "view": "Registry64",
            "key_path": path[len(HIVE_PREFIX):],
            "value_name": name,
        },
        "value": {"native_type": native_type, "bytes": raw},
    }


def expected_operations(entry):
    services = entry["service"]
    registry = entry["registry"]
    require(isinstance(services, list) and len(services) == 1,
            f"{TWEAK_ID}: expected exactly one service declaration")
    service = services[0]
    require(set(service) == SERVICE_FIELDS, f"{TWEAK_ID}: unsupported/missing service fields")
    require(service["Name"] and "\0" not in service["Name"], f"{TWEAK_ID}: invalid service name")
    require(service["StartupType"] in START_TYPES, f"{TWEAK_ID}: unsupported StartupType")
    require(service["OriginalType"] in START_TYPES, f"{TWEAK_ID}: unsupported OriginalType")
    require(isinstance(registry, list) and len(registry) == 3,
            f"{TWEAK_ID}: expected exactly three registry declarations")

    # Keep upstream's phase order explicit: all service declarations precede registry.
    result = [{
        "operation": "set_service_start_type_if_expected",
        "target": {"name": service["Name"]},
        "start_type": START_TYPES[service["StartupType"]],
        "expected_start_type": START_TYPES[service["OriginalType"]],
    }]
    result.extend(expected_registry_operation(record, i) for i, record in enumerate(registry))
    return result


def validate(review, source, source_digest):
    require(set(review) == {"review_format", "review_only", "authorization", "source", "tweak", "review_notes"},
            "review envelope fields")
    require(review["review_format"] == "rustweaker-location-review-v1", "review format")
    require(review["review_only"] is True and review["authorization"] == "none",
            "review must be explicitly non-authorizing")
    require(source_digest == SOURCE_SHA256, "frozen source SHA-256 mismatch")
    require(review["source"] == {"path": SOURCE, "sha256": source_digest}, "source provenance")
    require(TWEAK_ID in source, f"source missing {TWEAK_ID}")
    entry = source[TWEAK_ID]
    # Closed source allowlist makes scripts, Values combos, AppX and future actions fail closed.
    require(set(entry) == SOURCE_TOP_KEYS, f"{TWEAK_ID}: unsupported/missing action or source fields")
    require(isinstance(entry["service"], list) and isinstance(entry["registry"], list),
            f"{TWEAK_ID}: service/registry declaration shape")

    tweak = review["tweak"]
    require(set(tweak) == {"id", "title", "summary", "source_id", "support", "source_declarations", "operations"},
            "tweak review fields")
    require(tweak["id"] == TWEAK_ID and tweak["source_id"] == TWEAK_ID,
            "tweak identity")
    require(tweak["title"] == entry["Content"], "tweak title differs from source")
    require(tweak["summary"] ==
            "Upstream declarative service and registry apply declarations; review data only, support unverified.",
            "tweak summary")
    require(tweak["support"] == {"status": "unverified"}, "support must remain unverified")

    declarations = tweak["source_declarations"]
    require(set(declarations) == {"service", "registry"}, "source declaration fields")
    require(declarations == {"service": entry["service"], "registry": entry["registry"]},
            "source declarations differ from frozen upstream (Original* retained as source data)")
    operations = tweak["operations"]
    require(isinstance(operations, list), "operations must be a list")
    for operation in operations:
        if operation.get("operation") == "set_service_start_type_if_expected":
            require(type(operation.get("start_type")) is int, "service start_type must be an integer")
            require(type(operation.get("expected_start_type")) is int,
                    "service expected_start_type must be an integer")
        if operation.get("operation") == "set_registry_value":
            value = operation.get("value", {})
            require(type(value.get("native_type")) is int, "registry native_type must be an integer")
            require(isinstance(value.get("bytes"), list)
                    and all(type(byte) is int for byte in value["bytes"]),
                    "registry bytes must be integer values")
    require(operations == expected_operations(entry),
            "typed operations mismatch or wrong service-before-registry order")
    require(all(type(byte) is int and 0 <= byte <= 255
                for operation in tweak["operations"] if operation["operation"] == "set_registry_value"
                for byte in operation["value"]["bytes"]), "registry bytes must be byte integers")
    require(review["review_notes"] == NOTES, "review notes must preserve the stated limits")


def expect_rejected(review, source, digest, label):
    try:
        validate(review, source, digest)
    except (KeyError, TypeError, ValueError):
        return
    raise ValueError(f"negative review check unexpectedly accepted: {label}")


def check_negative_cases(review, source, digest):
    cases = []

    changed = copy.deepcopy(review)
    changed["tweak"]["support"] = {"status": "verified", "min_ntddi": 0x06010000,
                                    "max_ntddi": None, "evidence": ["fabricated"]}
    cases.append((changed, source, "verified support"))

    changed = copy.deepcopy(review)
    changed["tweak"]["operations"][0], changed["tweak"]["operations"][1] = (
        changed["tweak"]["operations"][1], changed["tweak"]["operations"][0])
    cases.append((changed, source, "registry before service"))

    changed = copy.deepcopy(review)
    changed["tweak"]["operations"][0]["start_type"] = START_TYPES["Manual"]
    cases.append((changed, source, "OriginalType used as intended value"))

    changed = copy.deepcopy(review)
    changed["tweak"]["operations"][0]["expected_start_type"] = START_TYPES["Automatic"]
    cases.append((changed, source, "wrong expected start type"))

    changed = copy.deepcopy(review)
    del changed["tweak"]["operations"][0]["expected_start_type"]
    cases.append((changed, source, "missing service guard"))

    changed = copy.deepcopy(review)
    changed["tweak"]["operations"][0]["operation"] = "set_service_start_type"
    cases.append((changed, source, "unconditional service operation"))

    changed = copy.deepcopy(review)
    changed["tweak"]["operations"][0]["expected_start_type"] = True
    cases.append((changed, source, "boolean service guard"))

    changed = copy.deepcopy(review)
    changed["tweak"]["operations"][1]["value"]["bytes"] = [65, 0, 108, 0, 108, 0, 111, 0, 119, 0, 0, 0]
    cases.append((changed, source, "OriginalValue used as intended value"))

    changed = copy.deepcopy(review)
    changed["tweak"]["operations"].pop()
    cases.append((changed, source, "omitted registry declaration"))

    changed = copy.deepcopy(source)
    changed[TWEAK_ID]["InvokeScript"] = ["unreviewed action"]
    cases.append((review, changed, "additional source action field"))

    changed = copy.deepcopy(review)
    changed["authorization"] = "allow"
    cases.append((changed, source, "authorization flag"))

    for candidate, candidate_source, label in cases:
        expect_rejected(candidate, candidate_source, digest, label)
    return len(cases)


def main():
    source_bytes = (ROOT / SOURCE).read_bytes()
    source = json.loads(source_bytes.decode("utf-8-sig"), strict=False,
                        object_pairs_hook=unique_object)
    review = json.loads((ROOT / FIXTURE).read_text(encoding="utf-8"),
                        object_pairs_hook=unique_object)
    digest = hashlib.sha256(source_bytes).hexdigest().upper()
    validate(review, source, digest)
    rejected = check_negative_cases(review, source, digest)
    print(f"Source SHA-256: {digest}")
    print("Verified WPFTweaksLocation review data: service lfsvc Manual (SCM 3) -> Disabled (SCM 4), "
          "then 3 typed registry set declarations; OriginalType checked as guard, OriginalValue as source data; "
          "support unverified, no authorization or execution.")
    print(f"Negative checks rejected: {rejected}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, KeyError, TypeError, ValueError, AttributeError) as error:
        print(f"Location review verification failed: {error}", file=sys.stderr)
        sys.exit(1)
