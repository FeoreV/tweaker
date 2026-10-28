# Source audit: `WPFTweaksLocation`

**Scope:** source-only audit of this exact upstream identifier. This records checked-in declarations, handler behavior, draft inventory and review-fixture facts; it does not implement, approve, or activate the tweak. `winutil-26.08.19/` was read only. No scripts or tests were run.

## Source facts

The frozen upstream catalog entry names `WPFTweaksLocation`, “Location Tracking - Disable,” and declares service `lfsvc` with apply `Disabled` and static `OriginalType` `Manual` (JSON lines 109–120). It then declares three ordered registry writes (lines 121–142): `HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location\Value` as string `Deny` (`OriginalValue` `Allow`); `HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Sensor\Overrides\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}\SensorPermissionState` as DWORD 0 (`OriginalValue` 1); and `HKLM:\SYSTEM\Maps\AutoUpdateEnabled` as DWORD 0 (`OriginalValue` 1). These are catalog declarations, not evidence of machine state or successful effects. [Catalog](../winutil-26.08.19/config/tweaks.json)

`Invoke-WinUtilTweaks` defaults `KeepServiceStartup` to true (lines 18–22), selects `Value`/`StartupType` for apply and `OriginalValue`/`OriginalType` for undo (lines 27–40), and handles service declarations before registry declarations. During apply with the default guard, it queries the service; when `StartType` differs from the catalog's `OriginalType`, it suppresses the service setter (lines 42–53). A `ServiceNotFoundException` is caught without clearing `$changeservice`, so the code still calls `Set-WinUtilService` (lines 44–61). Undo bypasses this guard, because it runs only when `!$undo` (line 47). Registry declarations are then independently iterated in order (lines 64–67). [Handler](../winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1)

`Set-WinUtilRegistry` creates a missing registry path (lines 33–37), then sets or removes the requested value (lines 39–48); its catch blocks warn/log failures (lines 49–62). It does not persist a before-image transaction. `Set-WinUtilService` re-queries the service and returns without writing if its current `StartType` already equals the requested type (lines 25–32), otherwise attempts the service change; its catch block warns/logs failures, including a missing service (lines 34–52). It also has no transaction persistence. [Registry setter](../winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1) · [Service setter](../winutil-26.08.19/functions/private/Set-WinUtilService.ps1)

The current-system helper's registry path checks compare each current value to the declared apply value and mark registry-backed toggles false if not all values match (lines 75–106). For services, it compares `Get-Service`'s `StartType` to the declared `StartupType` when the service exists (lines 109–120). This is status/reporting logic; it does not establish whether the configured writes produce the described system effect. [Current-system helper](../winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1)

The draft inventory row 42 lists only the first registry value plus `lfsvc`; it omits the Sensor override and Maps auto-update writes while marking the item “Compatible,” “Direct Win32,” and undo-supported. Those labels are inventory assertions, not verification evidence. [Inventory](../TWEAKS_INVENTORY.md)

The review fixture records support as `unverified` and authorization as `none` (lines 2–16), includes all four source declarations (lines 17–47), and encodes one conditional service operation followed by three registry operations (lines 49–116). Its service condition is `start_type: 4`, `expected_start_type: 3` (lines 51–56). `src/data/catalog.rs` has a decoder test asserting unverified support and four operations in source order (lines 275–291); `src/engine/tweak_plan.rs` exercises conditional operation behavior through mock backends (lines 694–773). This is fixture/test evidence only, not a production loader, activation path, Windows behavior proof, or support approval. [Review fixture](../config/catalog.location-review.v1.json) · [Decoder test](../src/data/catalog.rs) · [Plan tests](../src/engine/tweak_plan.rs)

## Inferences and unknowns

- **Inference — partial success:** service and registry operations are separate sequential calls, and registry failures are caught per setter. A service change may succeed while any subset of the three registry writes fails, or vice versa. The handler provides no all-or-nothing transaction or rollback guarantee.
- **Inference — static originals are not snapshots:** `OriginalType` and each `OriginalValue` are fixed catalog literals. The handler selects them during undo; it does not query and persist each target's actual pre-apply state. Undo can therefore restore those literals rather than the exact prior state, and can miss absent values, differing data/types, or created keys. A skipped service apply (current startup type differs from `Manual`) does not prevent the independent registry writes; a later undo still attempts to set the service to `Manual`.
- **Inference — conditional fixture semantics:** its expected startup type represents a guard condition encoded by the review fixture, whereas upstream `OriginalType` is merely a fixed undo declaration plus the default apply guard's expected value. The fixture does not change upstream behavior or confer authorization.
- **Unknown:** Windows-version support and actual effects of any of these values/service settings. No Windows/SCM execution or platform evidence was reviewed.
- **Unknown:** exact registry values/types/key existence and service startup state on any target machine, as well as each operation's runtime success.
- **Unknown:** whether the draft's compatibility, direct-Win32, or undo claims are valid. The cited handler/setters are PowerShell and do not prove a Rust implementation or transactional recovery.

## Source identity

SHA-256 hashes below were computed from the exact current files cited above. Line references identify the reviewed source locations.

| File | Reviewed lines | SHA-256 |
|---|---:|---|
| `winutil-26.08.19/config/tweaks.json` | 109–145 | `CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9` |
| `winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1` | 18–67 | `27E7DEAF9B48ECC74085F980AD82DD9CD9DCBBCBA0C7E9D33F58C9475BCF510B` |
| `winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1` | 30–62 | `DF2251738B3B0AD0360147F99267D33718C02F37AF77579BD34D29958BCBE726` |
| `winutil-26.08.19/functions/private/Set-WinUtilService.ps1` | 21–53 | `C20752477C0B1883AC82DCA39E645272BADC506F6E36EFF1C83011F1FBE667DA` |
| `winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1` | 64–125 | `FE3D566A8EB751288449E5CDEE3D8F3D8AE619DF10D171E5EAF0FD0E90B3DCC1` |
| `TWEAKS_INVENTORY.md` | 42 | `9D279831C03E076010C4B14EA98336F537E75C815B4EF22454079FF8B098E720` |
| `config/catalog.location-review.v1.json` | 1–116 | `1F4F366BB1067AB1FE7CF81AE43FFFCC3DF236ED1BADDE3055D960B156304EC9` |
| `src/data/catalog.rs` | 275–291 | `83691F2D5ED303804491D33118481DF6C5B2A511D82279C9A7569D228B99DE7D` |
| `src/engine/tweak_plan.rs` | 694–773 | `C1D2B1FDD44D2B6CD9D8B116FAC81A6AE10FC7C3BA3DC05B1EA48E93A9953C13` |

## Rust observer handoff — 2026-10-28

`src/engine/location_status.rs` and `--location-status <native|64|32>` now observe this exact entry through read-only registry and SCM configuration queries. The observer checks the three registry values in current-system helper order, then `lfsvc`. It follows the source's missing-registry fallback to static originals and explicitly skips a missing service because the helper only checks `StartType` when `Get-Service` returned an object. Read/query errors and malformed or unsupported values are reported as unknown unless another observation establishes a mismatch. `Deny` uses ASCII case-insensitive matching; this and conservative malformed/error handling do not reproduce all PowerShell coercion, culture, or nonterminating-error behavior. The report includes no raw registry bytes and is non-executable. No live registry/SCM operation or effect/OS-support claim was made; verification details are in `AGENT_TASKS.md`.
