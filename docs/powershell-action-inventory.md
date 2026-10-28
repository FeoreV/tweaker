# PowerShell action inventory (frozen source comparison)

This document inventories the checked-out frozen reference at `winutil-26.08.19/`. It records identifiers and source entrypoints; it is not a claim that the Rust application implements or authorizes those operations. The upstream directory is read-only.

## Method and scope

- Public handler names were read from top-level `function` declarations under `winutil-26.08.19/functions/public/`; dispatcher routing was inspected in `functions/public/Invoke-WPFButton.ps1`.
- IDs below were read in source order from `config/feature.json`, `tweaks.json`, `appx.json`, `applications.json`, and `preset.json`. `feature.json` and `tweaks.json` contain literal line breaks inside quoted strings, so ID extraction used a parser that escaped those line breaks before JSON decoding; values/behavior were not normalized or inferred from the IDs.
- Paths in this document are repository-relative. “Rust evidence” means the named source currently exists; observer and data-only capabilities are distinguished from action execution.

## Public PowerShell entrypoints

Found 33 public `.ps1` files. Names are based on the actual top-level function declarations; use each linked path for its implementation and parameters.

- `Initialize-WPFUI` — [winutil-26.08.19/functions/public/Initialize-WPFUI.ps1](winutil-26.08.19/functions/public/Initialize-WPFUI.ps1)
- `Invoke-WinUtilAutoRun` — [winutil-26.08.19/functions/public/Invoke-WinUtilAutoRun.ps1](winutil-26.08.19/functions/public/Invoke-WinUtilAutoRun.ps1)
- `Invoke-WPFAppxInstall` — [winutil-26.08.19/functions/public/Invoke-WPFAppxInstall.ps1](winutil-26.08.19/functions/public/Invoke-WPFAppxInstall.ps1)
- `Invoke-WPFAppxRemoval` — [winutil-26.08.19/functions/public/Invoke-WPFAppxRemoval.ps1](winutil-26.08.19/functions/public/Invoke-WPFAppxRemoval.ps1)
- `Invoke-WPFButton` — [winutil-26.08.19/functions/public/Invoke-WPFButton.ps1](winutil-26.08.19/functions/public/Invoke-WPFButton.ps1)
- `Invoke-WPFFeatureInstall` — [winutil-26.08.19/functions/public/Invoke-WPFFeatureInstall.ps1](winutil-26.08.19/functions/public/Invoke-WPFFeatureInstall.ps1)
- `Invoke-WPFFixesNetwork` — [winutil-26.08.19/functions/public/Invoke-WPFFixesNetwork.ps1](winutil-26.08.19/functions/public/Invoke-WPFFixesNetwork.ps1)
- `Invoke-WPFFixesNTPPool` — [winutil-26.08.19/functions/public/Invoke-WPFFixesNTPPool.ps1](winutil-26.08.19/functions/public/Invoke-WPFFixesNTPPool.ps1)
- `Invoke-WPFFixesUpdate` — [winutil-26.08.19/functions/public/Invoke-WPFFixesUpdate.ps1](winutil-26.08.19/functions/public/Invoke-WPFFixesUpdate.ps1)
- `Invoke-WPFFixesWinget` — [winutil-26.08.19/functions/public/Invoke-WPFFixesWinget.ps1](winutil-26.08.19/functions/public/Invoke-WPFFixesWinget.ps1)
- `Invoke-WPFGetInstalled` — [winutil-26.08.19/functions/public/Invoke-WPFGetInstalled.ps1](winutil-26.08.19/functions/public/Invoke-WPFGetInstalled.ps1)
- `Invoke-WPFImpex` — [winutil-26.08.19/functions/public/Invoke-WPFImpex.ps1](winutil-26.08.19/functions/public/Invoke-WPFImpex.ps1)
- `Invoke-WPFInstall` — [winutil-26.08.19/functions/public/Invoke-WPFInstall.ps1](winutil-26.08.19/functions/public/Invoke-WPFInstall.ps1)
- `Invoke-WPFInstallUpgrade` — [winutil-26.08.19/functions/public/Invoke-WPFInstallUpgrade.ps1](winutil-26.08.19/functions/public/Invoke-WPFInstallUpgrade.ps1)
- `Invoke-WPFOOSU` — [winutil-26.08.19/functions/public/Invoke-WPFOOSU.ps1](winutil-26.08.19/functions/public/Invoke-WPFOOSU.ps1)
- `Invoke-WPFPanelAutologin` — [winutil-26.08.19/functions/public/Invoke-WPFPanelAutologin.ps1](winutil-26.08.19/functions/public/Invoke-WPFPanelAutologin.ps1)
- `Invoke-WPFPopup` — [winutil-26.08.19/functions/public/Invoke-WPFPopup.ps1](winutil-26.08.19/functions/public/Invoke-WPFPopup.ps1)
- `Invoke-WPFPresets` — [winutil-26.08.19/functions/public/Invoke-WPFPresets.ps1](winutil-26.08.19/functions/public/Invoke-WPFPresets.ps1)
- `Invoke-WPFRunspace` — [winutil-26.08.19/functions/public/Invoke-WPFRunspace.ps1](winutil-26.08.19/functions/public/Invoke-WPFRunspace.ps1)
- `Invoke-WPFSelectedCheckboxesUpdate` — [winutil-26.08.19/functions/public/Invoke-WPFSelectedCheckboxesUpdate.ps1](winutil-26.08.19/functions/public/Invoke-WPFSelectedCheckboxesUpdate.ps1)
- `Invoke-WPFSSHServer` — [winutil-26.08.19/functions/public/Invoke-WPFSSHServer.ps1](winutil-26.08.19/functions/public/Invoke-WPFSSHServer.ps1)
- `Invoke-WPFSystemRepair` — [winutil-26.08.19/functions/public/Invoke-WPFSystemRepair.ps1](winutil-26.08.19/functions/public/Invoke-WPFSystemRepair.ps1)
- `Invoke-WPFTab` — [winutil-26.08.19/functions/public/Invoke-WPFTab.ps1](winutil-26.08.19/functions/public/Invoke-WPFTab.ps1)
- `Invoke-WPFToggleAllCategories` — [winutil-26.08.19/functions/public/Invoke-WPFToggleAllCategories.ps1](winutil-26.08.19/functions/public/Invoke-WPFToggleAllCategories.ps1)
- `Invoke-WPFtweaksbutton` — [winutil-26.08.19/functions/public/Invoke-WPFtweaksbutton.ps1](winutil-26.08.19/functions/public/Invoke-WPFtweaksbutton.ps1)
- `Invoke-WPFUIElements` — [winutil-26.08.19/functions/public/Invoke-WPFUIElements.ps1](winutil-26.08.19/functions/public/Invoke-WPFUIElements.ps1)
- `Invoke-WPFUIThread` — [winutil-26.08.19/functions/public/Invoke-WPFUIThread.ps1](winutil-26.08.19/functions/public/Invoke-WPFUIThread.ps1)
- `Invoke-WPFUltimatePerformance` — [winutil-26.08.19/functions/public/Invoke-WPFUltimatePerformance.ps1](winutil-26.08.19/functions/public/Invoke-WPFUltimatePerformance.ps1)
- `Invoke-WPFundoall` — [winutil-26.08.19/functions/public/Invoke-WPFundoall.ps1](winutil-26.08.19/functions/public/Invoke-WPFundoall.ps1)
- `Invoke-WPFUnInstall` — [winutil-26.08.19/functions/public/Invoke-WPFUnInstall.ps1](winutil-26.08.19/functions/public/Invoke-WPFUnInstall.ps1)
- `Invoke-WPFUpdatesdefault` — [winutil-26.08.19/functions/public/Invoke-WPFUpdatesdefault.ps1](winutil-26.08.19/functions/public/Invoke-WPFUpdatesdefault.ps1)
- `Invoke-WPFUpdatesdisable` — [winutil-26.08.19/functions/public/Invoke-WPFUpdatesdisable.ps1](winutil-26.08.19/functions/public/Invoke-WPFUpdatesdisable.ps1)
- `Invoke-WPFUpdatessecurity` — [winutil-26.08.19/functions/public/Invoke-WPFUpdatessecurity.ps1](winutil-26.08.19/functions/public/Invoke-WPFUpdatessecurity.ps1)

## Configured feature/action identifiers

`config/feature.json` contains 33 top-level identifiers. `Invoke-WPFButton` checks this map first: entries with `function` invoke that named function when available; entries with non-empty `InvokeScript` run the configured script blocks. Other entries are controls/selections consumed by feature UI code; this inventory does not assign them unverified execution semantics.

- `WPFFeaturesdotnet` — .NET Framework (Versions 2, 3, 4) - Enable; config entry (no function/InvokeScript found)
- `WPFFixesNTPPool` — NTP Server - Enable; function `Invoke-WPFFixesNTPPool`
- `WPFFeatureshyperv` — Hyper-V - Enable; config entry (no function/InvokeScript found)
- `WPFFeatureslegacymedia` — Legacy Media Components (WMP, DirectPlay) - Enable; config entry (no function/InvokeScript found)
- `WPFFeaturewsl` — Windows Subsystem for Linux (WSL) - Enable; config entry (no function/InvokeScript found)
- `WPFFeaturenfs` — Network File System (NFS) - Enable; 5 configured InvokeScript block(s)
- `WPFFeatureRegBackup` — Registry Backup (Daily Task 12:30am) - Enable; 1 configured InvokeScript block(s)
- `WPFFeatureEnableLegacyRecovery` — Legacy F8 Boot Recovery - Enable; 1 configured InvokeScript block(s)
- `WPFFeatureDisableLegacyRecovery` — Legacy F8 Boot Recovery - Disable; 1 configured InvokeScript block(s)
- `WPFFeaturesSandbox` — Windows Sandbox - Enable; config entry (no function/InvokeScript found)
- `WPFFeatureInstall` — Install Features; function `Invoke-WPFFeatureInstall`
- `WPFPanelAutologin` — AutoLogon - Run; function `Invoke-WPFPanelAutologin`
- `WPFFixesUpdate` — Windows Update - Reset; function `Invoke-WPFFixesUpdate`
- `WPFFixesNetwork` — Network - Reset; function `Invoke-WPFFixesNetwork`
- `WPFPanelDISM` — System Corruption Scan - Run; function `Invoke-WPFSystemRepair`
- `WPFFixesWinget` — WinGet - Reinstall; function `Invoke-WPFFixesWinget`
- `WPFPanelComputer` — Computer Management; 1 configured InvokeScript block(s)
- `WPFPanelControl` — Control Panel; 1 configured InvokeScript block(s)
- `WPFPanelMouse` — Mouse Properties; 1 configured InvokeScript block(s)
- `WPFPanelNetwork` — Network Connections; 1 configured InvokeScript block(s)
- `WPFPanelPower` — Power Panel; 1 configured InvokeScript block(s)
- `WPFPanelPrinter` — Printer Panel; 1 configured InvokeScript block(s)
- `WPFPanelPrograms` — Programs and Features; 1 configured InvokeScript block(s)
- `WPFPanelRegion` — Region; 1 configured InvokeScript block(s)
- `WPFPanelSecurity` — Security and Maintenance; 1 configured InvokeScript block(s)
- `WPFPanelSound` — Sound Settings; 1 configured InvokeScript block(s)
- `WPFPanelSystem` — System Properties; 1 configured InvokeScript block(s)
- `WPFPanelTimedate` — Time and Date; 1 configured InvokeScript block(s)
- `WPFPanelFirewall` — Windows Defender Firewall; 1 configured InvokeScript block(s)
- `WPFPanelRestore` — Windows Restore; 1 configured InvokeScript block(s)
- `WPFWinUtilInstallPSProfile` — CTT PowerShell Profile - Install; function `Invoke-WinUtilInstallPSProfile`
- `WPFWinUtilUninstallPSProfile` — CTT PowerShell Profile - Remove; function `Invoke-WinUtilUninstallPSProfile`
- `WPFWinUtilSSHServer` — OpenSSH Server - Enable; function `Invoke-WPFSSHServer`

## Tweak and toggle identifiers

`config/tweaks.json` contains 66 top-level IDs, including tweak checkboxes, toggle checkboxes, DNS selector, and power buttons. All IDs are listed here; naming alone is not proof of behavior. The central source path is `functions/private/Invoke-WinUtilTweaks.ps1` for tweak execution, with the public apply workflow `functions/public/Invoke-WPFtweaksbutton.ps1` and undo workflow `functions/public/Invoke-WPFundoall.ps1`.

`WPFTweaksActivity`, `WPFTweaksHiber`, `WPFTweaksWidget`, `WPFTweaksRevertStartMenu`, `WPFTweaksDisableStoreSearch`, `WPFTweaksLocation`, `WPFTweaksServices`, `WPFTweaksBraveDebloat`, `WPFTweaksDisableWarningForUnsignedRdp`, `WPFTweaksEdgeDebloat`, `WPFTweaksConsumerFeatures`, `WPFTweaksTelemetry`, `WPFTweaksDeliveryOptimization`, `WPFTweaksRemoveEdge`, `WPFTweaksDisableBitLocker`, `WPFTweaksUTC`, `WPFTweaksRemoveOneDrive`, `WPFTweaksRemoveHomeAndGallery`, `WPFTweaksDisplay`, `WPFTweaksReservedStorage`, `WPFTweaksRestorePoint`, `WPFTweaksEndTaskOnTaskbar`, `WPFTweaksStorage`, `WPFTweaksWindowsAI`, `WPFTweaksWPBT`, `WPFTweaksPreventDeviceMetadataFromNetwork`, `WPFTweaksRazerBlock`, `WPFTweaksDisableNotifications`, `WPFTweaksBlockAdobeNet`, `WPFTweaksRightClickMenu`, `WPFTweaksDiskCleanup`, `WPFTweaksDeleteTempFiles`, `WPFTweaksIPv46`, `WPFTweaksTeredo`, `WPFTweaksDisableIPv6`, `WPFTweaksDisableBGapps`, `WPFTweaksDisableExplorerAutoDiscovery`, `WPFToggleDetailedBSoD`, `WPFToggleBatteryPercentage`, `WPFToggleDarkMode`, `WPFToggleShowExt`, `WPFToggleHiddenFiles`, `WPFToggleVerboseLogon`, `WPFToggleNewOutlook`, `WPFToggleScrollbars`, `WPFMultiplaneOverlay`, `WPFToggleMouseAcceleration`, `WPFToggleNumLock`, `WPFToggleWindowSnapping`, `WPFToggleStandbyFix`, `WPFToggleS3Sleep`, `WPFToggleHideSettingsHome`, `WPFToggleBingSearch`, `WPFToggleLoginBlur`, `WPFTweaksDisableLockscreen`, `WPFToggleStartMenuRecommendations`, `WPFToggleStickyKeys`, `WPFToggleTaskbarAlignment`, `WPFToggleTaskbarSearch`, `WPFToggleTaskView`, `WPFToggleGameMode`, `WPFToggleLongPaths`, `WPFOOSUbutton`, `WPFchangedns`, `WPFAddUltPerf`, `WPFRemoveUltPerf`

The separate config IDs `WPFchangedns`, `WPFAddUltPerf`, and `WPFRemoveUltPerf` are present in the same list; dispatcher mappings for the two power IDs are in `functions/public/Invoke-WPFButton.ps1`. DNS selection is used by `Invoke-WPFtweaksbutton.ps1` via private `Set-WinUtilDNS.ps1`.

## AppX identifiers

`config/appx.json` contains 33 package-selection IDs:

`WPFAppxMicrosoft_WindowsFeedbackHub`, `WPFAppxMicrosoft_GetHelp`, `WPFAppxMicrosoft_OutlookForWindows`, `WPFAppxMSTeams`, `WPFAppxClipchamp_Clipchamp`, `WPFAppxMicrosoft_MicrosoftOfficeHub`, `WPFAppxMicrosoft_ZuneMusic`, `WPFAppxMicrosoft_BingSearch`, `WPFAppxMicrosoftCorporationII_QuickAssist`, `WPFAppxMicrosoft_WindowsDevHome`, `WPFAppxMicrosoft_WindowsCrossDevice`, `WPFAppxMicrosoft_Todos`, `WPFAppxMicrosoft_PowerAutomateDesktop`, `WPFAppxMicrosoft_YourPhone`, `WPFAppxMicrosoft_MicrosoftStickyNotes`, `WPFAppxMicrosoft_WindowsSoundRecorder`, `WPFAppxMicrosoft_WindowsAlarms`, `WPFAppxMicrosoft_Paint`, `WPFAppxMicrosoft_WindowsNotepad`, `WPFAppxMicrosoft_ScreenSketch`, `WPFAppxMicrosoft_Copilot`, `WPFAppxMicrosoft_WindowsCalculator`, `WPFAppxMicrosoft_WindowsCamera`, `WPFAppxMicrosoft_WindowsPhotos`, `WPFAppxMicrosoft_BingNews`, `WPFAppxMicrosoft_BingWeather`, `WPFAppxMicrosoft_GamingApp`, `WPFAppxMicrosoft_XboxGamingOverlay`, `WPFAppxMicrosoft_XboxIdentityProvider`, `WPFAppxMicrosoft_XboxSpeechToTextOverlay`, `WPFAppxMicrosoft_Xbox_TCUI`, `WPFAppxMicrosoft_StartExperiencesApp`, `WPFAppxMicrosoft_MicrosoftSolitaireCollection`

Source handler references: public `functions/public/Invoke-WPFAppxInstall.ps1` and `Invoke-WPFAppxRemoval.ps1`; private package helpers `functions/private/Install-WinUtilAPPX.ps1`, `Remove-WinUtilAPPX.ps1`, and `Remove-WinUtilProvisionedAPPX.ps1`. Dispatcher IDs include `WPFInstallSelectedAppx`, `WPFRemoveSelectedAppx`, `WPFDefaultAppxSelection`, `WPFSelectAllAppx`, `WPFClearAppxSelection`, and `WPFGetInstalledAppx` in `Invoke-WPFButton.ps1`.

### AppX and optional-feature behavior audit — 2026-10-28

- AppX install processes selected IDs on a runspace and calls `Install-WinUtilAPPX` with catalog `PackageId` and `StoreId` ([public handler](../winutil-26.08.19/functions/public/Invoke-WPFAppxInstall.ps1#L29)). The helper searches installed/provisioned package metadata for a local manifest, attempts `Add-AppxPackage -Register` through `powershell.exe`, and may fall back to WinGet's Store source when a Store ID exists ([helper](../winutil-26.08.19/functions/private/Install-WinUtilAPPX.ps1#L23)).
- AppX removal calls the per-user/all-users and provisioned-package helpers; individual package failures are logged and swallowed. The public handler also stops `GameBarFTServer` and writes HKCU `GameDVR\AppCaptureEnabled=0` for Xbox Gaming Overlay, stops `dllhost` for Notepad, and invokes `Get-Package`/`Uninstall-Package` for the Teams meeting add-in ([public handler](../winutil-26.08.19/functions/public/Invoke-WPFAppxRemoval.ps1#L39), [per-user helper](../winutil-26.08.19/functions/private/Remove-WinUtilAPPX.ps1#L21), [provisioned helper](../winutil-26.08.19/functions/private/Remove-WinUtilProvisionedAPPX.ps1#L25)). There is no source-provided exact rollback transaction for these effects.
- The feature handler iterates `$sync.selectedFeatures` and delegates each ID to `Invoke-WinUtilFeatureInstall` ([public handler](../winutil-26.08.19/functions/public/Invoke-WPFFeatureInstall.ps1#L15)). The helper enables declared optional features with `Enable-WindowsOptionalFeature -Online -All -NoRestart`, then executes configured script blocks ([private helper](../winutil-26.08.19/functions/private/Invoke-WinUtilFeatureInstall.ps1#L4)). Among the selectable feature definitions, scripts include NFS registry/command changes, scheduled registry-backup task creation, and `bcdedit` changes ([feature config](../winutil-26.08.19/config/feature.json#L60)). These are heterogeneous operations, not a single reversible feature toggle.
- The inspected AppX and feature handlers contain no explicit Windows-version gate. This audit does not infer supported OS versions from API names or comments. Rust currently recognizes the IDs in data-only selection metadata; `package_import.rs` rejects AppX/features, and no executor or undo/recovery path exists. The handlers therefore remain unavailable in Rust.

## Application catalog identifiers

`config/applications.json` contains 232 top-level application IDs. These are package catalog selections; source-specific install/uninstall handling is reached through public `Invoke-WPFInstall.ps1`, `Invoke-WPFUnInstall.ps1`, and package manager helpers under `functions/private/` (including `Install-WinUtilWinget.ps1` and `Install-WinUtilChoco.ps1`).

`1password`, `7zip`, `adobe`, `advancedip`, `aimp`, `angryipscanner`, `anydesk`, `audacity`, `autoruns`, `rdcman`, `autohotkey`, `battlenet`, `bitwarden`, `blender`, `brave`, `bruno`, `bulkcrapuninstaller`, `blurautoclicker`, `calibre`, `cemu`, `chatgpt`, `chatterino`, `chrome`, `chromium`, `cinebenchr23`, `claude`, `claude-code`, `cmake`, `codex`, `cpuz`, `crystaldiskinfo`, `crystaldiskmark`, `cursor`, `ddu`, `discord`, `dismtools`, `ntlite`, `dorion`, `dockerdesktop`, `dotnet6`, `dotnet8`, `dotnet9`, `dotnet10`, `dropbox`, `eaapp`, `eartrumpet`, `edge`, `es-de`, `enteauth`, `epicgames`, `files`, `firefox`, `firefoxesr`, `floorp`, `flux`, `foobar`, `fnm`, `foxpdfreader`, `geforcenow`, `gimp`, `git`, `gitextensions`, `githubcli`, `githubdesktop`, `gog`, `golang`, `googledrive`, `gpuz`, `gsudo`, `helium`, `hugo`, `handbrake`, `heroiclauncher`, `hwinfo`, `hwmonitor`, `imageglass`, `internetdownloadmanager`, `irfanview`, `itch`, `itunes`, `java8`, `java21`, `java25`, `jellyfinmediaplayer`, `jellyfinserver`, `jetbrains`, `jpegview`, `joplin`, `keepassxc`, `klite`, `kodi`, `lazygit`, `libreoffice`, `librewolf`, `localsend`, `mpc-qt`, `mpv`, `matrix`, `minitoolpartitionwizard`, `modrinth`, `moonlight`, `mpchc`, `msedgeredirect`, `msiafterburner`, `mullvadvpn`, `mullvadbrowser`, `nomacs`, `nanazip`, `netbird`, `tailscale`, `naps2`, `neovim`, `nextclouddesktop`, `nmap`, `nodejs`, `nodejslts`, `pnpm`, `notepadplus`, `nuget`, `nvclean`, `obs`, `obsidian`, `okular`, `onedrive`, `onlyoffice`, `OPAutoClicker`, `openrgb`, `OpenVPN`, `OVirtualBox`, `policyplus`, `processexplorer`, `Paintdotnet`, `parsec`, `peazip`, `pdf-xchange`, `pdf24creator`, `pdfgear`, `pdfsam`, `playnite`, `plex`, `plexdesktop`, `posh`, `postman`, `powershell`, `powertoys`, `prismlauncher`, `processlasso`, `protonauth`, `protonmail`, `protondrive`, `protonpass`, `protonvpn`, `processmonitor`, `putty`, `python3`, `qbittorrent`, `qownnotes`, `qtox`, `revo`, `WiseProgramUninstaller`, `rufus`, `rustlang`, `sdio`, `sharex`, `nilesoftShell`, `systeminformer`, `signal`, `signalrgb`, `simplenote`, `simplewall`, `slack`, `startallback`, `starship`, `steam`, `roblox`, `sublimetext`, `sumatra`, `sunshine`, `tcpview`, `teams`, `teamviewer`, `teamspeak3`, `teamspeak6`, `telegram`, `terminal`, `thunderbird`, `betterbird`, `tor`, `totalcommander`, `treesize`, `ttaskbar`, `ubisoft`, `ungoogled`, `unity`, `vagrant`, `everything`, `vc2015_32`, `vc2015_64`, `ventoy`, `vesktop`, `viber`, `visualstudio2022`, `visualstudio2026`, `vivaldi`, `vlc`, `vrdesktopstreamer`, `vscode`, `vscodium`, `waterfox`, `whatsapp`, `wingetui`, `winrar`, `winscp`, `wireguard`, `wireshark`, `wiztree`, `xeheditor`, `xournal`, `yarn`, `zoom`, `uv`, `tightvnc`, `glazewm`, `Overwolf`, `OFGB`, `ZenBrowser`, `Zed`, `zotero`, `deskflow`, `Ruby`, `Lua`, `CloudflareWARP`

## Preset identifiers and members

`config/preset.json` defines 4 named selection presets. Member IDs are listed in source order exactly as parsed:

### `Standard` (12 IDs)

`WPFTweaksActivity`, `WPFTweaksConsumerFeatures`, `WPFTweaksDisableExplorerAutoDiscovery`, `WPFTweaksWPBT`, `WPFTweaksLocation`, `WPFTweaksServices`, `WPFTweaksTelemetry`, `WPFTweaksDeliveryOptimization`, `WPFTweaksDiskCleanup`, `WPFTweaksDeleteTempFiles`, `WPFTweaksEndTaskOnTaskbar`, `WPFTweaksRestorePoint`

### `Minimal` (4 IDs)

`WPFTweaksConsumerFeatures`, `WPFTweaksWPBT`, `WPFTweaksServices`, `WPFTweaksTelemetry`

### `Advanced` (17 IDs)

`WPFTweaksRestorePoint`, `WPFTweaksActivity`, `WPFTweaksConsumerFeatures`, `WPFTweaksDisableExplorerAutoDiscovery`, `WPFTweaksWPBT`, `WPFTweaksLocation`, `WPFTweaksServices`, `WPFTweaksTelemetry`, `WPFTweaksDeliveryOptimization`, `WPFTweaksDeleteTempFiles`, `WPFTweaksEndTaskOnTaskbar`, `WPFTweaksDisableStoreSearch`, `WPFTweaksRevertStartMenu`, `WPFTweaksWidget`, `WPFTweaksRemoveOneDrive`, `WPFTweaksWindowsAI`, `WPFTweaksRightClickMenu`

### `AppxDefault` (18 IDs)

`WPFAppxMicrosoft_WindowsFeedbackHub`, `WPFAppxMicrosoft_GetHelp`, `WPFAppxMicrosoft_MicrosoftOfficeHub`, `WPFAppxMicrosoft_WindowsCalculator`, `WPFAppxClipchamp_Clipchamp`, `WPFAppxMicrosoft_WindowsAlarms`, `WPFAppxMicrosoftCorporationII_QuickAssist`, `WPFAppxMicrosoft_WindowsSoundRecorder`, `WPFAppxMicrosoft_MicrosoftStickyNotes`, `WPFAppxMicrosoft_Todos`, `WPFAppxMicrosoft_MicrosoftSolitaireCollection`, `WPFAppxMicrosoft_PowerAutomateDesktop`, `WPFAppxMicrosoft_WindowsDevHome`, `WPFAppxMicrosoft_BingWeather`, `WPFAppxMicrosoft_StartExperiencesApp`, `WPFAppxMicrosoft_BingNews`, `WPFAppxMicrosoft_Copilot`, `WPFAppxMicrosoft_BingSearch`

The public preset handler is `functions/public/Invoke-WPFPresets.ps1`; dispatcher buttons and selection filters are in `functions/public/Invoke-WPFButton.ps1`. Preset membership describes selections, not a Rust action implementation.

## Action family cross-reference

| Family | PowerShell source entrypoints / identifiers | Rust capability observed |
|---|---|---|
| Tweak apply/undo | `Invoke-WPFtweaksbutton`, `Invoke-WPFundoall`, private `Invoke-WinUtilTweaks`; tweak IDs above | `src/engine/tweak_plan.rs` defines a generic guarded plan/apply API; `config/catalog.v1.json` is documented in `AGENT_START.md` as a single unverified review fixture, not a production action catalog. Selection IDs in `src/data/selection_catalog.rs` are validation metadata. No general UI/CLI apply/undo route was found in `src/main.rs`. |
| Apps/packages | `Invoke-WPFInstall`, `Invoke-WPFUnInstall`, `Invoke-WPFInstallUpgrade`; app IDs above | `src/engine/package_plan.rs`, `package_import.rs`, and `package_state.rs` support planning/import/matching; CLI exposes data-only plan commands in `src/main.rs`. No package manager execution route found. |
| AppX | `Invoke-WPFAppxInstall`, `Invoke-WPFAppxRemoval`; AppX IDs above | IDs/presets in selection catalog only; no AppX executor found in `src/`. |
| Optional features and feature actions | IDs in `feature.json`, notably `WPFFeatureInstall`; public `Invoke-WPFFeatureInstall` and config scripts | Feature selection IDs are metadata; no optional-feature action executor found in `src/`. |
| Repair, cleanup, update, power, restore, DNS | `Invoke-WPFSystemRepair`, `Invoke-WPFFixesUpdate`, `Invoke-WPFUpdatesdefault/disable/security`, `Invoke-WPFUltimatePerformance`; `WPFTweaksDiskCleanup`, `WPFTweaksRestorePoint`, `WPFchangedns` | No corresponding mutation handler found in `src/`. `src/engine/diagnostics.rs` exposes diagnostics collection; that is not system repair. |
| Network / Winsock | `Invoke-WPFFixesNetwork`; `Set-WinUtilDNS` helper; network and DNS IDs/config | `src/engine/winsock_catalog.rs` plus `--winsock-report` in `src/main.rs` capture/report catalog state. This is read-only and is not a Winsock reset or DNS setter. |
| Installed state | `Invoke-WPFGetInstalled`; `WPFGetInstalled`, `WPFGetInstalledTweaks`, `WPFGetInstalledAppx` dispatcher IDs | `src/engine/installed_apps_windows.rs` inventories uninstall registry records; toggle/status modules and CLI commands are observers. These do not infer complete applied-tweak, package-manager, or AppX state. |

The current CLI source also includes selection import/export, preset preview, application filtering, registry/service reads and diagnostics. See `src/main.rs`, `src/engine/application_filter.rs`, `src/data/selection_import.rs`, `selection_export.rs`, and `selection_state.rs`; these are data/read paths and do not enable the PowerShell mutations listed above.

## Public handlers outside this action inventory

These public entrypoints exist in the checked tree but are not comprehensively inventoried above because they concern UI/runtime orchestration, ISO/USB workflows, or other adjacent functionality. Their presence is confirmed by public-file discovery; behavior is intentionally not summarized here:

- `Initialize-WPFUI` — [winutil-26.08.19/functions/public/Initialize-WPFUI.ps1](winutil-26.08.19/functions/public/Initialize-WPFUI.ps1)
- `Invoke-WinUtilAutoRun` — [winutil-26.08.19/functions/public/Invoke-WinUtilAutoRun.ps1](winutil-26.08.19/functions/public/Invoke-WinUtilAutoRun.ps1)
- `Invoke-WPFImpex` — [winutil-26.08.19/functions/public/Invoke-WPFImpex.ps1](winutil-26.08.19/functions/public/Invoke-WPFImpex.ps1)
- `Invoke-WPFOOSU` — [winutil-26.08.19/functions/public/Invoke-WPFOOSU.ps1](winutil-26.08.19/functions/public/Invoke-WPFOOSU.ps1)
- `Invoke-WPFPanelAutologin` — [winutil-26.08.19/functions/public/Invoke-WPFPanelAutologin.ps1](winutil-26.08.19/functions/public/Invoke-WPFPanelAutologin.ps1)
- `Invoke-WPFPopup` — [winutil-26.08.19/functions/public/Invoke-WPFPopup.ps1](winutil-26.08.19/functions/public/Invoke-WPFPopup.ps1)
- `Invoke-WPFRunspace` — [winutil-26.08.19/functions/public/Invoke-WPFRunspace.ps1](winutil-26.08.19/functions/public/Invoke-WPFRunspace.ps1)
- `Invoke-WPFSelectedCheckboxesUpdate` — [winutil-26.08.19/functions/public/Invoke-WPFSelectedCheckboxesUpdate.ps1](winutil-26.08.19/functions/public/Invoke-WPFSelectedCheckboxesUpdate.ps1)
- `Invoke-WPFSSHServer` — [winutil-26.08.19/functions/public/Invoke-WPFSSHServer.ps1](winutil-26.08.19/functions/public/Invoke-WPFSSHServer.ps1)
- `Invoke-WPFTab` — [winutil-26.08.19/functions/public/Invoke-WPFTab.ps1](winutil-26.08.19/functions/public/Invoke-WPFTab.ps1)
- `Invoke-WPFToggleAllCategories` — [winutil-26.08.19/functions/public/Invoke-WPFToggleAllCategories.ps1](winutil-26.08.19/functions/public/Invoke-WPFToggleAllCategories.ps1)
- `Invoke-WPFUIElements` — [winutil-26.08.19/functions/public/Invoke-WPFUIElements.ps1](winutil-26.08.19/functions/public/Invoke-WPFUIElements.ps1)
- `Invoke-WPFUIThread` — [winutil-26.08.19/functions/public/Invoke-WPFUIThread.ps1](winutil-26.08.19/functions/public/Invoke-WPFUIThread.ps1)

Unknown / not established by this inventory: the complete runtime binding from every UI control to every handler; semantics of individual tweak scripts beyond the handler/config text inspected; support by a Rust execution path; rollback guarantees for upstream PowerShell operations; and unlisted behavior in private helpers. The public handler list above remains the source of truth for the actual public entrypoint inventory.

## Window Snapping operation-plan status

`src/engine/window_snapping_plan.rs` contains a crate-private typed operation description for the audited ID: Apply writes the source `REG_SZ "1"`; source Undo writes configured `REG_SZ "0"`; journal recovery restores the exact captured pre-state instead. Five mock tests exercise the transaction/recovery contract and fail closed for a missing key. This does not implement or expose a Windows mutation action. `src/engine/tweak_plan.rs` rejects HKCU, and the Windows registry writer rejects it until user-profile identity is bound to recovery. The PowerShell helper creates missing paths; Rust intentionally does not, because creation rollback is unsupported.

The guarded mutation path also rejects all `Hive::Users` targets, including the `WPFToggleNumLock` HKU operation. This is deny-all-HKU policy; the read-only HKU observer remains supported. Existing HKU mutation records fail closed during Windows recovery and remain retained pending a reviewed user-bound target/recovery design.

## Per-action behavior audit: `WPFTweaksActivity`

Source entry: [`config/tweaks.json`](../winutil-26.08.19/config/tweaks.json#L2) declares three `HKLM:\SOFTWARE\Policies\Microsoft\Windows\System` values: `EnableActivityFeed`, `PublishUserActivities`, and `UploadUserActivities`. Each is `DWord`, Apply value `0`, and configured Undo `<RemoveEntry>` ([entry details](../winutil-26.08.19/config/tweaks.json#L7), [second value](../winutil-26.08.19/config/tweaks.json#L15), [third value](../winutil-26.08.19/config/tweaks.json#L23)). The dispatcher selects `Value` for Apply or `OriginalValue` for Undo and routes each registry item to `Set-WinUtilRegistry` ([`Invoke-WinUtilTweaks.ps1`](../winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1#L27), [registry loop](../winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1#L64)). That helper creates a missing key, writes the value, or removes it for `<RemoveEntry>`; errors are caught and logged rather than propagated ([`Set-WinUtilRegistry.ps1`](../winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1#L33), [set/remove](../winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1#L39), [error handling](../winutil-26.08.19/functions/private/Set-WinUtilRegistry.ps1#L49)).

`Invoke-WinUtilCurrentSystem` treats a missing key/value as the configured `OriginalValue`, then compares it with `Value`; therefore absent values report not applied ([`Invoke-WinUtilCurrentSystem.ps1`](../winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1#L75), [missing/default handling](../winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1#L84), [comparison](../winutil-26.08.19/functions/private/Invoke-WinUtilCurrentSystem.ps1#L98)). No Activity-specific test or OS/build condition was found. The source does not establish which Windows versions honor these values. Rust must journal exact pre-state and restore it on transaction recovery; configured Undo's deletion behavior is not the recovery state. Rejecting a missing key is a deliberate deviation until key creation/deletion can be backed up and recovered. No Rust action executor is connected to this ID.
