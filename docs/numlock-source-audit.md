# Source audit: `WPFToggleNumLock`

**Scope:** source-only review of this one exact identifier. This records what the checked-in frozen reference, draft inventory, and current Rust fixture say; it does not activate or implement the tweak. The `winutil-26.08.19/` tree was read only.

## Source facts

The frozen upstream catalog defines `WPFToggleNumLock` with content `Num Lock on Startup`, type `Toggle`, and two ordered registry entries. Both target `InitialKeyboardIndicators`, use string value `"2"`, fixed `OriginalValue` `"0"`, and `DefaultState` `"false"`: first `HKU:\.Default\Control Panel\Keyboard` (JSON lines 1518–1532), then `HKCU:\Control Panel\Keyboard` (lines 1533–1540). These are distinct literal paths. `HKCU` is a context-dependent hive path; this source does not establish which account/context is selected at runtime. The `.Default` path is reported literally; this audit does not assign it a user identity or infer its semantics. [Catalog](../winutil-26.08.19/config/tweaks.json)

The draft inventory row 85 (entry 48) lists only the `.DEFAULT` path and marks the tweak “Compatible”, “Direct Win32”, and undo-supported. It omits the catalog's second, `HKCU` operation. Those labels are inventory assertions, not evidence that Windows versions, execution contexts, Win32 implementation, or restoration have been verified. [Inventory](../TWEAKS_INVENTORY.md)

`Invoke-WinUtilTweaks` chooses `Value` for apply and `OriginalValue` for undo (lines 27–40), then iterates every registry entry in order and calls `Set-WinUtilRegistry` (lines 64–67). The setter creates a missing key path (lines 33–37), attempts the value write, and catches/logs errors (lines 39–62); it does not persist a durable before-image or report transactional rollback. Consequently the two writes may partially succeed. If a target value was absent or had a nonzero value, different type, or different raw representation before apply, undo's fixed string `"0"` cannot reconstruct that prior state. If a key was created, the described undo sets a value but does not remove that created key. [Handler](../winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1) · [Setter](../winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1)

`Get-WinUtilToggleStatus` first returns a cached status if present, otherwise traverses all registry records (lines 1–18). For each record it reads the value if the path exists; when missing, it substitutes `Value` for `DefaultState=true` or `OriginalValue` for `false` (lines 19–30), and compares that result with the configured `Value` (lines 32–39). Thus a missing record with this entry's false default compares as `"0"` versus `"2"`, and status is false. The current-system helper calls this status function when processing toggle entries (lines 52–70); this shows a consumer of toggle status, not a system behavior guarantee. [Status helper](../winutil-26.08.19/functions/private/Get-WinUtilToggleStatus.ps1) · [Current-system helper](../winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1)

The Rust fixture classifier records `WPFToggleNumLock` as rejected at operation index 1, checks that this operation targets `Hive::CurrentUser`, and expects plan construction to reject that operation (test source lines 1054, 1075–1085). This is fixture/test evidence for the current plan gate only; it is not production execution or Windows behavior evidence. [Fixture source](../src/engine/tweak_plan.rs)

## Inferences and unknowns

- **Inference:** because the upstream helper issues separate sequential calls and its setter catches failures without a durable transaction, success of the first operation does not establish success of the second, and the helper provides no all-or-nothing guarantee.
- **Unknown:** the identities or user/session effects associated with the two registry paths in a particular invocation; no claim is made that they refer to the same user or produce system-wide behavior.
- **Unknown:** runtime behavior on any Windows release, including whether either configured value produces the described startup effect. No Windows execution or version-support evidence was examined.
- **Unknown:** whether the draft inventory's “Compatible”, “Direct Win32”, and undo claims hold. The cited implementation is PowerShell source, while the inventory's “Direct Win32” label does not identify a verified implementation for this entry.
- **Unknown:** exact prior registry state on any machine and whether the entries/keys exist there.

## Source identity

SHA-256 hashes below are for the exact files cited above, calculated from the current checkout. Line references identify the reviewed source locations.

| File | Reviewed lines | SHA-256 |
|---|---:|---|
| `winutil-26.08.19/config/tweaks.json` | 1518–1543 | `CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9` |
| `winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1` | 27–40, 64–67 | `27E7DEAF9B48ECC74085F980AD82DD9CD9DCBBCBA0C7E9D33F58C9475BCF510B` |
| `winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1` | 33–62 | `DF2251738B3B0AD0360147F99267D33718C02F37AF77579BD34D29958BCBE726` |
| `winutil-26.08.19/functions/private/Get-WinUtilToggleStatus.ps1` | 1–40 | `E8A9775E129918C3E53D6CCE10A8215EBC6745917C7E73542B7144B387B0EBBF` |
| `winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1` | 52–70 | `FE3D566A8EB751288449E5CDEE3D8F3D8AE619DF10D171E5EAF0FD0E90B3DCC1` |
| `TWEAKS_INVENTORY.md` | 85 | `9D279831C03E076010C4B14EA98336F537E75C815B4EF22454079FF8B098E720` |
| `src/engine/tweak_plan.rs` | 1054, 1075–1085 | `C1D2B1FDD44D2B6CD9D8B116FAC81A6AE10FC7C3BA3DC05B1EA48E93A9953C13` |

The original source audit itself ran no scripts or tests and changed no source, schema, inventory, or upstream files. The later Rust observer is recorded separately below.

## Rust observer handoff — 2026-10-28

`src/engine/numlock_status.rs` now implements a read-only observer for this exact ID, and `--toggle-status WPFToggleNumLock <native|64|32>` exposes it on Windows. The observer reads both source targets in order, but reports a narrower interpretation than PowerShell: missing values use the configured off default; present strings are interpreted only as valid UTF-16 `REG_SZ`; unreadable, malformed, or unsupported data is `unknown`, unless another record gives a definite mismatch. This does not preserve the PowerShell cache or coercion behavior. Reports include target identity and read-state but omit raw registry bytes. No writes, effect claims, or Windows-version support claims are made. Stable tests, strict Clippy, fmt and Win7 target builds are recorded in `AGENT_TASKS.md`; no Win7 VM or live NumLock behavior was tested.
