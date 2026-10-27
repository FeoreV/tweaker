# Upstream custom script action audit

Reviewed 2026-10-28 by reading the frozen `config/tweaks.json` scripts and referenced helper implementations. No script was executed. This documents source behavior, not an approved port. Fixed `OriginalValue`/`OriginalType` declarations are not machine snapshots; an `UndoScript` name alone does not establish rollback.

## Findings by entry

| ID | Apply action observed | UndoScript / relevant undo behavior | Porting / rollback finding |
|---|---|---|---|
| `WPFTweaksHiber` | `powercfg.exe /hibernate off` | Runs `/hibernate on` | Sets a fixed state; does not capture whether hibernation was already off. |
| `WPFTweaksWidget` | Stops matching processes; removes two AppX packages; restarts Explorer | No `UndoScript`; dispatcher has no AppX undo path | Package removal is destructive and has no catalog undo. |
| `WPFTweaksDisableStoreSearch` | `icacls /deny Everyone:F` on the current user's Store database | `icacls /grant Everyone:F` | Does not restore the original ACL; grant is broad and may not match prior permissions. |
| `WPFTweaksServices` | Reads physical-memory capacity with PowerShell `Get-CimInstance Win32_PhysicalMemory`; writes `SvcHostSplitThresholdInKB` in addition to service declarations | No `UndoScript`; generic dispatcher uses declared service `OriginalType`, while the script-written threshold has no declared original value | Relies on a CIM query. Do not carry this query into Rustweaker; use the project's required direct Win32 memory API and capture the prior registry state, or omit the feature. |
| `WPFTweaksTelemetry` | Changes Defender sample consent and services; sets a machine environment variable; removes `HKCU\Software\Microsoft\Siuf\Rules\PeriodInNanoSeconds` | Applies fixed consent/startup/environment values | Undo omits restoration of the removed SIUF property and does not restore observed service/environment values. |
| `WPFTweaksRemoveEdge` | Creates an Edge system-app path, runs setup uninstall with `--delete-profile` | `winget install Microsoft.Edge` | Profile deletion is destructive; reinstall does not restore profile or prior installation/version. |
| `WPFTweaksDisableBitLocker` | `Disable-BitLocker` on the system drive | `Enable-BitLocker` on the system drive | Encryption/protection state and progress are not snapshotted; enabling is not an exact inverse of the observed prior state. |
| `WPFTweaksRemoveOneDrive` | Uninstalls OneDrive, stops processes, deletes two data directories, changes/re-grants ACL rights, may delete the user folder/environment variable, disables `OneSyncSvc` | Installs through winget and sets `OneSyncSvc` Automatic | Data deletion and ACL/service changes are not restored; fixed Automatic is not the observed startup type. |
| `WPFTweaksDisplay` | Sets binary `UserPreferencesMask` | Removes `UserPreferencesMask` | Removal does not restore a pre-existing binary value. |
| `WPFTweaksReservedStorage` | DISM disables reserved storage | DISM enables it | Fixed inverse; no prior state capture or interruption/recovery contract. |
| `WPFTweaksRestorePoint` | May enable system restore, then creates a checkpoint | No `UndoScript`; generic dispatch can restore the declared registry value only | Creating the restore point itself has no inverse; system-restore configuration still needs a captured-state contract. |
| `WPFTweaksWindowsAI` | Creates Appx EndOfLife marker; removes AppX/winget packages; changes service and optional-feature state | No `UndoScript`; generic registry dispatch may use catalog originals, but script effects are not reversed | Multiple system-level removals/state changes; do not expose as a tweak without a separate recovery design. |
| `WPFTweaksRazerBlock` | Deletes files under an existing Razer installer directory (or creates it), then denies writes | Removes the deny ACE | File deletion is not restored; ACL undo does not recover contents. |
| `WPFTweaksBlockAdobeNet` | Downloads a mutable hosts list, appends it to the system hosts file, flushes DNS | Rewrites hosts content using a marker-based regex and flushes DNS | No pre-state snapshot; result depends on remote content and marker shape. Must not be treated as an exact append/undo pair. |
| `WPFTweaksRightClickMenu` | Creates a CLSID/InprocServer32 registry tree and stops Explorer | Deletes the entire tree | Could delete pre-existing values/tree; no state snapshot or conflict check. |
| `WPFTweaksDiskCleanup` | Runs cleanmgr and `DISM /Cleanup-Image /StartComponentCleanup /ResetBase` | No `UndoScript` or declarative registry/service undo | `/ResetBase` removes uninstallability of superseded updates; no rollback. |
| `WPFTweaksDeleteTempFiles` | Recursively removes contents of user and Windows temp directories | No `UndoScript` or declarative registry/service undo | Destructive filesystem deletion; no file backup or undo. |
| `WPFTweaksTeredo` | Sets Teredo state to disabled | Sets state to default | “Default” is not the captured prior configuration. |
| `WPFTweaksDisableIPv6` | Disables IPv6 binding on all adapters | Enables IPv6 binding on all adapters | Undo enables bindings that may have been disabled before apply. Requires per-adapter snapshot. |
| `WPFTweaksDisableExplorerAutoDiscovery` | Deletes `Bags` and `BagMRU`; creates `FolderType=NotSpecified` | Deletes `Bags` and `BagMRU` again | Undo repeats destructive deletion and does not restore the deleted view database or prior FolderType. |
| `WPFToggleDarkMode` | Refreshes Explorer and conditionally resets theme to Auto | Same script | Script bodies are identical UI/theme helper actions; they do not themselves reverse the declarative registry values. |
| `WPFToggleShowExt` | Restarts/refreshes Explorer | Same script | UI refresh, not a restoration operation. |
| `WPFToggleHiddenFiles` | Restarts/refreshes Explorer | Same script | UI refresh, not a restoration operation. |
| `WPFToggleStartMenuRecommendations` | Restarts/refreshes Explorer | Same script | UI refresh, not a restoration operation. |
| `WPFToggleTaskbarAlignment` | Restarts/refreshes Explorer | Same script | UI refresh, not a restoration operation. |

The inventory comparison records 25 entries with `InvokeScript` and 19 with `UndoScript`; this table traces all entries containing either field. Six have no `UndoScript`: Widgets, Services, Restore Point, Windows AI, Disk Cleanup and Delete Temp Files.

## Implications for rustweaker

- Never translate upstream undo literals into transaction backups. Capture exact state before each mutation and refuse the operation if the backup cannot be durably committed.
- Keep destructive actions (profile/data/package/file deletion, DISM ResetBase, BitLocker transitions and broad ACL edits) out of the initial supported catalog unless an operation-specific recovery contract and tests exist.
- Do not port the CIM memory query. Use supported direct Win32 APIs if the feature is retained; otherwise omit the operation.
- For remote downloads, define provenance, integrity verification, licensing, timeout, and offline behavior before implementation.
- These findings are source review only. They do not authorize live mutation and do not establish per-Windows-version support.
