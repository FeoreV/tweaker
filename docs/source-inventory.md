# Source inventory: frozen-source audit

Verified: 2026-10-28 against the checked-in frozen source; source directory remains read-only. SHA-256 anchors for the audited files:

- `config/tweaks.json`: `CEEEAF210309390CE094A16BB3F0467852E915E9F946E7C7C723A9AF1FCBB2F9`
- `config/applications.json`: `46AD39984E5E7D1F38EFFC657467B5F1A63B9BFAD777CBEC870CFD2910B9F30F`
- `LICENSE`: `469B54DADFA5C0D0002291E452FCAA32165356D3459E1DEE040404DB5439FB1C`

Counts are top-level keys, not verified implementations or compatibility claims. The read-only comparison script also reports source/draft ID mismatches:

```powershell
python scripts/audit-source-inventory.py
```

To refresh the deterministic per-entry fact extract:

```powershell
python scripts/audit-source-inventory.py --write-facts
```

This writes [`upstream-tweaks-facts.json`](upstream-tweaks-facts.json): all 66 frozen-source IDs, their source labels, raw registry/service/AppX/ComboItems declarations, other field names, and UTF-8 byte counts/SHA-256 for each InvokeScript/UndoScript. Script bodies are intentionally not copied into the extract. This is provenance data for review, not an execution manifest or an assessment of compatibility, reversibility, licensing, or safety. Regenerate it after the frozen source hash changes and review the diff.

The same command also writes [`inventory-source-comparison.json`](inventory-source-comparison.json), which joins rows only on exact ID. For each union ID it records upstream `Content`/category and declared action counts alongside the inventory's display name, category, operation summary, apply/undo values, and compatibility/undo claims. `source_only` and `draft_only` entries remain separate. This exposes what a draft row claims next to source declarations; it does not parse the prose into verified registry edits or infer aliases.

Selected name-similarity candidates were manually traced through their declarations and handlers in [`id-divergence-review.md`](id-divergence-review.md). It records a few supported feature correspondences, partial mappings, and tempting matches rejected due to different behavior. This is not a full review of every candidate or a compatibility audit.

The exact-ID rows' draft OS and undo claims were separately counted and compared with source metadata in [`inventory-claim-audit.md`](inventory-claim-audit.md). That report distinguishes a catalog declaration from actual OS support and from a pre-mutation rollback snapshot.

The 25 custom-script entries were read without execution and their side effects/undo limits are in [`custom-action-audit.md`](custom-action-audit.md). This includes actions outside the declarative registry/service records, six entries with no `UndoScript`, CIM-based memory discovery, remote downloads, file/package deletion, and scripts whose “undo” repeats deletion or applies a fixed state.

The generic registry/service dispatch, registry-valued combo path, and DNS provider path are traced in [`declarative-handler-audit.md`](declarative-handler-audit.md). In particular, combo rollback is only in-memory and does not retain native value types; DNS applies across all up adapters before other tweaks and has no rollback for partial failure.

Reproduce with Python: `json.loads(path.read_text(encoding="utf-8-sig"), strict=False)` and `len(...)`. The `strict=False` option is necessary for the current tweak file: strict parsing fails at line 67 due to an unescaped newline in an embedded script. It must not be interpreted as permission to accept arbitrary malformed input in user presets. Application catalog parses with the same command; assess strict parsing separately before import.

- 66 tweak entries: 50 contain `registry`, 2 contain `service`, 25 contain `InvokeScript`, 19 contain `UndoScript`; categories overlap. 29 contain `Type`; 2 have `ComboItems`. Presence of a field is not proof of supported or reversible behavior.
- 232 application entries: all have a `winget` field; 230 have a `choco` field. A field's existence does not prove package availability, installation compatibility, or support on Win7.
- `WPFTweaksActivity` (tweaks.json:2) contains three registry entries with original value `<RemoveEntry>`. OriginalValue is a proposed upstream undo, not a snapshot of actual system state.
- `WPFTweaksHiber` (tweaks.json:32) has **two** registry entries and invokes `powercfg.exe /hibernate off`; its undo script invokes `powercfg.exe /hibernate on`. The current inventory's summary mentions one registry entry and labels it direct Win32, so it is incomplete and must not be used as an implementation specification. A command's effects and reversibility require separate investigation.
- The draft has 66 rows but only **42 exact ID matches** with the frozen 66-key catalog. It contains 24 IDs not present in the source and omits 24 source IDs. No rename/alias mapping has been established; do not silently treat similarly named entries as equivalents. The script above prints both complete mismatch sets and confirms there are no duplicate draft IDs.
- Upstream `LICENSE` is MIT, copyright CT Tech Group LLC; preserve its required notice when copying substantial portions. This does not establish licensing for other dependencies or application packages.

The counts do not validate every entry, Windows version, service action, executable script, package ID, or undo. Trace each adopted tweak to the upstream entry and implementation, and test it independently before enabling it. ISO creation remains out of scope.

The application install path is `functions/public/Invoke-WPFInstall.ps1` plus `functions/private/Get-WinUtilSelectedPackages.ps1`. It splits selected IDs by manager preference, skips blank/`na` package IDs, de-duplicates IDs, and under Choco preference falls back to the row's winget ID when its choco ID is unavailable. It then invokes package-manager install helpers. The catalog has no pinned package version/hash or per-package license field; this review did not query package repositories or vendor licenses. Treat package availability, package license and install behavior as unverified per application.

## What the upstream dispatcher actually does (handler-level spot check)

Read-only trace of `functions/private/Invoke-WinUtilTweaks.ps1`, `functions/private/Set-WinUtilRegistry.ps1`, and `pester/tweaks.Tests.ps1` confirms the catalog is interpreted as declarative inputs to generic helpers, with several important limits:

- Apply selects registry `Value`, service `StartupType`, and `InvokeScript`; undo selects `OriginalValue`, `OriginalType`, and `UndoScript`. The Pester dispatcher tests assert these argument choices. `OriginalValue`/`OriginalType` are catalog-provided values, not values snapshotted from the machine before the change.
- Registry entries with a `Values` member are filtered out by this ordinary setter loop. The registry helper creates a missing key path, writes values with `Set-ItemProperty`, and interprets the literal `<RemoveEntry>` as removal of the named value. Thus a catalog “undo” may create a missing key before attempting to restore/remove a value; it is not a transaction or restoration of an observed prior state. The helper catches and logs errors instead of returning a transaction result.
- Service apply defaults `KeepServiceStartup` to true: it queries `Get-Service` and only calls the setter when the observed startup type equals catalog `OriginalType`; callers can disable that guard. Undo directly passes catalog `OriginalType`. In the not-found catch path, the code warns but does not clear `$changeservice`, so it proceeds to call the service setter afterward. This behavior is source-observed, not validated against a live SCM.
- `InvokeScript` and `UndoScript` strings are converted to PowerShell scriptblocks and passed to `Invoke-WinUtilScript`. Their effects and rollback completeness are entry-specific; presence of an undo script is not proof of symmetry or safety.
- The dispatcher invokes AppX removals only during apply, including provisioned packages; there is no corresponding undo dispatch in this function.

These observations establish dispatch semantics only. The included Pester tests mock registry/service/script/AppX helpers, so they verify routing and arguments, not Windows state changes, failure atomicity, actual helper effects, or safety. Do not port this helper behavior as authorization to mutate state: the Rustweaker transaction/backup contract and supported-OS gates still apply.

## Selection ID and preset extraction

`config/selection-catalog.v1.json` is a generated selection-only fixture, anchored to SHA-256 values for the frozen `applications.json`, `tweaks.json`, `feature.json`, `appx.json`, and `preset.json`. It includes IDs recognized by `Update-WinUtilSelections`: 232 applications, 38 tweaks, 23 toggles, 10 features, and 33 AppX entries. The feature JSON also contains `WPFFixes*` and other UI entries; those are excluded because they are not recognized selection families in the helper. `preset.json` contains four named presets: Standard, Minimal, Advanced, and AppxDefault.

Run `python scripts/verify-selection-catalog.py` to verify hashes, category membership, preset order, and that each preset ID belongs to exactly one family. The verifier parses `tweaks.json` and `feature.json` with Python's `strict=False` because source string values contain literal newlines; the frozen files are never rewritten. This fixture only validates selection IDs and preset membership. It does not describe tweak operations, OS support, installation behavior, rollback, or permission to mutate system state.

## Window Snapping read-only observer

The implemented `WPFToggleWindowSnapping` observer is exposed by `--toggle-status WPFToggleWindowSnapping <native|64|32>`. It reads only `HKCU\Control Panel\Desktop\WindowArrangementActive`, accepts a bounded valid `REG_SZ`, treats a missing key/value as checked per the source declaration, and reports valid nonmatching strings as unchecked. Unsupported types, malformed strings, or read failures are unknown. Output excludes raw registry bytes and has `executable=false`. This does not implement the toggle action or establish that the setting has a Windows effect. The UI remains disconnected from this observer.

The source entrypoint/ID discovery is recorded in [`powershell-action-inventory.md`](powershell-action-inventory.md). It lists 33 public handlers and the five catalog families, while explicitly distinguishing Rust data/read functionality from absent action execution. The linked source paths and catalog totals were verified against the read-only checkout. This is not yet a per-action side-effect, OS-support, or recovery audit.
