# PowerShell to Rust port roadmap

Status: working plan, verified against the checkout on 2026-10-28. The frozen `winutil-26.08.19/` tree is a read-only reference. A catalog ID is not an implementation or authorization to execute an action.

## Current state

- Rust has data-only catalogs, selection import/export/preset preview, package plans and list matching; Windows observers cover selected toggle/tweak declarations; storage/journal/recovery prototypes and guarded registry transaction paths exist.
- The Preact interface is a standalone preview with shared local selection across apps, tweaks, toggles, features and AppX. It has no Rust bridge; all system/package actions remain unavailable.
- Some backend capabilities and tests exist, but user-facing actions are not generally routed through a complete transaction/recovery contract. Source-side `OriginalValue` is not the actual machine pre-state.
- AppX, feature enablement, most tweak actions, script actions, network/power, cleanup/repair, update and package execution are not ported as enabled operations.

## Roadmap and acceptance gates

### 1. Complete source behavior inventory

Inventory dispatcher handlers and every selection family/action: all 66 tweak declarations, 232 application records, features, AppX, presets, package managers, repair/cleanup/network/update/power and public UI actions. For each, record exact source references, conditions, reads/writes, child processes/downloads, OS assumptions, side effects, rollback claims and unresolved behavior. Distinguish verified source facts from Rust implementation and Windows evidence. Preserve MIT notice and review third-party runtime/package licensing.

**Accept when:** a reproducible inventory covers every exposed action/ID or explicitly marks it unsupported/blocked; source hashes and references are checked; no design-proposal paths are presented as implemented.

### 2. Close durable recovery foundations before live writes

Finish bounded journal/checkpoint lifecycle and recovery invariants; validate protected-root ACLs with a separate account and positive controls; test path replacement/reparse attacks, concurrent operations and every write/flush/rename/readback interruption; demonstrate behavior in disposable VMs with hard power-off where possible. Document filesystem/device durability limits.

**Accept when:** failures before durable backup publication never reach a mutation; reopen and recovery are idempotent; partial restore can resume safely; adversarial ACL/path/locking tests pass; Win7 x86/x64 and current Windows VM evidence is recorded. Host tests alone do not prove power-loss durability.

### 3. Establish action authorization and typed transactional backends

Create a reviewed immutable source/provenance validator and enforce OS, architecture, elevation, hive/view and resource constraints at runtime. Implement typed registry, service, filesystem and process/package boundaries only as needed. Before each write, persist exact observed state (including absence and native type/data), verify publication, then apply; restore the captured state on failure. Treat external commands and deletions as potentially non-reversible.

**Accept when:** each resource type has mock-backed snapshot/apply/undo and failure-injection tests, plus disposable-VM validation; no catalog-deserialized value can authorize writes by itself; unsupported or unbackable operations fail closed.

### 4. Finish read-only parity and selection contracts

Bind catalogs/observers to source hashes and per-entry evidence. Cover exact types, defaults, missing keys/services, registry views and unknown-on-error behavior. Keep the five-family selection/preset workflow data-only and align Rust/TypeScript fixtures and edge cases. Add the remaining independently verifiable observers before action execution.

**Accept when:** every surfaced observer has source references, bounded parsing, exact target identity, tests for missing/malformed/error cases and `executable=false`; Rust and UI selection tests agree; the preview never implies an action succeeded.

### 5. Port declarative tweaks and toggles incrementally

For each entry compare dispatcher behavior, conditional guards, apply and undo semantics, OS support and all associated registry/service values. Add one operation at a time through the transaction layer. Treat multi-value and combo actions as transactions over all members. Audit every custom script separately; do not translate a script into an assumed declarative equivalent.

**Accept when:** each enabled ID has complete provenance, supported-OS evidence, exact backup/recovery behavior, no-write-on-backup-failure tests, partial-failure rollback tests and VM validation. The 66 IDs are not 66 safe generic registry operations: 25 include scripts and two define combo items.

### 6. Audit subsystem operations independently

AppX/provisioned packages, optional features, network/DNS/Winsock, power plans, restore points, updates, cleanup, repair, and service/network fixes each need a separate side-effect and recovery design. Installer processes need bounded output, timeout/cancel, exit/state verification, provenance and realistic rollback. Irreversible actions can remain unavailable when they cannot meet the backup invariant.

**Accept when:** each subsystem has an explicit OS capability matrix and action-level evidence, or is visibly marked unavailable with a reason. A generic restore point is not a substitute for exact transaction records.

### 7. Integrate a minimal frontend after stable APIs

Keep the current preview separate until a narrow Rust bridge exists. Then expose only supported operations through asynchronous jobs, with progress, exact plans, results/errors and recovery state. Do not block the Sciter UI thread; preserve Win7 runtime constraints and licensing/redistribution requirements.

**Accept when:** bridge payloads are bounded and typed, unsupported actions cannot be invoked, UI integration tests cover cancel/error/recovery flows, and actual rendering is verified on supported OS/architectures.

### 8. Release verification

Run locked Rust tests/check, strict Clippy, formatting and UI build; build i686 and x86_64 Win7 SP1 release targets; perform complete PE import/subsystem audits; exercise disposable VMs for each claimed Windows version and architecture; verify UI runtime version/licensing and packaging. Separate host compilation evidence from actual Win7 execution evidence.

**Accept when:** every claimed capability has reproducible build, test, import-audit and VM evidence; known limitations and unsupported actions are documented.

## Immediate next work

1. Handle-relative journal I/O is implemented and independently reviewed on the host: all-features tests, locked check, strict Clippy, fmt and both nightly Win7 target builds pass. Root-substitution tests pass; the ancestor replacement test is conditional because this host denies moving an open ancestor. Continue with PE import audit, named-account ACL proof, restart identity, power-loss and Win7 VM gates; these remain release blockers.
2. Expand `powershell-action-inventory.md` from ID/entrypoint coverage into per-action semantics. Activity History, AppX install/removal, and optional-feature handlers now have focused source audits; the rest of tweak, toggle, feature, AppX, package, repair, cleanup, network, update, and power actions still need equivalent evidence.
3. Close journal-root ACL/path attack, separate-account, crash-durability, user/profile identity, and disposable-VM gates before enabling live system mutations.
4. Complete source-traced read-only observers and shared Rust/TypeScript selection contract coverage in parallel with independent source audits.
5. Port actions incrementally through persisted exact pre-state and typed recovery contracts; prioritize reversible declarative actions and keep unbackable scripts/package/subsystem operations unavailable until their own contracts are proven.
6. Connect the minimal asynchronous UI bridge only after stable, reviewed operation APIs exist, then complete target, PE, runtime/licensing, packaging, and VM release verification.

## Source-handler spot check (not a complete inventory)

Read-only verification of `winutil-26.08.19/functions/public/Invoke-WPFButton.ps1`, `Invoke-WPFtweaksbutton.ps1`, and `Invoke-WPFundoall.ps1` confirms the public dispatcher routes tweak apply/undo, install/uninstall/upgrade, AppX install/removal, ultimate-performance plan, and Windows Update choices. `config/feature.json` maps optional-feature installation, update fixes, and network fixes to handlers; `config/tweaks.json` includes `WPFchangedns`. The source has handler files for these families, including `Invoke-WPFSystemRepair.ps1`, `Invoke-WPFFixesNetwork.ps1`, `Invoke-WPFUltimatePerformance.ps1`, `Invoke-WPFAppxInstall.ps1`, `Invoke-WPFAppxRemoval.ps1`, and `private/Set-WinUtilDNS.ps1`.

Current Rust source has data-only package planning/import and selection handling, read-only diagnostics/inventory/status/Winsock reporting, and generic transaction/tweak plan infrastructure. That infrastructure does not constitute routed AppX, feature, repair/cleanup, DNS/network reset, power plan, restore point, or Windows Update actions. Full package install/uninstall/upgrade execution is also not exposed. These gaps remain unavailable; ID recognition and a generic plan type do not provide action parity. ISO/USB workflows are outside this spot check and need explicit scope review.

## First source-specific operation slice

`window_snapping_plan.rs` now translates one audited toggle's apply/source-undo values into exact typed operations with mock transaction/recovery tests. It is internal and intentionally has no executor route. This begins the per-action semantics stage but does not alter the current full-port gap: other toggle/tweak and subsystem actions remain unimplemented, and live HKCU action support remains blocked by identity-bound recovery and journal path security.

## HKEY_USERS mutation gate — 2026-10-28

- Closed the `Hive::Users` write bypass: `TweakPlan` rejects HKU set/delete operations (including later items in a batch), and the guarded Windows registry backend rejects HKU transaction/recovery queries before mutation. The independent read-only path remains available for observers such as NumLock.
- The reviewed registry fixture now expects `WPFToggleNumLock`'s HKU operation to be rejected alongside HKCU operations. Existing persisted HKU mutations therefore fail closed during Windows recovery and remain journaled; no migration or automatic identity binding is attempted.
- Verification: Rust 1.97.1 locked all-features tests passed (320 library + 23 CLI), all-features check, strict Clippy and fmt passed. Nightly 1.101.0 Win7 i686/x86_64 experimental-ui release builds passed. Six vendored Sciter warnings remain. The selected PE audit was not rerun because `dumpbin`/`llvm-objdump` are unavailable in this environment; no Win7 VM or live registry write was run.
- Next: continue action-level source semantics and close journal-root identity/path replacement, separate-account, power-loss and Windows VM gates. HKCU/HKU writes remain disabled until recovery binds and verifies user/profile identity, and the journal foundation is independently accepted.
