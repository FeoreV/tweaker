# Review of draft/upstream tweak ID divergences

Reviewed 2026-10-28 against the read-only frozen source. This is a manual semantic review of selected candidate pairs generated from the exact-ID comparison. A similar name is not an alias. “Feature correspondence” below only identifies likely subject matter; it does not approve the draft behavior, compatibility claims, or undo semantics.

## Supported correspondences and limits

| Upstream ID | Draft ID | Finding | Evidence and limit |
|---|---|---|---|
| `WPFTweaksDisableBGapps` | `WPFTweaksDisableBGApps` | Same feature and registry target; ID casing differs. | The upstream declaration and draft both target `HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications\GlobalUserDisabled`, applying `1`. Upstream `OriginalValue` is `0`; the draft additionally claims `DeleteValue` as an undo alternative, which is not in this declaration. This supports a feature mapping, not the extra delete behavior. |
| `WPFToggleMouseAcceleration` | `WPFToggleMouseAccel` | Same three registry values and apply/undo literals; ID is abbreviated. | Upstream declares `MouseSpeed=1`, `MouseThreshold1=6`, `MouseThreshold2=10`, each `DWord`, with each `OriginalValue=0`. Draft names the same three values and numbers. Its compatibility and “undo yes” claims remain unverified. |
| `WPFAddUltPerf` | `WPFTweaksAddUltPerf` | Same apparent enable action; implementation is a UI button handler, not catalog scripts. | `functions/public/Invoke-WPFButton.ps1` routes the upstream ID to `Invoke-WPFUltimatePerformance -Enable`; `functions/public/Invoke-WPFUltimatePerformance.ps1` duplicates and activates the Ultimate Performance plan. Draft describes that general action, but its claimed Win7 fallback is absent from this handler. Do not assert Win7 support from this pairing. |
| `WPFRemoveUltPerf` | `WPFTweaksRemoveUltPerf` | Same apparent disable/reset topic, but materially different operation. | Upstream routes to `Invoke-WPFUltimatePerformance` without `-Enable`; that helper runs `powercfg /restoredefaultschemes`. Draft says it activates the Balanced scheme only. These operations are not interchangeable; the row must not be implemented as an exact upstream port. |
| `WPFMultiplaneOverlay` | `WPFToggleMultiplaneOverlay` | Same feature; draft describes only part of the source declaration. | Upstream has two registry records: `OverlayTestMode` with three UI values and `GraphicsDrivers\DisableOverlays` with separate values. Draft mentions only `OverlayTestMode=5`. Treat as partial coverage, not equivalent behavior. |
| `WPFOOSUbutton` | `WPFTweaksOO` | Same O&O ShutUp10++ launch feature; transport detail in the draft is inaccurate. | `Invoke-WPFButton.ps1` dispatches the upstream ID to `Invoke-WPFOOSU`; that handler downloads `OOSU10.exe` from O&O using `Save-WinUtilFile`, whose implementation uses .NET `System.Net.WebRequest`, then starts the executable. The draft says WinHTTP. This correspondence does not establish current download availability, binary integrity, licensing, or Win7 support. |

## Candidate pairs rejected as equivalent on current evidence

| Upstream ID | Draft ID | Reason |
|---|---|---|
| `WPFTweaksWidget` | `WPFToggleTaskbarWidgets` | Upstream stops widget processes and removes `Microsoft.WidgetsPlatformRuntime` and `MicrosoftWindows.Client.WebExperience` AppX packages. Draft toggles `TaskbarDa`. Removal and taskbar-button visibility are different actions; the upstream entry has no undo script. |
| `WPFTweaksDisableWarningForUnsignedRdp` | `WPFTweaksUnsignedRDP` | Upstream sets `RedirectionWarningDialogVersion` and `RdpLaunchConsentAccepted`; draft sets `AuthenticationLevelOverride`. Warning/consent behavior and authentication-level behavior are distinct settings. |
| `WPFTweaksRemoveHomeAndGallery` | `WPFTweaksRemoveHome` + `WPFTweaksRemoveGallery` | The subject overlaps, but upstream is one tweak with three per-user registry values: two CLSID pin-state values plus `Explorer\Advanced\LaunchTo`. The draft splits it into two entries and claims key deletion/recreation. That is not the same declared operation or undo contract. |
| `WPFTweaksRemoveOneDrive` | `WPFTweaksRemoveOnedrive` | Same broad product subject, but not equivalent operation. Upstream script uninstalls OneDrive, removes two data directories, stops processes, changes then re-grants ACL rights, may remove the user OneDrive directory/environment variable, and disables `OneSyncSvc`. Its undo runs `winget install` and sets the service Automatic. The draft instead claims a policy registry value plus uninstall/reinstall. Do not map or port without a separate design and data/ACL/service recovery review. |
| `WPFTweaksPreventDeviceMetadataFromNetwork` | `WPFTweaksDeviceMetadata` | The value name overlaps, but source content says “Prevent Device Companion Apps” and uses `HKLM\SOFTWARE\Policies\Microsoft\Windows\Device Metadata`; draft claims `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Device Metadata`. Path and stated intent differ. Needs primary behavior validation before any mapping. |

## Remaining unmatched IDs

The other source-only IDs and draft-only IDs remain unmapped. Draft-only rows need their own provenance; their presence in this inventory does not establish upstream origin. Source-only entries need independent review before deciding whether they are out of scope, omitted intentionally, or need new draft rows. The generated [`inventory-source-comparison.json`](inventory-source-comparison.json) is the complete exact-ID inventory of both sets. No fuzzy-match candidate is approved implicitly by this report.

No upstream file was modified. The findings are source-code/catalog tracing only; none is evidence of live Windows behavior or permission to mutate the system.
