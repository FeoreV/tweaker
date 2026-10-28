# Source audit: `WPFToggleDetailedBSoD`

**Scope.** This documents one ID in the frozen upstream catalog, `WPFToggleDetailedBSoD`. It is a source audit only; it does not implement, activate, or approve this tweak.

## Source facts

The frozen `winutil-26.08.19/config/tweaks.json` entry at lines 1226–1251 identifies the tweak as `BSoD Verbose Mode`, type `Toggle` (lines 1226–1232). It declares exactly two registry records, both at `HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl`:

| Lines | Value name | Type | Apply value | Configured undo value | DefaultState |
|---|---|---|---|---|---|
| 1233–1239 | `DisplayParameters` | `DWord` | `1` | `0` | `false` |
| 1241–1247 | `DisableEmoticon` | `DWord` | `1` | `0` | `false` |

The entry has no service, script, AppX, or combobox fields in this range. Its only remaining field is a documentation link (lines 1249–1251).

`winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1` maps undo to `OriginalValue` and apply to `Value` (lines 24–40). For each registry record, it iterates sequentially and calls `Set-WinUtilRegistry` with the selected field (lines 64–67). It does not query and capture the live previous values in this path.

`winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1` creates the registry path if absent (lines 33–37), then sets or removes the named value (lines 39–47). Its catch blocks warn and log errors (lines 49–62); they do not propagate a transaction/rollback mechanism. **Inference:** because writes are sequential and errors are caught per call, one record can be changed while another fails, leaving partial state. Creating a missing key is also not undone by this code.

`winutil-26.08.19/functions/private/Get-WinUtilToggleStatus.ps1` first returns a cached status if present (lines 5–10). Otherwise it checks every declared registry record, reads the value when the path exists (lines 17–23), substitutes `Value` or `OriginalValue` for missing data according to `DefaultState` (lines 25–30), and compares each result to `Value`, returning false on the first mismatch (lines 32–39). For this entry, a missing value is treated as its configured `OriginalValue` (`0`) and therefore as off.

`winutil-26.08.19/pester/toggle-status.Tests.ps1` defines generic mock registry fixtures and mocks the registry accessors (lines 15–45); it tests missing-path/default and cache behavior (lines 52–72). These fixtures are not the BSoD entry and do not exercise the Windows registry or establish OS support.

## Analysis and limits

`OriginalValue: "0"` is static catalog data, not a captured pre-change snapshot. Undo writes zero to both names; it cannot restore an absent value, a prior nonzero value, a value of another registry type, or key absence. The helper creates an absent key and this path has no corresponding key deletion. Thus the inventory's “undo yes” claim is not evidence of exact state restoration.

`TWEAKS_INVENTORY.md` line 95 lists only `DisplayParameters`, omitting `DisableEmoticon`. It also labels the entry “Compatible”, “Direct Win32”, and “Yes” for undo. Those labels are unsupported for this entry by the cited source: the source shows two PowerShell registry writes and no platform test or live-state backup/restore contract. No Windows version compatibility conclusion, including Windows 7 compatibility, follows from this code or the mocked Pester fixtures alone.

This audit does not establish what the settings do on any Windows release, whether either value is recognized, whether a restart is needed, or whether the configuration is appropriate for a particular machine. Those questions remain unverified here.

## Source locations and SHA-256

Line numbers refer to the frozen checkout files inspected for this audit.

| Source | Relevant lines | SHA-256 |
|---|---:|---|
| `winutil-26.08.19/config/tweaks.json` | 1226–1251 | `CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9` |
| `winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1` | 24–40, 64–67 | `27E7DEAF9B48ECC74085F980AD82DD9CD9DCBBCBA0C7E9D33F58C9475BCF510B` |
| `winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1` | 30–62 | `DF2251738B3B0AD0360147F99267D33718C02F37AF77579BD34D29958BCBE726` |
| `winutil-26.08.19/functions/private/Get-WinUtilToggleStatus.ps1` | 1–39 | `E8A9775E129918C3E53D6CCE10A8215EBC6745917C7E73542B7144B387B0EBBF` |
| `winutil-26.08.19/pester/toggle-status.Tests.ps1` | 15–45, 52–72 | `6F4CAA75864964D92B056D63745291D6D3EA4259CE02405A02960B45E4E90E9E` |
| `TWEAKS_INVENTORY.md` | 95 | `9D279831C03E076010C4B14EA98336F537E75C815B4EF22454079FF8B098E720` |

All hashes above were computed with `Get-FileHash -Algorithm SHA256`.
