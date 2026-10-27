---
last_updated: 2026-10-28
maintained_by: Gemini 4 Argon (auto-research) & Antigravity
status: provisional-unverified
title: Provisional Windows System Tweaks Inventory
project: rustweaker
---

> **Project status (2026-10-28):** A read-only development CLI and standalone UI preview exist. Journal/recovery and guarded Win32 mutation prototypes are present, but tweak-application actions are not exposed by the app. This inventory does not describe implemented behavior. Validate every proposed entry against source, contracts, and tests before implementation. See [AGENT_START.md](AGENT_START.md).


# Tweaks Inventory: Unverified 66-Entry Draft

> **Do not use this document as an implementation spec, compatibility policy, or evidence that any operation is implemented.** It contains unverified proposed summaries. The reproducible facts currently checked are only the upstream entry counts and the named discrepancies in [the source audit](docs/source-inventory.md). Per-entry registry data, scripts, undo behavior, Windows compatibility, and “port complexity” must be checked against the frozen source and actual platform behavior before use.
> Cross-links: [ARCHITECTURE.md](file:///d:/pen1s/projects/tweaker/ARCHITECTURE.md) | [CODEBASE.md](file:///d:/pen1s/projects/tweaker/CODEBASE.md)

---

## 1. Historical estimates — not verified compatibility or implementation counts

The table below preserves estimates from an earlier draft. None of these compatibility or port-complexity totals has been independently established. The application has no tweak-application UI/CLI path, so “enabled,” “displayed,” and “automatically skipped” are not current behavior.

| Compatibility Category | Tweak Count | Percentage | Technician Handling Rule in rustweaker |
|:---|:---:|:---:|:---|
| Windows 7 SP1 | 40 (historic estimate) | Unverified | No support/enablement claim. |
| Windows 10+ only | 20 (historic estimate) | Unverified | No runtime policy exists. |
| Windows 11 only | 6 (historic estimate) | Unverified | No runtime policy exists. |
| Port complexity | 59 Direct Win32, 6 Hybrid PS, 1 External Download (historic estimate) | Unverified | Not an implementation plan. |

---

## 2. Draft summaries for source-by-source audit

Each row is a lead to investigate, not a confirmed upstream transcription. For example, `WPFTweaksHiber` is known to have two registry entries plus invoke/undo scripts, while this draft row lists one registry path and calls the operation Direct Win32. `WPFTweaksActivity` has upstream `<RemoveEntry>` original values, which are not equivalent to restoring a captured pre-state. See [the source audit](docs/source-inventory.md). Do not implement rows directly from this table.

| # | Tweak ID | Display Name | Category | Type | Registry Path & Key / Service / Operations | Enabled Value | Original / Disabled Value | Win7 Compat | Port Complexity | Undo Support |
|:---:|:---|:---|:---|:---:|:---|:---|:---|:---:|:---:|:---:|
| 1 | `WPFTweaksRestorePoint` | Create Restore Point | Essential Tweaks | Toggle | Invokes `srclient.dll!SRSetRestorePointW` + sets `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore` -> `SystemRestorePointCreationFrequency` | `0` (DWORD) + Create RP | `1440` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ❌ No |
| 2 | `WPFTweaksActivity` | Disable Activity History | Essential Tweaks | Toggle | `HKLM\SOFTWARE\Policies\Microsoft\Windows\System` -> `EnableActivityFeed`, `PublishUserActivities`, `UploadUserActivities` | `0` (DWORD) | DeleteValue / `1` | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 3 | `WPFTweaksConsumerFeatures` | Disable Consumer Features | Essential Tweaks | Toggle | `HKLM\SOFTWARE\Policies\Microsoft\Windows\CloudContent` -> `DisableWindowsConsumerFeatures` | `1` (DWORD) | DeleteValue / `0` | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 4 | `WPFTweaksTelemetry` | Disable Telemetry | Essential Tweaks | Toggle | `HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection` -> `AllowTelemetry`; `HKLM\...\Policies\DataCollection` -> `AllowTelemetry`; Services `DiagTrack`, `dmwappushservice` | `0` (DWORD); Services Disabled (`4`) | `1` / `3` (DWORD); Services Auto (`2`) | ✅ Compatible *(DiagTrack in Win7 via KB3068708)* | 🟢 Direct Win32 | ✅ Yes |
| 5 | `WPFTweaksLocation` | Disable Location Tracking | Essential Tweaks | Toggle | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location` -> `Value`; Service `lfsvc` (Geolocation) | `"Deny"` (SZ); `lfsvc` = Disabled | `"Allow"` (SZ); `lfsvc` = Manual | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 6 | `WPFTweaksServices` | Services - Set to Manual | Essential Tweaks | Toggle | Configures ~25 non-essential background services (`MapsBroker`, `NetTcpPortSharing`, `RemoteRegistry`, `shpamsvc`, `WerSvc`, `wisvc`, `WMPNetworkSvc`, `Fax`, etc.) + `SvcHostSplitThresholdInKB` | Demand (`3`) or Disabled (`4`); Split = RAM KB | OriginalType per service; `3670016` (DWORD) | ✅ Compatible *(skips non-existent services cleanly)* | 🟢 Direct Win32 | ✅ Yes |
| 7 | `WPFTweaksDisableExplorerAutoDiscovery` | Disable Explorer Auto Folder Discovery | Essential Tweaks | Toggle | `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags\AllFolders\Shell` -> `FolderType` | `"NotSpecified"` (SZ) | DeleteKey | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 8 | `WPFTweaksWifi` | Disable Wi-Fi Sense | Essential Tweaks | Toggle | `HKLM\SOFTWARE\Microsoft\PolicyManager\default\WiFi\AllowWiFiHotSpotReporting` -> `Value`; `AllowAutoConnectToWiFiSenseHotspots` | `0` (DWORD) | `1` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 9 | `WPFTweaksEndTaskOnTaskbar` | Enable End Task With Right Click | Essential Tweaks | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings` -> `TaskbarEndTask` | `1` (DWORD) | `0` (DWORD) | ❌ Win11 only | 🟢 Direct Win32 | ✅ Yes |
| 10 | `WPFTweaksPowershell7Tele` | Disable PowerShell 7 Telemetry | Essential Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment` -> `POWERSHELL_TELEMETRY_OPTOUT` | `"1"` (SZ) | DeleteValue | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 11 | `WPFTweaksStorage` | Disable Storage Sense | Essential Tweaks | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\StorageSense\Parameters\StoragePolicy` -> `01` | `0` (DWORD) | `1` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 12 | `WPFTweaksHiber` | Disable Hibernation | Essential Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\Power` -> `HibernateEnabled`; runs `powercfg.exe /hibernate off` | `0` (DWORD) | `1` (DWORD); `powercfg /hibernate on` | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 13 | `WPFTweaksDiskCleanup` | Run Disk Cleanup | Essential Tweaks | Toggle | Native recursive file deletion in `%TEMP%`, `C:\Windows\Temp`, `C:\Windows\Prefetch` + `cleanmgr.exe /sagerun:1` | Execute cleanup | N/A | ✅ Compatible | 🟢 Direct Win32 | ❌ No |
| 14 | `WPFTweaksDeleteTempFiles` | Delete Temporary Files | Essential Tweaks | Toggle | Native `FindFirstFileExW` + `DeleteFileW` across `%TEMP%`, `C:\Windows\Temp`, `SoftwareDistribution\Download` | Execute cleanup | N/A | ✅ Compatible | 🟢 Direct Win32 | ❌ No |
| 15 | `WPFTweaksDVR` | Disable GameDVR | Essential Tweaks | Toggle | `HKCU\System\GameConfigStore` -> `GameDVR_FSEBehavior`, `GameDVR_Enabled`; `HKLM\SOFTWARE\Policies\Microsoft\Windows\GameDVR` -> `AllowGameDVR` | `2` / `0` (DWORD) | `1` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 16 | `WPFTweaksBraveDebloat` | Brave Debloat | Essential Tweaks | Toggle | `HKLM\SOFTWARE\Policies\BraveSoftware\Brave` -> `BraveRewardsDisabled`, `BraveWalletDisabled`, `BraveVPNDisabled`, `BraveAIChatEnabled` | `1`, `1`, `1`, `0` (DWORD) | DeleteValue | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 17 | `WPFTweaksEdgeDebloat` | Edge Debloat | Essential Tweaks | Toggle | `HKLM\SOFTWARE\Policies\Microsoft\Edge` -> `HubsSidebarEnabled`, `ShoppingAssistantEnabled`, `UserFeedbackAllowed`, `BackgroundModeEnabled` | `0` (DWORD) | DeleteValue | ✅ Compatible *(Edge v109 on Win7)* | 🟢 Direct Win32 | ✅ Yes |
| 18 | `WPFTweaksHomegroup` | Disable HomeGroup Services | Essential Tweaks | Toggle | Services `HomeGroupListener`, `HomeGroupProvider` via `ChangeServiceConfigW` | Disabled (`4`) | Manual (`3`) | ✅ Compatible *(Primary on Win7/8.1)* | 🟢 Direct Win32 | ✅ Yes |
| 19 | `WPFTweaksDeBloat` | Remove ALL MS Store Apps (Caution) | Essential Tweaks | Toggle | Enumerates non-whitelisted UWP packages via AppX script | `Remove-AppxPackage` | N/A | ⚠️ Win10+ | 🟡 Hybrid PS | ❌ No |
| 20 | `WPFTweaksWPBT` | Disable WPBT Execution | Advanced Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager` -> `DisableWpbtExecution` | `1` (DWORD) | `0` / DeleteValue | ✅ Compatible *(Harmless on Win7)* | 🟢 Direct Win32 | ✅ Yes |
| 21 | `WPFTweaksWindowsAI` | Disable Windows AI (Copilot/Recall) | Advanced Tweaks | Toggle | `HKCU\Software\Policies\Microsoft\Windows\WindowsCopilot` -> `TurnOffWindowsCopilot`; `HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsAI` -> `DisableAIDataAnalysis`, `TurnOffSavingSnapshots` | `1` (DWORD) | DeleteValue | ❌ Win11 only | 🟡 Hybrid PS / DISM | ✅ Yes |
| 22 | `WPFTweaksRightClickMenu` | Classic Right-Click Menu | Advanced Tweaks | Toggle | `HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32` -> `(Default)` | `""` (Empty SZ) | DeleteKeyTree | ❌ Win11 only | 🟢 Direct Win32 | ✅ Yes |
| 23 | `WPFTweaksRemoveHome` | Remove Home from Explorer | Advanced Tweaks | Toggle | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\{f874310e-b6b7-47dc-bc84-b9e6b38f5903}` | DeleteKey / `1` | CreateKey / `0` | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 24 | `WPFTweaksRemoveGallery` | Remove Gallery from Explorer | Advanced Tweaks | Toggle | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}` | DeleteKey | CreateKey | ❌ Win11 only | 🟢 Direct Win32 | ✅ Yes |
| 25 | `WPFTweaksRemoveOnedrive` | Remove OneDrive | Advanced Tweaks | Toggle | Executes `OneDriveSetup.exe /uninstall` + sets `HKLM\SOFTWARE\Policies\Microsoft\Windows\OneDrive` -> `DisableFileSyncNGSC` | `1` (DWORD) + Uninstall | `0` (DWORD) + Reinstall | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 26 | `WPFTweaksRemoveEdge` | Remove Microsoft Edge | Advanced Tweaks | Toggle | `HKLM\SOFTWARE\Microsoft\EdgeUpdate` -> `DoNotUpdateToEdgeWithChromium` + runs `setup.exe --uninstall --system-level --force-uninstall` | `1` (DWORD) + Uninstall | `0` + Reinstall | ✅ Compatible | 🟡 Hybrid PS / Setup | ✅ Yes |
| 27 | `WPFTweaksRemoveCopilot` | Remove Copilot | Advanced Tweaks | Toggle | `HKCU` & `HKLM` `Policies\Microsoft\Windows\WindowsCopilot` -> `TurnOffWindowsCopilot`; `Remove-AppxPackage *Copilot*` | `1` (DWORD) | DeleteValue | ⚠️ Win10+ | 🟡 Hybrid PS | ✅ Yes |
| 28 | `WPFTweaksDisableFSO` | Disable Fullscreen Optimizations | Advanced Tweaks | Toggle | `HKCU\System\GameConfigStore` -> `GameDVR_DXGIHonorFSEWindowsCompatible`, `GameDVR_FSEBehaviorMode` | `1`, `2` (DWORD) | `0`, `0` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 29 | `WPFTweaksDisableLMS1` | Disable Intel LMS Service | Advanced Tweaks | Toggle | Service `LMS` via `ChangeServiceConfigW` | Disabled (`4`) | Automatic (`2`) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 30 | `WPFTweaksIPv46` | Prefer IPv4 over IPv6 | Advanced Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters` -> `DisabledComponents` | `32` (`0x20` DWORD) | `0` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 31 | `WPFTweaksTeredo` | Disable Teredo Tunneling | Advanced Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters` -> `DisabledComponents` or `netsh int teredo set state disabled` | `1` (DWORD) | `0` / default | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 32 | `WPFTweaksDisableIPv6` | Disable IPv6 Completely | Advanced Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters` -> `DisabledComponents` | `255` (`0xFF` DWORD) | `0` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 33 | `WPFTweaksBlockAdobeNet` | Block Adobe Network in Hosts | Advanced Tweaks | Toggle | Appends Adobe telemetry hosts blocklist to `%SystemRoot%\System32\drivers\etc\hosts` | Append hosts block | Restore hosts backup | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 34 | `WPFTweaksDebloatAdobe` | Disable Adobe Background Services | Advanced Tweaks | Toggle | Services `AdobeARMservice`, `agsservice`, `AGMService`, `CCXProcess` via `ChangeServiceConfigW` | Disabled (`4`) | Automatic / Manual | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 35 | `WPFTweaksDisplay` | Set Visual Effects for Performance | Advanced Tweaks | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects` -> `VisualFXSetting`; `HKCU\Control Panel\Desktop` -> `MenuShowDelay` | `2` (DWORD), `"0"` | `0` (DWORD), `"400"` | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 36 | `WPFTweaksUTC` | Set RTC Time to UTC (Dual Boot) | Advanced Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\TimeZoneInformation` -> `RealTimeIsUniversal` | `1` (DWORD) | `0` / DeleteValue | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 37 | `WPFTweaksDisableBGApps` | Disable Background Apps | Advanced Tweaks | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications` -> `GlobalUserDisabled` | `1` (DWORD) | `0` / DeleteValue | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 38 | `WPFTweaksDisableStoreSearch` | Disable Store Search in Start | Advanced Tweaks | Toggle | `HKLM\SOFTWARE\Policies\Microsoft\Windows\Explorer` -> `NoUseStoreOpenWith` | `1` (DWORD) | `0` / DeleteValue | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 39 | `WPFTweaksUnsignedRDP` | Allow Unsigned RDP Connections | Advanced Tweaks | Toggle | `HKCU\Software\Microsoft\Terminal Server Client` -> `AuthenticationLevelOverride` | `0` (DWORD) | DeleteValue | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 40 | `WPFTweaksDeviceMetadata` | Prevent Device Metadata Auto-Download | Advanced Tweaks | Toggle | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Device Metadata` -> `PreventDeviceMetadataFromNetwork` | `1` (DWORD) | `0` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 41 | `WPFTweaksDisableNotifications` | Disable Notification Center | Advanced Tweaks | Toggle | `HKCU\Software\Policies\Microsoft\Windows\Explorer` -> `DisableNotificationCenter`; `HKCU\...\PushNotifications` -> `ToastEnabled` | `1`, `0` (DWORD) | `0`, `1` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 42 | `WPFTweaksDisableCStates` | Disable CPU Core Parking (C-States) | Advanced Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerSettings\54533251-82be-4824-96c1-47b60b740d00\0cc5b647-c1df-4637-891a-dec35c318583` -> `ValueMax` | `0` (DWORD) | `100` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 43 | `WPFTweaksIRPStackSize` | Increase LanmanServer IRPStackSize | Advanced Tweaks | Toggle | `HKLM\SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters` -> `IRPStackSize` | `30` (DWORD) | `15` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 44 | `WPFTweaksHoverTime` | Reduce Mouse Hover Delay | Advanced Tweaks | Toggle | `HKCU\Control Panel\Mouse` -> `MouseHoverTime` | `"400"` (SZ) | Default | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 45 | `WPFTweaksOO` | Run O&O ShutUp10++ | Advanced Tweaks | Button | Downloads `OOSU10.exe` via WinHTTP and launches process | Execute `OOSU10.exe` | N/A | ⚠️ Win10+ | 🟢 Direct Win32 | ❌ No |
| 46 | `WPFToggleDarkMode` | Enable Windows Dark Theme | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` -> `AppsUseLightTheme`, `SystemUsesLightTheme` | `0` (DWORD) | `1` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 47 | `WPFToggleBingSearch` | Bing Search in Start Menu | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Search` -> `BingSearchEnabled` | `1` (DWORD) | `0` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 48 | `WPFToggleNumLock` | NumLock on Startup | Preferences | Toggle | `HKU\.DEFAULT\Control Panel\Keyboard` -> `InitialKeyboardIndicators` | `"2"` (SZ) | `"0"` (SZ) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 49 | `WPFToggleVerboseLogon` | Verbose Status Messages on Logon | Preferences | Toggle | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System` -> `VerboseStatus` | `1` (DWORD) | `0` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 50 | `WPFToggleShowExt` | Show Known File Extensions | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced` -> `HideFileExt` | `0` (DWORD) | `1` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 51 | `WPFToggleHiddenFiles` | Show Hidden Files in Explorer | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced` -> `Hidden` | `1` (DWORD) | `2` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 52 | `WPFToggleMouseAccel` | Mouse Acceleration Thresholds | Preferences | Toggle | `HKCU\Control Panel\Mouse` -> `MouseSpeed`, `MouseThreshold1`, `MouseThreshold2` | `"1"`, `"6"`, `"10"` | `"0"`, `"0"`, `"0"` | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 53 | `WPFToggleStickyKeys` | Disable Sticky Keys Shortcut | Preferences | Toggle | `HKCU\Control Panel\Accessibility\StickyKeys` -> `Flags` | `"510"` (SZ) | `"506"` (SZ) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 54 | `WPFToggleTaskbarWidgets` | Show Taskbar Widgets Button | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced` -> `TaskbarDa` | `1` (DWORD) | `0` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 55 | `WPFToggleTaskbarSearch` | Show Taskbar Search Button | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Search` -> `SearchboxTaskbarMode` | `1` / `2` (DWORD) | `0` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 56 | `WPFToggleTaskView` | Show Task View Button | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced` -> `ShowTaskViewButton` | `1` (DWORD) | `0` (DWORD) | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 57 | `WPFToggleTaskbarAlignment` | Center Taskbar Icons | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced` -> `TaskbarAl` | `1` (Center) | `0` (Left) | ❌ Win11 only | 🟢 Direct Win32 | ✅ Yes |
| 58 | `WPFToggleDetailedBSoD` | Show Detailed BSoD Parameters | Preferences | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\CrashControl` -> `DisplayParameters` | `1` (DWORD) | `0` (DWORD) | ✅ Compatible | 🟢 Direct Win32 | ✅ Yes |
| 59 | `WPFToggleS3Sleep` | Force S3 Sleep (Disable S0 Standby) | Preferences | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\Power` -> `PlatformAoAcOverride` | `0` (DWORD) | DeleteValue | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 60 | `WPFToggleStandbyFix` | Fix Modern Standby Drain | Preferences | Toggle | `HKLM\SYSTEM\CurrentControlSet\Control\Power` -> `CsEnabled`, `PlatformAoAcOverride` | `0` (DWORD) | `1` / DeleteValue | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 61 | `WPFToggleScrollbars` | Always Show Full Scrollbars | Preferences | Toggle | `HKCU\Control Panel\Accessibility` -> `DynamicScrollbars` | `0` (DWORD) | `1` (DWORD) | ❌ Win11 only | 🟢 Direct Win32 | ✅ Yes |
| 62 | `WPFToggleBatteryPercentage` | Show Battery Percentage in Tray | Preferences | Toggle | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced` -> `ShowBatteryPercentage` | `1` (DWORD) | `0` (DWORD) | ❌ Win11 only | 🟢 Direct Win32 | ✅ Yes |
| 63 | `WPFToggleWindowSnapping` | Enable Aero Snap Window Snapping | Preferences | Toggle | `HKCU\Control Panel\Desktop` -> `WindowArrangementActive` | `"1"` (SZ) | `"0"` (SZ) | ✅ Compatible *(Introduced in Win7!)* | 🟢 Direct Win32 | ✅ Yes |
| 64 | `WPFToggleMultiplaneOverlay` | Disable Multiplane Overlay (MPO) | Preferences | Toggle | `HKLM\SOFTWARE\Microsoft\Windows\Dwm` -> `OverlayTestMode` | `5` (DWORD) | DeleteValue | ⚠️ Win10+ | 🟢 Direct Win32 | ✅ Yes |
| 65 | `WPFTweaksAddUltPerf` | Enable Ultimate Performance Scheme | Performance | Button | Runs `powercfg -duplicatescheme e9a42b02-d5df-448d-aa00-03f14749eb61` + `-setactive` *(High Perf `8c5e7fda-...` on Win7)* | Activate Scheme | N/A | ✅ Compatible *(with fallback)* | 🟢 Direct Win32 | ❌ No |
| 66 | `WPFTweaksRemoveUltPerf` | Reset Power Scheme to Balanced | Performance | Button | Runs `powercfg -setactive 381b4222-f694-41f0-9685-ff5bb260df2e` | Activate Balanced | N/A | ✅ Compatible | 🟢 Direct Win32 | ❌ No |
