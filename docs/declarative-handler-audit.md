# Declarative registry, service, combo and DNS handler audit

Reviewed 2026-10-28 by tracing frozen source handlers. No registry, service, adapter, or filesystem state was changed. This describes upstream behavior only; it is not the rustweaker mutation contract.

## Ordinary registry and service declarations

`functions/private/Invoke-WinUtilTweaks.ps1` chooses `Value`/`StartupType`/`InvokeScript` for apply and `OriginalValue`/`OriginalType`/`UndoScript` for undo. These are static catalog values. The ordinary registry loop skips records with a `Values` map. The generic service setter does not record the observed starting type.

`functions/private/Set-WinUtilRegistry.ps1` creates a missing key path, then writes a value or removes it when passed `<RemoveEntry>`. It catches and logs errors instead of propagating a structured failure result to its caller. Consequently the dispatcher may proceed and log a completed tweak after a failed registry operation. Fixed originals are not an observed-state backup.

`functions/private/Set-WinUtilService.ps1` queries a service, skips a service already at the requested startup type, and otherwise uses `Set-Service` or `sc.exe config` for delayed auto-start under older PowerShell. It catches and logs failures. The dispatcher’s default apply guard compares the observed service start type to catalog `OriginalType` and skips the write if they differ; undo bypasses that guard and passes fixed `OriginalType`. This is a limited conflict-avoidance convention, not a durable pre-state snapshot. The earlier not-found path in the dispatcher warns but leaves the setter path enabled; the setter then queries again and logs its own error.

## Registry state combos

The only source tweak with registry `Values` maps is `WPFMultiplaneOverlay`; its `ComboItems` are `Enabled`, `Disabled (Compatibility)`, and `Fully Disabled`. Ordinary tweak dispatch skips those registry records. The UI instead reads the current values with `Get-WinUtilRegistryComboValue`, infers a state through `Get-WinUtilRegistryComboState`, and calls `Set-WinUtilRegistryComboState` when the selection changes.

The combo setter has a useful local failure path: before applying, it records each value's existence and data in memory; after writing it re-reads and verifies; on failure it tries to restore the captured existence/data and verifies again. Important limits:

- The snapshot is process memory only, not a durable journal. A crash cannot resume it.
- It captures value existence/data, not the native registry value type. Restore calls the generic writer using the configured type.
- Verification compares stringified data/existence, not exact native type/data bytes.
- `<RemoveEntry>` is treated as the configured effective default when detecting a UI state, so an absent value and an explicitly stored default can map to the same state.
- The generic registry helper catches write errors; the combo setter compensates by re-reading, but this still is not a persisted transaction.

This path is distinct from the ordinary toggle loop and does not validate the claim that every draft row with “undo yes” is reversible.

## DNS selection

`WPFchangedns` uses `ComboItems` but is not a registry-values combo. `Invoke-WPFtweaksbutton.ps1` reads its selection and calls `Set-WinUtilDNS` before looping over selected tweaks. `Default` is a no-op; other choices enumerate all adapters whose status is `Up`. `Set-WinUtilDNS.ps1` changes IPv4/IPv6 DNS using `Set-DnsClientServerAddress` and `netsh`, and may add/remove DNS-over-HTTPS registrations and per-interface registry data. The DNS provider definitions are external catalog data, separate from tweak entries.

The setter catches an error and returns false but does not restore already processed adapters or DoH state. The UI stops applying the selected tweaks when DNS returns false, but any DNS partial changes remain. No observed previous DNS configuration is journaled by this upstream path. Do not port it as a transaction or infer Win7 support from its UI presence.

## rustweaker implications

- Keep registry/service writes behind the persisted transaction and recovery sessions; no generic helper should swallow a write failure as success.
- Model combo changes using durable exact typed snapshots and recovery, or keep them unavailable.
- Treat DNS as multi-resource mutation across adapters, registry and DoH configuration; require a full backup/recovery design before exposing it.
- Preserve the zero-WMI/CIM and Win7 constraints independently. Handler presence is not proof of supported OS/API availability.
