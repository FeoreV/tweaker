# Source audit: `WPFTweaksUTC`

**Scope:** source-only review of the exact ID `WPFTweaksUTC`. This document records declarations and code paths; it does not authorize or perform activation. No scripts or tests were executed. The frozen `winutil-26.08.19/` tree was read only.

## Findings

The upstream catalog has one registry declaration for `HKLM:\SYSTEM\CurrentControlSet\Control\TimeZoneInformation`, value `RealTimeIsUniversal`, `Value` string `"1"`, `Type` `QWord`, and `OriginalValue` string `"0"`. The entry itself has no `Type` field (Toggle or otherwise). Its description says it is for dual-boot time synchronization; that wording is a catalog description, not evidence of operating-system behavior. Source: [`tweaks.json`](../winutil-26.08.19/config/tweaks.json), lines 626–641.

The generic upstream tweak path chooses `Value` on apply and `OriginalValue` on undo, then passes the configured `Type` and selected value to `Set-WinUtilRegistry`. The setter creates a missing registry path, sets the named value, and catches/logs errors. These sources describe the script implementation; they do not establish successful behavior on any Windows version. Sources: [`Invoke-WinUtilTweaks.ps1`](../winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1), lines 25–39 and 64–68; [`Set-WinUtilRegistry.ps1`](../winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1), lines 30–62.

The current-system status path treats entries without `Type == "Toggle"` as non-toggle entries. It counts registry values equal to `Value`; when a value is missing it substitutes `DefaultState` when present, otherwise `OriginalValue`. This UTC entry has no `DefaultState`, so that path uses its declared `OriginalValue` for a missing value. This is status computation, not a snapshot of prior native type/data. Source: [`Invoke-WinUtilCurrentSystem.ps1`](../winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1), lines 64–105.

The draft inventory row is inconsistent with the upstream entry: it calls the item `Toggle`, labels `1` as `DWORD`, and gives undo as `0 / DeleteValue`. The cited upstream entry instead has `Type: QWord`, no entry-level `Type` field, and `OriginalValue: "0"`; it does not declare `<RemoveEntry>` or a deletion operation. The row's compatibility and direct-Win32 claims are inventory assertions, not verified by the sources reviewed here. Source: [`TWEAKS_INVENTORY.md`](../TWEAKS_INVENTORY.md), line 73.

The review fixture marks support `unverified` and encodes one `set_registry_value` operation with `native_type: 11` and eight data bytes `[1,0,0,0,0,0,0,0]`. This encoding represents a QWORD-shaped payload in the fixture. The associated Rust test lists `WPFTweaksUTC`, accepts fixture operations through a mock backend, initializes registry state as `ValueMissing`, applies and recovers the planned operations, then checks equality with that mock initial state. That is fixture/plan mock coverage only; it does not demonstrate Windows registry writes, RTC semantics, supported OS versions, or a production catalog/executor. Sources: [`catalog.registry-review.v1.json`](../config/catalog.registry-review.v1.json), lines 1093–1123; [`tweak_plan.rs`](../src/engine/tweak_plan.rs), lines 1038 and 1075–1114.

## Recovery and type-fidelity limits

The upstream fixed undo value `0` cannot by itself restore an originally absent value, an originally absent key, or arbitrary prior native type and data. The apply setter can create a missing key; the reviewed generic undo path supplies a value but contains no corresponding exact pre-state capture or key cleanup. Thus these declarations do not provide exact reversible-state semantics. A transactional implementation would need to capture and persist presence, native type, and exact data before mutation, and define safe cleanup for keys it creates.

Native type fidelity matters here: the declaration explicitly says `QWord`, and the review fixture represents eight bytes. Recasting it as `DWORD` would not preserve the declared type. This audit does not claim that the declaration is accepted by Windows, that it changes RTC behavior, or that any particular Windows release supports the intended behavior; those remain unverified.

## Source hashes

SHA-256 hashes below cover the complete files at audit time (not just cited line ranges):

| Source | SHA-256 |
|---|---|
| `winutil-26.08.19/config/tweaks.json` | `CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9` |
| `winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1` | `27E7DEAF9B48ECC74085F980AD82DD9CD9DCBBCBA0C7E9D33F58C9475BCF510B` |
| `winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1` | `DF2251738B3B0AD0360147F99267D33718C02F37AF77579BD34D29958BCBE726` |
| `winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1` | `FE3D566A8EB751288449E5CDEE3D8F3D8AE619DF10D171E5EAF0FD0E90B3DCC1` |
| `TWEAKS_INVENTORY.md` | `9D279831C03E076010C4B14EA98336F537E75C815B4EF22454079FF8B098E720` |
| `config/catalog.registry-review.v1.json` | `65DA7031FCBD00569F76BCAD8FD4845609DE99BCD3DC1DD2C362B1A714D73C32` |
| `src/engine/tweak_plan.rs` | `C1D2B1FDD44D2B6CD9D8B116FAC81A6AE10FC7C3BA3DC05B1EA48E93A9953C13` |

## Rust observer handoff — 2026-10-28

`src/engine/utc_status.rs` and the `--utc-status <native|64|32>` CLI command now expose a read-only observer for this exact declaration. It accepts only the audited `REG_QWORD` type and eight-byte little-endian value, uses the caller-selected registry view, and reports `checked`, `unchecked`, or `unknown`. The JSON includes target/read-state metadata, not the raw value bytes. It is explicitly non-executable and does not claim an OS effect or support. No live registry access was performed; verification details are in `AGENT_TASKS.md`.
