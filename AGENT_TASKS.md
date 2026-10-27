# Agent task queue

Journal-root ACL creation/open validation is implemented in `src/data/windows_security.rs` and integrated with `FileJournalStorage`. Remaining path-replacement, cross-account, external-writer, power-loss, and Windows 7 VM criteria are recorded in `docs/journal-root-security.md`; this remains an open release gate.

Updated: 2026-10-28. A0 host CI matrix is locally verified and the project baseline is tracked locally; remote CI remains outstanding. A1 source/provenance report acceptance is complete with compatibility, package and per-operation behavior limits explicitly unverified. A2/A3 reports exist with release gaps. B1 now has typed catalog schema v1 and bounded decoder plus one test-only embedded review fixture; there is no production loader or executor. B2 has guarded registry/service prototypes routed through managed file transactions. The Win32 setter has not been executed. Application UI/CLI mutation entrypoints remain absent; journal namespace replacement, final SCM external-actor race and power-loss release gates remain open. See the latest B1/B2 handoffs. `AGENTS.md` and `AGENT_START.md` apply to every task. Work against actual files, not imagined interfaces. Record results using the handoff template.

## Phase 0: establish a buildable foundation

**A0 — Bootstrap and integration owner** (first; owns root manifest, `src/lib.rs`, `src/main.rs`, build configuration and CI skeleton). Create a minimal Rust project with a non-mutating entry point, clear module boundaries, and reproducible host checks. Decide shared conventions and integration branch/ownership before parallel edits. Do not introduce UI binaries, live system mutations or unverified dependency assumptions. Acceptance: host check, library tests, formatting and clippy pass; report exact toolchain. Investigate both Win7 targets and prerequisites and report evidence; do not declare compatibility based on host success. Publish interfaces/file ownership before A1–A3 integrate.

**A1 — Source inventory and provenance** (can begin in parallel; owns a new provenance report under `docs/`, not the upstream or shared inventory). Examine upstream catalogs and corresponding implementations; identify actual counts, entry types, custom actions, required licenses and differences from `TWEAKS_INVENTORY.md`. Produce traceable source paths and a small verified sample, marking unknowns. Acceptance: reproducible counting method, paths and explicit discrepancies; no catalog claims inferred solely from existing prose. Coordinate catalog schema with A0/A2.

**A2 — Safety and rollback contract** (can begin in parallel; owns a new contract document under `docs/`; implementation after A0 integration). Specify persisted snapshots for missing/present values, exact types, service states and filesystem objects, durable ordering, failure/recovery and mock tests. Identify operations that cannot meet the invariant and keep them disabled pending a solution. Acceptance: reviewable operation lifecycle and failure cases, including backup failure before mutation, interruption, partial application and restoration. No live system changes.

**A3 — Compatibility and UI feasibility** (can begin in parallel; owns a new feasibility report under `docs/`). Verify toolchain and target prerequisites, PE import audit process, and exact Sciter runtime/bridge, licensing, redistribution, platform and Preact compatibility using primary evidence and, where feasible, a disposable proof of concept. Distinguish Win7, WinPE and Safe Mode evidence; report unresolved constraints and decisions. Acceptance: evidence links, commands and observed results; no unsupported promise or silent runtime download.

## Phase 1: after integration contracts are agreed

**B1 — Data and transaction foundation** (owns `src/data/`, assigned config schema files; depends on A0 and A2). Implement typed, versioned snapshot storage and catalog schemas, durable persistence and mock-based failure/recovery tests. Coordinate trait contracts with B2. No OS mutations.

**B2 — Registry and service backends** (owns `src/engine/registry.rs`, `services.rs` and related tests; depends on A0, A2 and B1 contracts). Implement mock-first query and transactional mutation through persisted backup, exact restore of absent/present values and correct views/handle ownership. Acceptance: tests prove no writes on snapshot failure and restoration after partial failures; report both Win7 builds and VM tests separately.

**B3 — UI shell** (owns `ui/` and agreed bridge adapter files; depends on A0 and A3; contract with integrator). Build a non-mutating shell and explicitly mocked data until bridge and runtime are validated. Acceptance: frontend build, bridge contract tests where available, documented runtime/OS verification and no fake success for system operations.

**B4 — Catalog implementation** (owns agreed `config/` files; depends on A1 and B1 schema). Add a small traceable set first, verify every action and OS constraint against reference and tests; expand only as supported. Do not turn defaults into rollback state. Acceptance: source provenance, schema validation, mock forward/undo tests and explicit unsupported cases.

Later cleanup, installation, repair, diagnostics and CLI tasks require separate contracts and test plans. Do not expose operations that cannot satisfy the safety gate. Integrator resolves shared-file conflicts and performs final verification; task completion is not release certification.

## A0 handoff — 2026-10-27

- Status: bootstrap implemented; Win7 compatibility and CI execution remain unverified. Shared files are owned by the integrator: `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/main.rs`, `.gitignore`, `.github/workflows/rust.yml`. Downstream agents should coordinate changes to these files; domain files belong to the task owners listed above. Interfaces beyond the empty library are intentionally not yet fixed; agree the transaction and backend contracts before B1/B2 implementation.
- The non-mutating binary only prints an under-development message; it has no functional CLI, UI, or system operations. No dependencies or async runtime were introduced. Work remains in the existing checkout on `main`; no integration branch or commit was created. Coordinate ownership before concurrent changes.
- Default PATH: Chocolatey cargo/rustc 1.97.0 (host `x86_64-pc-windows-gnu`). `cargo check`, `cargo test`, `cargo test --lib`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` passed. Zero tests exist. `rustup run stable cargo check` and `rustup run stable cargo test --lib` also passed with MSVC 1.97.1. CI workflow has not been run remotely.
- Both requested `cargo +nightly build` commands failed because PATH's Chocolatey cargo is not a rustup proxy. `rustup run nightly cargo --version` confirms nightly MSVC is not installed. Rustc lists both Win7 target names, but this does not prove they build or run. Nightly with `rust-src`, MSVC target/linker prerequisites, PE import auditing and Win7 VM testing remain for compatibility work; do not infer compatibility from host checks.
- Next: A1, A2 and A3 can proceed with their isolated reports; agree B1/B2 contracts before implementation.

## Continuation handoff — 2026-10-27

- `docs/source-inventory.md` records reproducible initial counts and examples, including the upstream tweak file's non-standard unescaped newlines. Full catalog provenance and compatibility remain unverified.
- `docs/rollback-contract.md` specifies pre-state, ordering, recovery and test requirements. It is a design, not a durability guarantee.
- `src/engine/transaction.rs` contains a generic single-operation capture/persist/apply coordinator and four mock tests. It has no actual durable store, rollback implementation, target locking, registry/service backend or production caller. `SnapshotStore::persist` is a contract that an implementation must prove; a mock passing does not prove durability. Do not expose real mutations until the full safety contract is enforced.
- Default host: `cargo check`, `cargo test` (4 tests), `cargo fmt --check`, and strict clippy passed. Rustup stable MSVC: `cargo check`, `cargo test --lib` (4 tests), and strict clippy passed. Both Win7 release builds and PE audits remain unverified.
- Next: implement and test durable journal and recovery, verify upstream samples and UI/Win7 feasibility; then develop real backends behind those guarantees.

## Recovery foundation — 2026-10-27

- `src/engine/recovery.rs` adds conflict-aware single-record recovery, verification and retry tests using an in-memory backend. It deliberately requires caller-held locking and persisted recovery progress; it does not connect to the current snapshot-only store or a real backend. It cannot detect ABA changes. Recovery writes must themselves satisfy the backup-before-mutation invariant.
- `cargo check`, `cargo test` (8 tests), `cargo fmt --check`, and strict clippy passed on the default GNU host. Windows 7 release builds, OS-level persistence, and VM recovery tests remain outstanding.

## Recovery backup — 2026-10-27

- `SnapshotStore` now receives both pre-state and intended change. `recover_one` requires a store and persists the current and intended restoration states before a restore call; failed persistence prevents that call. Tests cover this contract using a mock.
- GNU host: check, test (10 tests), format check and strict clippy pass. MSVC stable: `cargo test --lib` passes (10 tests). No durable store, locking, Windows backend or real recovery is implemented; these tests do not prove persistence or crash safety.

## Registry model — 2026-10-27

- `src/engine/registry.rs` models hive, view, value name, native type and exact bytes. A test-only in-memory backend exercises apply and undo, absent values, view isolation and journal failure. This model does not yet capture missing keys, normalize Windows case-insensitive names, implement Windows APIs, or supply real locking and durable storage. Do not enable it as a system backend.
- GNU host check/test (13 tests)/fmt/clippy passed; stable MSVC `cargo test --lib` passed (13 tests). Win7 builds remain unverified.

## Journal format — 2026-10-27

- `src/data/journal.rs` provides bounded, versioned JSON encoding and validation for a registry record; it is not a disk journal or integrity check. Valid but altered JSON is not detected; malformed, truncated, oversized and unsupported-version inputs are rejected. It does not yet represent key existence or arbitrary Windows UTF-16 names. No real changes are enabled.
- Added serde and serde_json, recorded in Cargo.lock. GNU check and 16 tests passed. A mixed-toolchain Clippy invocation failed (Chocolatey rustc 1.97.0 and rustup Clippy 1.97.1); after putting `C:\Users\FeoreV\.cargo\bin` first in PATH and selecting a separate target directory, MSVC rustup `cargo check`, `cargo test` (16), `cargo fmt --check` and strict Clippy all passed. Earlier `rustup run stable cargo` tests did not establish which rustc was used because PATH still selected Chocolatey rustc; treat those earlier MSVC labels as unverified. Both Win7 targets and dependencies remain unverified.

## Journal write protocol — 2026-10-27

- `src/data/store.rs` connects registry snapshot storage to a storage abstraction: exclusive create, synchronization, fresh read and byte/record verification. An I/O or verification failure poisons the writer so sequence numbers are not silently reused. Mock tests cover each failure stage and duplicate IDs. This is not an actual disk implementation: durable namespace synchronization, ID allocation, locking, later integrity verification and crash recovery remain unproven.
- With rustup first in PATH and isolated target directory, MSVC `cargo check`, `cargo test` (19 tests), `cargo fmt --check` and strict Clippy passed. Win7 targets and actual disk durability remain unverified.

## UI preview — 2026-10-27

- `ui/` contains a Preact/TypeScript preview with navigation and unavailable-state messaging; it performs no system actions and is not connected to Sciter or the Rust bridge. `npm --prefix ui run build` passed with Node 26.5.0/npm 11.17.0 and produced `ui/dist/bundle.js` and `ui/dist/styles.css`. Browser or Sciter rendering, accessibility, Win7 runtime and production packaging remain unverified. npm reported one pending esbuild install-script approval, although this build passed.
- Rust MSVC `cargo check` and `cargo test` passed (19 tests).

## UI verification — 2026-10-27

- Loaded the local preview in the in-app browser; Overview rendered with disabled actions, and navigation to Tweaks and Applications updated the heading and unavailable state. Visual screenshot inspected at the default viewport. This is browser evidence only, not Sciter, Win7, keyboard-accessibility or full responsive verification.
- `npm --prefix ui ci` and `npm --prefix ui run build` passed on local Node 26.5.0/npm 11.17.0. `.github/workflows/ui.yml` adds Node 22 CI build and checked-in asset diff; remote execution and Node 22 have not been verified locally.

## Tweak catalog preview — 2026-10-27

- `scripts/extract-tweak-display.py` extracts ID, name, category and description only from the read-only upstream catalog into `ui/src/data/tweaks.json`; it never imports executable scripts or registry operations. `THIRD_PARTY/WINUTIL-LICENSE.txt` preserves the upstream MIT notice; release packaging must include it. The UI shows 66 source entries with search and disabled actions. Descriptions are upstream claims, not verified compatibility or behavior.
- `npm --prefix ui run build` passed. In the local in-app browser, Tweaks displayed 66 entries and searching `hibernation` displayed 1 result. MSVC cargo check, test (19), fmt and strict clippy passed. Sciter/Win7 and actual tweak execution are not verified.

## Compatibility evidence — 2026-10-27

- `docs/compatibility-feasibility.md` records official sources and current local prerequisites. A specific Sciter runtime/version, its license, Preact behavior, Win7 builds, import audit and VM checks remain open. No release readiness claim follows from this research.
- UI CI now verifies that both generated assets are tracked before checking their diff; the workflow itself remains unexecuted remotely.

## Windows registry read — 2026-10-27

- `src/engine/registry/windows.rs` adds read-only `RegOpenKeyExW`/`RegQueryValueExW` support with owned-handle cleanup, explicit view selection, missing-key vs missing-value results, exact native type/bytes, size bounds and bounded retry for resizing values. It is not wired into application behavior and cannot write. `String` target names cannot represent every possible UTF-16 registry name; snapshots and durable storage are incomplete.
- Stable MSVC host check, test (21 tests including read-only SystemRoot query), fmt and strict clippy passed. No x86/Win7 build or OS-specific VM test was performed.

## Read-only CLI — 2026-10-27

- `src/main.rs` supports help, version and `--read-registry <hive> <view> <key-path> <value-name>`; output is JSON with missing-key/value distinctions or native type and bytes. It has no mutation commands and unsupported arguments fail with exit 2. No arguments fail explicitly because GUI is not implemented.
- Stable MSVC `cargo check`, `cargo test` (21 library, 2 CLI), fmt and strict clippy passed. A real read of HKLM Windows NT CurrentVersion SystemRoot and `--help` succeeded; `--clean` returned exit 2. Win7/x86 and Sciter remain unverified.

## Read-only diagnostics — 2026-10-27

- `src/engine/diagnostics.rs` uses RtlGetVersion and GlobalMemoryStatusEx to report OS version/build and total/available physical memory; `--diagnostics` prints JSON. It does not perform repairs or mutate system settings. CPU and disk telemetry remain unimplemented.
- Stable MSVC cargo check/test (22 library and 2 CLI tests), fmt and strict clippy passed. `cargo run -- --diagnostics` returned plausible data on the current host. Win7/x86, PE imports and UI bridge remain unverified.

## Win7 build experiment — 2026-10-27

- Installed rustup nightly 1.101.0 and rust-src. Both Win7 release builds pass with `.cargo/config.toml` static CRT and console subsystem 6.01. Selected forbidden imports and dynamic CRT DLLs are absent according to dumpbin; x86 and x64 `--version` and `--diagnostics` run on the current Windows host. See `docs/compatibility-feasibility.md` for limits. Full import audit, Win7 VM, GUI and third-party runtimes remain unverified.
- Stable MSVC cargo check and test passed (24 tests).

## Automated import gate — 2026-10-27

- `scripts/audit-win7-imports.ps1` checks selected forbidden imports, dynamic CRT DLLs and subsystem 6.01 using dumpbin. Both current Win7 binaries pass; a normal host debug binary was correctly rejected for WaitOnAddress/WakeByAddress and dynamic CRT. This is not a comprehensive Windows 7 compatibility audit.
- `.github/workflows/win7.yml` adds x86/x64 nightly build and this audit; remote workflow and dated toolchain installation were not exercised. Stable host cargo check and 24 tests pass.

## Reverse batch recovery — 2026-10-27

- `recover_reverse` recovers records in reverse and requires a checkpoint after each step before an earlier record is touched. Mock tests cover duplicate targets, conflicts, checkpoint failure and retry. The caller must persist the initial checkpoint, bind it to a validated transaction, hold locks and supply a truly durable store/checkpoint. Neither persistence nor real OS restoration exists; ABA and concurrent changes remain limitations.
- Stable MSVC check, 27 tests, fmt and strict Clippy passed. Both Win7 target builds and selected import audits passed after the change; VM execution remains outstanding.

## Checkpoint binding — 2026-10-27

- `src/data/checkpoint.rs` provides bounded, versioned checkpoint encoding bound by SHA-256 to the exact ordered registry records, transaction ID and sequence numbers. It detects changes to records relative to a retained checkpoint, not malicious replacement of both or corruption of checkpoint progress. It does not persist, authenticate, lock or drive actual recovery.
- `docs/journal-persistence.md` records Windows flushing evidence and unresolved durability limits. No disk backend is enabled. Stable MSVC check, 30 tests, fmt and strict Clippy passed. Both Win7 target builds and selected import audits passed; no Win7 VM run.

## Checkpoint corruption detection — 2026-10-27

- Replaced the earlier SHA-256 implementation and removed `sha2` to honor allowed dependencies. Version 2 checkpoints retain the exact ordered records and CRC-32 over canonical payload, including progress; changed progress without updated CRC is rejected. CRC is only accidental-corruption detection, not authentication or protection against deliberate recomputation. Old version 1 is unsupported; no persistent checkpoints existed. Checkpoint storage, locks and real recovery are still not implemented.
- Stable MSVC check, 32 tests, fmt and strict clippy passed. Both Win7 target builds and selected import audits passed. Win7 VM remains untested.

## CPU diagnostics — 2026-10-27

- `--diagnostics` now includes an optional CPUID processor brand for x86/x64, without WMI. Unsupported or malformed brand data returns null. Stable MSVC check, 33 tests, fmt and strict Clippy passed. Both Win7 targets rebuilt and passed selected import audits; both CPU diagnostic commands ran on the current host. Win7 VM and disk diagnostics remain outstanding.

## System volume capacity — 2026-10-27

- `--diagnostics` queries free and total space for the volume containing the Windows directory using `GetWindowsDirectoryW` and `GetDiskFreeSpaceExW`. Caller-specific fields may reflect quota; total free is separate. An API failure is returned as `system_volume_error`, not silently represented as zero. SMART, physical drive model and volume-to-device mapping remain unimplemented.
- Stable MSVC check, 33 tests, fmt and strict clippy passed. Both Win7 target builds, selected import audits and x86/x64 diagnostic runs succeeded on the current host; Win7 VM remains untested.

## Read-only service query — 2026-10-27

- `src/engine/services.rs` opens the local SCM and service with query-only permissions, closes both handles with RAII, and reports status or missing service. `--service-status <name>` outputs JSON; it does not control services or read their configuration. Stable MSVC check, 36 tests, fmt and strict clippy passed. Win7 x86/x64 builds and selected audits passed and queried EventLog on the current host; Win7 VM remains untested.

## Registry snapshot distinction — 2026-10-27

- Registry `ValueState` now differentiates missing key, missing value and present native type/bytes; Win32 reads and the journal use the same representation. Journal format is version 2; old format is rejected. Tests cover serialized distinction and mock restoration. A missing-key marker does not identify missing ancestors, prove safe key deletion, or provide a real restoration backend.
- Stable MSVC cargo check, 38 tests, fmt and strict Clippy passed. Both Win7 release targets and selected import audits passed; Win7 VM remains untested.

## Service configuration query — 2026-10-27

- `--service-config <name>` queries selected numeric service configuration: type, start type and error control. It uses SCM query-only rights and RAII handles. This is not a complete service snapshot: paths, account, dependencies, delayed start and other settings are not captured; service mutations remain unavailable.
- Stable MSVC check, 39 tests, fmt and strict Clippy passed. Both Win7 target builds and selected audits passed; both queried EventLog on the current host. Win7 VM remains untested.

## Read-only storage descriptor — 2026-10-27

- `--disk-info <0..255>` opens the explicitly numbered physical drive with zero desired access and queries `IOCTL_STORAGE_QUERY_PROPERTY`. It reports vendor, model, bus type and removable flag, validates descriptor bounds and closes the handle. It does not identify the system drive or report SMART/health; access and unsupported device failures are surfaced.
- Stable MSVC check, 40 tests, fmt and strict Clippy passed. Both Win7 target builds and selected import audits passed; both queried PhysicalDrive0 on the current host. Win7 VM and wider device coverage remain untested.

## Experimental Sciter host — 2026-10-27

- Optional `experimental-ui` enables `--ui-preview <absolute-sciter-dll-path>`, embedding UI assets and loading an explicitly supplied runtime. The default build does not enable UI. There is no download, extraction, native bridge, production packaging or system action. `sciter-rs` 0.5.58 is optional and is distinct from the Sciter runtime license. The Rust binding and both Win7 target builds compile; selected import audits pass.
- Stable MSVC check, 40 default tests, fmt and strict Clippy passed; feature-enabled check, 42 tests and strict Clippy passed. Feature builds were not executed with a Sciter DLL, and DOM/CSS/JS compatibility, licensing, graphics fallback, Win7 VM and crash behavior remain unverified. CI includes the feature but has not run remotely.

## Sciter runtime probe — 2026-10-27

- `--sciter-version <absolute-dll-path>` loads an explicitly supplied runtime without opening a window. Both x86/x64 Sciter.JS 4.4.8.34 binaries from a pinned vendor archive loaded with matching application architectures on this host. Blob hashes and provenance are documented in `docs/compatibility-feasibility.md`; evaluation DLLs are ignored, not shipped. No UI rendering, production licensing conclusion or Win7 VM test.
- Default MSVC check, 40 tests, fmt and clippy passed; feature-enabled check, 42 tests and clippy passed. Both experimental UI Win7 builds and selected import audits passed.

## Hidden Sciter interaction — 2026-10-27

- `--sciter-smoke <absolute-dll>` loads the embedded UI in a hidden window and verifies Overview -> Tweaks after pumping messages. It passed for both target architectures on the current host using Sciter.JS 4.4.8.34. Shared CSS now uses focus and range media syntax accepted by this Sciter version; no CSS parser warnings appeared in the smoke output. Visual layout, full CSS, input/search, accessibility and Win7 remain unverified.
- The initial test exposed invalid-enum transmute in `sciter-rs` 0.5.58 `Window::new`; a single window flag avoids that path. Binding soundness is not established; callback enum transmutation also needs audit. The UI remains experimental.
- MSVC cargo check, default test (40), feature test (42), fmt and feature clippy passed; both Win7 builds and selected import audits passed.

## Local Sciter binding fixes — 2026-10-27

- `vendor/sciter-rs` contains the MIT-licensed 0.5.58 binding with raw notification matching and removal of unsound shared-reference `Value` indexing; Cargo uses it via a local patch. See `vendor/sciter-rs/PATCHES.md`. Other transmutations and mutable statics remain unaudited; compiler warnings remain visible.
- Both Win7 feature builds and selected import audits passed; x86/x64 hidden Sciter DOM smoke tests passed on the current host. This is not Win7 execution or a complete soundness review.

## A0 revalidation and Sciter callback ABI audit — 2026-10-27

- A0's host foundation checks pass with rustup MSVC Rust/Cargo 1.97.1: `cargo check --locked`, `cargo test --locked --lib` (38 library tests), `cargo fmt --check`, and strict Clippy. The integrated UI-feature run passes 40 library and 2 CLI tests; feature Clippy also passes.
- `eventhandler.rs` no longer transmutes raw event-group, method, behavior-event, phase, click/edit reason, or draw-layer values into Rust enums. `EVENT_GROUPS` and `BEHAVIOR_EVENTS` are transparent integer newtypes, preserving C width while accepting combined flags and custom event IDs. Initialization/SOM/draw command fields are read as raw `UINT`; unsupported phase/draw values and null parameters return false; unknown reason values use `EventReason::Unknown`.
- Targeted vendored tests for flag combinations, arbitrary app event IDs, unknown phases/layers/reasons pass. The full standalone vendor suite ran 11 tests successfully and failed 2 runtime-dependent `value` tests because `sciter.dll` was not on the test process path. This is a missing test runtime, not a claimed pass. The vendor build still emits pre-existing mutable-static, lifetime, and ownership warnings.
- Both nightly Win7 release targets rebuilt with `experimental-ui`; `scripts/audit-win7-imports.ps1` passed selected imports/subsystem checks; matching x86/x64 Sciter smoke runs returned `Overview -> Tweaks` on the current Windows host. No Win7 VM execution or full binding audit follows from this.
- Current project files remain untracked on `main` and `.agents/memory/*` has existing local edits. Keep shared root files with the integrator; A1/A2/A3 report scopes and B1/B2/B3/B4 ownership above remain distinct. A clean worktree based on `main` omits these files, so do not dispatch work there until an agreed baseline commit exists. Do not stage or commit the existing untracked project wholesale without the owner's instruction.
- Next: continue A3 against remaining unsafe code/runtime and license questions, then proceed to B1's durable on-disk storage behind the existing mock contracts. A0 is the start of the sequence, not a project completion state.

## A3 continuation: DOM/window bitmask ABI audit — 2026-10-27

- Replaced `ELEMENT_STATE_BITS` and `SCITER_CREATE_WINDOW_FLAGS` enum/transmute bitmasks with transparent `UINT` wrappers. `dom.rs` reads unknown state bits without constructing invalid enums; `WindowBuilder` masks the raw flag value and `Window::create` passes the underlying ABI value. The old single-flag workaround is no longer needed to avoid the invalid enum path.
- Added focused tests for combined and unknown DOM state bits and combined/unknown window flags. Root feature tests (40 library, 2 CLI), formatting, feature check, and strict Clippy passed.
- Both nightly Win7 release targets rebuilt with `experimental-ui`; selected import/subsystem audits and x86/x64 hidden Sciter smoke returned `Overview -> Tweaks` on this host. The binding still has unrelated unsafe ownership/pointer paths, warnings, and unreviewed runtime/license assumptions. Continue A3; do not enable system-changing actions.

## A3 continuation: loader shared state — 2026-10-27

- Replaced Sciter's mutable global custom-library path and extension host API with mutex-protected state. Loader calls clone the configured path before calling OS loading APIs; lock poisoning recovers the stored configuration. This removes these two mutable-static races, but does not establish thread safety for the binding as a whole.
- `cargo check --locked --features experimental-ui`, integrated feature tests, `cargo fmt --check`, strict feature Clippy and standalone vendored `extension` feature check passed. Both nightly Win7 release targets rebuilt; selected import/subsystem audits and x86/x64 Sciter smoke passed on this host.
- Remaining A3 work: review binding pointer ownership/lifetimes and function-pointer ABI assumptions, runtime licensing/redistribution evidence, and actual Win7 execution. Do not treat the binding as production-safe or turn on system mutations.

## A3 continuation: SOM asset ownership — 2026-10-27

- Removed undocumented `Send` from Sciter `Value` and video asset pointers, and `Send`/`Sync` from DOM elements. `Value::isolate` alone did not enforce the condition because the wrapper remained unconditionally transferable.
- Removed the safe `IAsset::from_raw(&*mut) -> &mut IAsset` aliasing escape. SOM add/release callbacks now access only the `AtomicI32` field through raw projection, and passport pointers are computed once before publishing the asset rather than mutating cached state in callbacks. Null callback handles return zero/null. `IAssetRef::from_raw` now requires `unsafe` and states its liveness contract.
- Added isolated vendor tests for asset reference clone/release lifetime, passport access, and null handles; both tests pass. `AssetPtr::adopt` now requires `unsafe`, the safe `From<*mut T>` path is removed, and `query_interface` rejects null before adopting.
- Root experimental UI tests/check/fmt/strict Clippy, the standalone extension build, and the two focused SOM tests pass. Both Win7 release targets rebuilt after these latest changes; selected import/subsystem audits and x86/x64 Sciter smoke runs passed on the current Windows host.
- The public safe `From<HELEMENT>` conversion was replaced with `unsafe Element::from_raw` plus a private constructor for Sciter-returned handles. Binding doctests exposed three stale examples for the already removed `Value` indexing API; those examples now use `get`/`get_item`. With the existing local x64 evaluation DLL available to doctests, all 33 enabled vendor doctests pass (10 ignored); without that runtime, Sciter-dependent examples cannot execute.
- After the Element boundary change, both Win7 target builds, selected import/subsystem audits and x86/x64 hidden Sciter smoke were rerun successfully on this host. Still no Win7 VM run or complete binding safety claim.
- Also made `Host::attach`, `Host::attach_with`, `Window::attach`, and `Window::attach_intercepted` unsafe APIs with native HWND liveness/thread preconditions. Compile-fail doctests now preserve non-`Send`/non-`Sync` wrappers. Full vendor docs pass with local runtime (38 passed, 10 ignored); focused SOM tests pass (2). Root feature check/tests/fmt/strict Clippy pass; both Win7 builds, selected audits, and smoke pass again after the attachment API changes.
- Raw parent handles are now covered too: `Window::create` is unsafe when callers supply the optional HWND, and `Builder::with_parent` is unsafe with a liveness condition that must hold until the builder is consumed. The application preview supplies no parent and documents the local safety argument. Rechecked root feature gates, vendor doctests, both Win7 builds, import audits and Sciter smoke after this change; all passed on the current Windows host.
- The next source pass marked public DOM lookups/child-window attachment, raw UTF conversion, windowless message handling, and video interface lookup unsafe where callers provide external pointers. Event/request source parameters now take `&Element`. Vendor tests pass (17 unit tests, 38 doctests with runtime, 10 ignored); root feature checks/tests/fmt/strict Clippy and both Win7 builds/audits/smoke pass after these API changes. Further callback payload null/bounds/lifetime and function-pointer ABI review remains for A3.
- Remaining A3 work includes review of other raw pointer boundaries and function-pointer ABI assumptions, runtime licensing/redistribution evidence, and actual Win7 execution. This is still not a production-safety claim.

## A3 continuation: callback payload validation — 2026-10-27

- Image save/paint callbacks no longer assert on null payloads. Image buffer lengths are bounded by `isize::MAX`, zero-length buffers avoid constructing slices from null pointers, and unwinding panics from image callbacks are caught before returning across the C ABI; image save reports `BAD_PARAM` if callback validation fails. Windowless bitmap dimensions and rectangle coordinates use checked conversions/arithmetic; oversized slices are rejected and user callback panics are contained. Value enumeration returns `FALSE` for null callback data/key/value pointers. Under release `panic=abort`, a panic still aborts.
- Stable MSVC 1.97.1 root verification: `cargo test --locked --features experimental-ui` (40 library, 2 CLI tests), `cargo fmt --check`, and strict feature Clippy pass using isolated `CARGO_TARGET_DIR=target/a3-callback-stable`. Vendor tests with the local evaluation DLL pass: 17 unit tests and 38 doctests, 10 ignored. Both nightly Win7 release target builds and selected import/subsystem audits pass after these callback edits; both binaries pass hidden Sciter smoke (`Overview -> Tweaks`) on this Windows host. This is not execution on Windows 7.
- Remaining A3 work: review callback ownership and function-pointer ABI assumptions, resolve runtime licensing/redistribution evidence, and obtain actual Win7 VM evidence. The runtime is evaluation-only and not packaged. Then continue B1 durable storage work; A0 is the first stage, not an overall completion criterion.

## B1 continuation: per-transaction durable journal scan — 2026-10-27

- Added per-record commit marker files. `sync` now verifies and flushes the record, then exclusively creates and flushes the marker; `scan` validates contiguous marker/record sequences, exact transaction identity, size limits and CRCs, and reports one uncommitted tail. `RegistryJournal::resume` accepts only a valid contiguous prefix with no incomplete tail. Transaction IDs are now lowercase ASCII to avoid Windows filename case-fold collisions.
- Windows host tests cover exclusive write/readback, duplicate create refusal, validation and size bounds, per-transaction scan/resume, refusal of incomplete/out-of-order tails, and CRC-detected record corruption. These tests run on the D: NTFS volume; they are not crash/power-loss tests. Latest stable checks pass: 54 library tests, 2 CLI tests, feature check, formatting and strict feature Clippy. Both Win7 release targets, selected import/subsystem audits and latest x86/x64 smoke pass on this host.
- B1 is still incomplete: no protected root ACL, atomic root-wide discovery, end-to-end recovery orchestration with a separate persisted recovery backup journal, target-level locking across different transaction IDs, crash/power-loss tests, or proof of underlying storage-device behavior. Filename-based transaction discovery exists but can race with concurrent transactions. Do not wire this adapter into a live mutation path. Next: integrate journal conversion, checkpoint resume and a separate recovery snapshot journal; add fault-injection coverage for every interrupted write boundary. A3's Win7 VM and Sciter runtime licensing work is also open.

## B1 continuation: lock lifetime tied to journal writer — 2026-10-27

- Raw `FileJournalStorage` no longer implements `JournalStorage`. The only built-in filesystem implementation is `LockedJournalStorage`, which owns both the storage root and an exclusive Windows file handle for one lowercase transaction ID. `RegistryJournal` therefore cannot use the built-in Windows files without acquiring the lock first, and the lock remains alive through the writer's lifetime. Reopening uses a newly opened root and lock guard.
- A Windows child-process test verifies that a second process gets `WouldBlock` while the first owns the same lock; same-process exclusion and reacquisition after drop also pass. `discover_transactions` identifies transaction IDs from record, marker and lock filenames and fails closed on unknown/non-regular/reparse entries; enumeration is not atomic against concurrent writers. Latest host verification: 54 library tests and 2 CLI tests pass; feature check, formatting, strict feature Clippy, both Win7 release builds, selected import/subsystem audits, and x86/x64 Sciter smoke pass on this host. These are not Win7 VM or power-loss tests.
- Remaining gaps: root ACL security, atomic root-wide discovery, target-level locks shared across distinct transaction IDs, crash/power-loss evidence, and recovery/checkpoint integration. No mutation path should accept these snapshots until the transaction coordinator uses this locked store and the remaining recovery contracts are verified.

## B1 continuation: journal-to-recovery adapter — 2026-10-27

- Added `recovery_records_from_journal` in `src/engine/recovery.rs`. It converts records from a committed journal prefix into recovery operations while preserving target, exact prior state, and intended state. It rejects sequence gaps, mixed transaction IDs, unsupported versions, invalid IDs/targets, and other record-format failures. Empty input yields an empty plan.
- Tests prove state/target preservation and rejection of malformed prefixes. Stable feature check, full tests (56 library, 2 CLI), formatting, strict Clippy, both nightly Win7 release builds, and selected import/subsystem audits passed after the change. The new conversion does not itself verify commit markers or storage; only pass it records from `JournalStorage::scan`.
- Added `LockedJournalStorage::open_recovery_checkpoint`, which rescans the transaction and requires exact equality with the supplied committed records. `FileRecoveryCheckpoint` owns the transaction lock, creates an initial checkpoint before recovery can start, appends write-through checkpoint generations and separate commit markers, verifies readback/CRC, resumes from the latest committed progress, and skips a single incomplete tail without overwriting it. Progress may only move toward zero. Filename discovery accepts the checkpoint sidecars, while journal record scanning ignores them. `sync_all` and current-host tests do not establish directory-entry durability across power loss.
- Host NTFS tests cover progress across reopen, incomplete-tail continuation, exact journal binding, forward-progress rejection, and discovery with checkpoint sidecars. Latest verification after this implementation: 59 library and 2 CLI tests, feature check, format, strict Clippy, both nightly Win7 release builds and selected import/subsystem audits passed.
- Added `recover_journal_prefix` to compose journal-record validation, reverse recovery, per-step pre-restore persistence, and checkpoint progress from the checkpoint's loaded index. An NTFS integration test runs it against a mock registry backend while a separate `RegistryJournal<LockedJournalStorage>` writes the recovery snapshots; the test confirms reverse restoration, exact backup-before/desired states, and completed progress. Engine tests also prove invalid records and backup-store failure prevent restore writes.
- `Checkpoint` version 3 stores the separate recovery snapshot transaction ID. `FileJournalStorage::create_new_transaction` atomically reserves a new ID and refuses IDs with existing logs even if their lock file is missing. `FileRecoverySession::start` generates an ID; `start_with_id` accepts an explicit ID. Both persist it before recovery starts. `resume` reloads it and resumes the separate snapshot journal. `run` owns both transaction locks during recovery.
- Resume now handles an incomplete recovery snapshot tail by allocating a new ID and committing that identity in a checkpoint generation with unchanged progress before retrying the unfinished step. The incomplete old journal is retained for diagnosis. Checkpoint loading permits ID rotation only when recovery progress is identical; changing ID alongside progress is rejected.
- NTFS integration coverage injects an incomplete trailing snapshot record after an interrupted restore, resumes into a generated journal ID, verifies the old tail remains, and completes without repeating the already checkpointed restore. Stable verification: 68 library and 2 CLI tests, feature check, formatting, and strict Clippy pass. Both nightly Win7 release builds and selected import/subsystem audits also pass for this continuation.
- Added an internal test-only I/O fault injector at record/checkpoint create, write, flush, readback and marker-publication boundaries. Journal commit markers and checkpoint commit markers now write to `.commit.pending`, flush and verify those bytes, then rename to `.commit`; a failed flush therefore leaves a recognizable pending tail rather than a committed marker. Scanners validate pending sidecars against the corresponding incomplete record/checkpoint, and transaction discovery recognizes them. The recovery-state tests fault each journal boundary and all checkpoint boundaries, reopen the real NTFS-backed store, confirm the writer is poisoned, and ensure only the post-publication readback fault has a committed record/checkpoint (whose data and marker were already flushed). `cargo test` now reports 72 library and 2 CLI tests. This does not prove atomic directory metadata persistence after power loss; real device/filesystem crash evidence remains absent.
- Added `FileJournalStorage::lock_registry_mutations`, a root-wide exclusive guard deliberately coarser than one target so it serializes all registry writes, including case variants, view aliases and merged HKCR paths. Transaction discovery validates and ignores this well-known lock file. `FileRecoverySession` acquires it after its source transaction lock and retains it for the session, so recovery query/restore calls cannot race a different cooperating recovery session. `FileTransactionSession` owns a fresh transaction journal and wraps each `apply_one` with the barrier; its mock integration test verifies the record and final commit marker exist before backend apply, and verifies barrier contention prevents query and record creation. Same-process and child-process exclusion tests pass. Lock order is transaction journal lock first, then registry barrier. B2 must route its real backend through `FileTransactionSession::apply`; generic `apply_one` remains available for mocks and does not enforce locking independently. Stable test totals are now 76 library and 2 CLI tests.
- Remaining B1/B2 gaps: protected journal-root ACL, atomic discovery, integration of the shared barrier into every apply path, power-loss/storage-device evidence, and a production Win32 restore/apply backend. The barrier serializes globally and may limit throughput; keep mutations disabled until all write paths use it and the other safety contracts are closed.
- Still missing: target-level locking across transactions, protected root ACL, atomic discovery, power-loss evidence, and a production Win32 restore backend. Checkpoint generations repeat the full ordered record list and need bounded-history/performance analysis for large transactions. Keep live mutations disabled. Next: add fault injection at journal/checkpoint write, flush, marker and readback boundaries.

## B2 continuation handoff — guarded Windows registry prototype (2026-10-27)

Task / status: B2, in progress.
Scope and files: `src/engine/registry/windows.rs`, `src/data/file_store.rs`, `AGENT_START.md`, `AGENT_TASKS.md`, `docs/rollback-contract.md`.
Result: Added a lock-guarded Win32 value backend and session entrypoints. Existing key values can be queried, applied and restored with exact native type/data; absent values are restored by deletion. The backend fails closed for missing-key creation/restoration and unsupported registry views. It is not exposed through the application UI/CLI.
Checks: stable MSVC feature check and test (81 library, 2 CLI), fmt, strict feature Clippy; nightly Win7 x86/x64 release builds using `-Z build-std=std,panic_abort`; selected import/subsystem audits all passed. No live registry mutation executed.
Risks / gaps: no key creation/deletion; no service/filesystem writer; root ACL, atomic discovery and power-loss guarantees unverified; no Win7 VM evidence; mock coverage includes exact native type/bytes, absent-value undo, conflicts, failed backup, partial apply with persisted file journal, restore failure/retry and verification mismatch; direct Win32 API edge cases still need VM validation.
Backend-path audit: both `WindowsRegistryBackend::new` call sites are inside guarded `FileTransactionSession::apply_registry` and `FileRecoverySession::run_registry`; no other production call sites were found. Next: specify the registry-view/missing-key policy and generalize B1 persistence to typed service records before implementing any service writer. Follow `docs/service-mutation-contract.md`; keep the service path read-only until that schema and recovery integration exist.

## Typed service-record codec prototype — 2026-10-27

- `src/data/journal.rs` now defines `ServiceTarget` and version-1 `ServiceRecord` for an existing service's startup-type transition, with strict unknown-field rejection and bounded JSON encode/decode. The codec preserves the SCM numeric values without lossy enum mapping and validates transaction ID and service-name nonempty/NUL constraints.
- This codec is intentionally standalone. `JournalStorage::scan`, `RegistryJournal`, `Checkpoint`, `FileRecoverySession`, and `recover_journal_prefix` still accept registry records only. It does not persist a service record and must not authorize a service write.
- Focused codec tests pass. Next B1 work is a tagged transaction record representation that safely carries both resource kinds through contiguous scan validation, exact checkpoint binding, and reverse recovery while rejecting unknown or mixed unsupported versions. Only then can a mock SCM service transaction coordinator be connected.

## B1 typed journal continuation — 2026-10-27

Task / status: B1, in progress; service mutation remains blocked by recovery integration and shared durability gates.
Scope and files: `src/data/journal.rs`, `src/data/store.rs`, `src/data/file_store.rs`, `docs/service-mutation-contract.md`.
Result: Tagged records flow through ordered file scanning; `TypedJournal` appends and resumes mixed resource snapshots. Checkpoint v4 binds typed records and reads v3 registry checkpoints. `FileTypedTransactionSession` enforces shared-barrier query → committed typed snapshot → apply ordering. `FileTypedRecoverySession` owns source/checkpoint/snapshot locks and the shared barrier, dispatches typed recovery, and rotates after an incomplete snapshot tail while preserving the old tail. NTFS-backed tests verify commit-before-apply/restore, partial-apply snapshot retention, barrier retention, checkpoint resume and tail rotation. No Win32 SCM backend or application mutation entrypoint was added.
Checks: rustup stable MSVC 1.97.1; `cargo check --locked --features experimental-ui`; `cargo test --locked --features experimental-ui` (93 library, 2 CLI); `cargo fmt --check`; `cargo clippy --locked --all-targets --features experimental-ui -- -D warnings`; nightly Rust 1.101.0 `cargo build --locked --target i686-win7-windows-msvc -Z build-std=std,panic_abort --release --features experimental-ui`; same for `x86_64-win7-windows-msvc`. All passed. The local Sciter vendor emits six existing warnings in ordinary check/build output; project Clippy passes with `-D warnings`.
Risks / gaps: Production service query/restore APIs and a Win32 typed apply backend are not implemented. Journal-root ACL, atomic discovery and power-loss behavior remain unverified. Win7 VM testing was not run. Source files remain untracked on `main`; do not stage or commit wholesale.
Next: Implement service query/restore APIs behind the typed managed sessions with mock SCM coverage for access errors, conflicts, partial failures and retry. Keep application mutation entrypoints disabled pending shared persistence gates.

## B2 continuation handoff — typed Win32 SCM backend (2026-10-27)

Task / status: B2, in progress; service writer prototype compiles, not approved for application use.
Scope and files: `src/engine/services.rs`, `src/data/file_store.rs`, `docs/service-mutation-contract.md`, `AGENT_START.md`, `AGENT_TASKS.md`.
Result: Added `WindowsTypedBackend` dispatching typed registry and service resources. Service operations query the numeric startup type, use `ChangeServiceConfigW` with `SERVICE_NO_CHANGE`/null for all other fields, then re-query and verify. Typed apply/recovery session methods retain the typed journal/checkpoint locks and shared mutation barrier. Added invalid mutation-name tests. No UI/CLI call site was found; no service setter was executed. Code review found no fixture abstraction for mock SCM tests; docs now state the missing coverage and the race with external service configuration changes between snapshot query and write.
Checks: rustup stable MSVC 1.97.1 `cargo check --locked --features experimental-ui`; `cargo test --locked --features experimental-ui` (93 library, 2 CLI); `cargo fmt --check`; `cargo clippy --locked --all-targets --features experimental-ui -- -D warnings`; nightly Win7 x86 and x64 release builds with `-Z build-std=std,panic_abort`. Passed. The initial host check failed because the trait was referenced from `engine::registry` instead of `engine::recovery`; corrected and reran all checks. Six existing vendor Sciter warnings remain in ordinary build output. No PE import audit or Win7 VM execution was done for this continuation.
Risks / gaps: The shared file barrier only coordinates this application's sessions; external SCM clients can race the snapshot/apply gap. The mock SCM test matrix, journal-root ACL, atomic transaction discovery, power-loss validation, Win7 VM validation and release gating remain open. Do not expose service mutation through UI/CLI or claim production safety.
Next: Introduce a narrow injectable SCM adapter and mock the query/change/re-query paths. Add race/conflict detection before apply, and test snapshot failure, API failure after partial change, restore retries and mismatch. Then address the shared durability gates before any application entrypoint.

## B2 continuation handoff — mockable SCM transaction and recovery (2026-10-27)

Task / status: B2, in progress; mock transaction/recovery path added, live service writes remain unapproved.
Scope and files: `src/engine/services.rs`, `docs/service-mutation-contract.md`, `AGENT_START.md`, `AGENT_TASKS.md`.
Result: Added the private `ScmApi` boundary and a reusable typed service backend. It binds each apply/restore to the observed target and state, re-queries after the durable snapshot, and refuses the write if a concurrent change is observed. Mock tests cover successful apply+verification, query and snapshot failures preventing write, partial change failure preserving pre-state, conflict before apply and restore, recovery snapshot failure, restore retry, and verification mismatch. NTFS-backed typed apply/recovery tests confirm the committed marker is visible before mock SCM writes, a failed restore retains its recovery snapshot, recovery resumes successfully after reopen, and the 21-boundary recovery fault matrix survives a second reopen after retry. The file barrier cannot prevent a change in the last query-to-SCM-call interval; that limit is explicit in the service contract.
Checks: Targeted `cargo test --locked --features experimental-ui services::tests` passes (16 service tests). Stable MSVC check, full tests (106 library, 2 CLI), fmt, strict Clippy and nightly Win7 x86/x64 release builds pass with `experimental-ui`. Six existing vendor Sciter warnings remain in ordinary check/build output. No PE import audit or Win7 VM execution was done for this continuation.
Risks / gaps: Mock adapter does not validate Win32 access-denied codes or actual SCM behavior; the adapter fault matrix now covers every recovery record/checkpoint I/O boundary, including a second reopen after retry. Microsoft docs confirm `LockServiceDatabase` has no effect since Vista and does not block configuration changes on older systems, so no supported-target SCM lock closes the final query-to-write race. Root ACL, atomic discovery and power-loss evidence remain open. No real service mutation was performed.
Next: Continue the root ACL, atomic discovery and power-loss release gates, then Win7 VM/real SCM validation. Keep UI/CLI actions disabled.

## Independent verification and ACL investigation — 2026-10-27

Task / status: B2 verification complete; shared ACL gate remains in progress.
Scope and files: Audited current `src/data/file_store.rs`, `src/engine/services.rs`, existing handoff/docs, and verified host/UI/Win7 builds. Added `docs/journal-root-security.md`; linked it from `AGENT_START.md` and this queue. Existing project files are untracked on `main`; preserve them and do not stage/commit wholesale.
Result: Confirmed the 21-boundary typed recovery fault matrix exists and passes; checkpoint sequence validation contains the retry fix. Confirmed typed SCM apply/recovery sessions and mocks exist, while the real setter has not been run and UI/CLI do not expose the calls. ACL remains unimplemented: current open checks only final path type/reparse and canonicalizes it. A one-time leaf DACL inspection cannot establish ancestor protection or stop later path replacement, so documented creation, inheritance, parent, lifetime binding and disposable test requirements instead of overstating a leaf check.
Checks: Stable MSVC 1.97.1 isolated target `target/b2-registry-stable`: check, full test (106 library + 2 CLI + doc tests), fmt and strict Clippy passed. `npm --prefix ui run build` passed. Nightly 1.101.0 x86/x64 Win7 release builds passed using matching `RUSTC`/`RUSTDOC`; selected import/subsystem audit script passed on both outputs. Six pre-existing vendor Sciter warnings remain. No Win7 VM, full PE audit, live SCM/registry write, or ACL mutation was performed.
Risks / gaps: Root ACL/path binding, atomic discovery, power-loss guarantees, the final external SCM actor race, actual SCM boundary behavior, and Win7 VM coverage remain open. Mixed toolchain attempts initially failed due Chocolatey cargo/rustc/rustdoc resolution; isolated matching-toolchain reruns passed.
Next: Implement or formally bound Windows 7-compatible secure journal-root creation and handle-anchored path operations against `docs/journal-root-security.md`; maintain disabled system mutation entrypoints until the ACL, discovery and power-loss gates close.

## B1 continuation handoff — cooperative atomic transaction-ID discovery (2026-10-27)

Task / status: B1, in progress; transaction-ID enumeration is serialized against cooperating journal API creation.
Scope and files: `src/data/file_store.rs`, `docs/rollback-contract.md`, `docs/journal-root-security.md`, `AGENT_START.md`, `AGENT_TASKS.md`.
Result: Added `.transaction-discovery.lock`. `discover_transactions` holds it throughout enumeration; `lock_transaction` and `create_new_transaction` hold it while opening or creating the per-transaction lock file that introduces an ID. The discovery lock itself is excluded from the scan. This closes the race where a cooperating API call introduced a transaction ID while discovery enumerated names. It does not provide an atomic snapshot against direct external filesystem writers and does not address ACL/path replacement. Both commit-marker publication sites now call `MoveFileExW(MOVEFILE_WRITE_THROUGH)` after the marker contents are flushed and read back; this is a durability improvement, not evidence of crash/power-loss behavior.
Checks: Stable MSVC check, full test (108 library + 2 CLI; doctests pass), fmt, strict Clippy and both nightly Win7 target release builds passed. The selected import/subsystem audit passed for both built binaries. A cross-process probe confirms a new ID cannot be introduced while discovery holds its lock; the discovery test and existing registry-mutation barrier test pass.
Risks / gaps: Root ACL/ancestor `FILE_DELETE_CHILD`, direct external writers, power-loss durability, actual SCM boundary behavior, external SCM races, and Win7 VM coverage remain open. An experiment showed a leaf directory handle without `FILE_SHARE_DELETE` still does not block rename via parent `FILE_DELETE_CHILD`; the ineffective handle-only patch was removed.
Next: Implement and test the secure root/ancestor ACL policy and creation path; keep all system mutation entrypoints disabled until root namespace protection and power-loss gates are satisfied.

## B1 continuation handoff — Windows journal-root ACL implementation (2026-10-28)

Task / status: B1, in progress; ACL creation and validation implemented on the Windows host. Release gate remains open.
Scope and files: `src/data/windows_security.rs` (new), `src/data/file_store.rs`, `src/data/mod.rs`, `src/engine/services.rs` test root, `Cargo.toml`, `docs/journal-root-security.md`.
Result: `FileJournalStorage::create_private` creates only a new local-drive leaf, applies a protected DACL, validates owner/DACL and checks every ancestor for reparse points and unsafe allow ACEs. `open` now performs the same checks and retains RAII handles for path components. Existing ACLs are not repaired. Test roots use this creation API. During verification, an initial inherited-ACE interpretation falsely rejected normal Windows paths; this was corrected by ignoring `INHERIT_ONLY_ACE` for the current directory. Review also caught that a draft `FILE_ALL_ACCESS` OWNER RIGHTS ACE would grant `WRITE_DAC`; replaced it with separate narrow directory/file masks. The file mask includes `FILE_APPEND_DATA`, needed by Windows write opens. These are verified by host execution of marker publication and reopen tests.
Checks: Rustup stable MSVC 1.97.1 with isolated `CARGO_TARGET_DIR=target/a0-acl-check`: latest independent rerun of `cargo check --locked --features experimental-ui`, `cargo test --locked --features experimental-ui` (118 library + 2 CLI; doctests pass), `cargo fmt --check`, and `cargo clippy --locked --all-targets --features experimental-ui -- -D warnings` passes. Nightly 1.101.0 x86 and x64 Win7 release builds pass with `-Z build-std=std,panic_abort --release --features experimental-ui`; `scripts/audit-win7-imports.ps1` selected forbidden-import/dynamic-CRT/subsystem checks pass for both using VS 2022 `dumpbin` 14.44.35207. Six existing vendor Sciter warnings remain. No Win7 VM was run.
- Risks / gaps: I verified the unfinished prior ACL draft against source and runtime rather than assuming it was complete. It was not wired into `FileJournalStorage`; once wired, initial full-suite runs exposed the inherited-ACE parsing bug and insufficient/misapplied OWNER RIGHTS masks. Those defects are fixed, and the complete Windows host suite now passes. Reparse rejection was verified read-only using this host's pre-existing `Application Data` junction; an unsafe disposable parent DACL was rejected; post-create ACL validation failure cleanup removed its test leaf. Anonymous and restricted `BUILTIN\Guests` tokens cannot rename the root while storage is open. A same-account trusted profile owner can rename it through the profile parent's `FILE_DELETE_CHILD`, so open handles are not an absolute path pin for that trusted principal. `Get-LocalUser` lists other local accounts, but no logon credentials/token for a separate named account was available; no such account was accessed. Creating a new symlink returned Windows error 1314, so no new link was created. Separate named-account access, privileged/external filesystem writers, power-loss behavior and Win7 VM validation remain unverified. Keep system-changing application entrypoints disabled. The UI/CLI still do not expose the mutation sessions.
- Next: Verify behavior with a separate named account if available; investigate a stronger handle-relative I/O design or retain the explicit same-account limitation. Keep system actions disabled until namespace, power-loss and supported-OS evidence is reviewed.

## B1 continuation — abrupt process termination recovery probe (2026-10-28)

Task / status: B1, in progress; a process-crash probe supplements error-injection coverage. Power-loss durability remains unverified.
Scope: `src/data/file_store.rs`, `docs/rollback-contract.md`, `AGENT_START.md`, `AGENT_TASKS.md`.
Result: Added child-process termination tests at record flush, before commit-marker publication, and after publication but before final readback. The parent process reopens the journal after each child exits without Rust destructors and confirms that pre-publication states remain an incomplete tail while an already published marker is treated as committed. This exercises process exit and OS handle cleanup only; it does not model cache loss, sudden power failure, filesystem metadata durability, or storage-device behavior.
Checks: Stable MSVC 1.97.1 `cargo check --locked --features experimental-ui`; full `cargo test --locked --features experimental-ui` (118 library + 2 CLI; doctests pass); `cargo fmt --check`; strict all-target Clippy all pass. Six existing Sciter vendor warnings remain.
Next: Keep system-changing entrypoints disabled. Define and execute an isolated VM/storage crash plan for the supported filesystems before making a power-loss durability claim; continue keeping the Win7 VM and separately named-account ACL checks distinct from host evidence.

## B1 continuation — fail-closed journal volume policy (2026-10-28)

Task / status: B1, in progress; unsupported journal volumes are now rejected before root creation/open. Crash and power-loss release gates remain open.
Scope: `src/data/windows_security.rs`, `Cargo.toml`, `docs/journal-root-security.md`, `docs/rollback-contract.md`, `docs/power-loss-test-plan.md`, `AGENT_START.md`, `AGENT_TASKS.md`.
Result: Root creation/open now query the containing volume and require `DRIVE_FIXED`, the `NTFS` filesystem name, and `FILE_PERSISTENT_ACLS`. Added policy tests rejecting removable and remote drive types, exFAT, and a filesystem without persistent ACLs. The workspace `D:` volume is reported fixed NTFS; all FileJournalStorage host tests ran there. The API contracts place GetVolumePathNameW/GetDriveTypeW/GetVolumeInformationW support before Windows 7. Added a staged test plan that separates guest hard-off recovery from true storage-cache/power-loss evidence. Host environment has Hyper-V cmdlets but `Get-VM` is denied by authorization policy; VBoxManage, vmrun, QEMU binaries were not found in PATH or checked standard install locations; optional-feature query requires elevation. No VM or power interruption was run.
Checks: Rustup stable MSVC 1.97.1 `cargo check --locked --features experimental-ui`; full `cargo test --locked --features experimental-ui` (119 library + 2 CLI; doctests pass); `cargo fmt --check`; strict all-target Clippy all pass. Nightly 1.101.0 x86/x64 Win7 release builds pass; selected forbidden-import/dynamic-CRT/subsystem audits pass on both. Six existing Sciter vendor warnings remain.
Next: Keep UI/CLI mutation entrypoints disabled. Resolve safe access to a disposable Win7 VM and execute the guest crash matrix; separately arrange a documented storage-cache interruption rig before claiming power-loss durability. Continue reviewing ancestor/path replacement bounds and named-account ACL behavior.

## B1 continuation — external-controller journal cut probe (2026-10-28)

Task / status: B1, in progress; a test-only probe is ready for record/commit-marker VM crash testing. No VM hard-off or storage power-loss test has been run.
Scope: `src/data/file_store.rs`, `docs/power-loss-test-plan.md`, `AGENT_START.md`, `AGENT_TASKS.md`.
Result: The exact child-process crash test covers 31 combinations: 11 record/commit-marker cuts, 10 initial checkpoint/marker cuts, and 10 recovery snapshot-ID rotation cuts. On the selected occurrence the writer signals the boundary over TCP and parks with journal handles open. The host parent kills the child and starts separate `verify` mode, which checks raw artifact state and recovery ID/progress after reopening. The external-controller `prepare`, `arm`, and `verify` modes touch only the explicitly supplied private journal root and do not mutate registry/SCM state. `docs/power-loss-test-plan.md` documents all modes and guest commands. This is host process-termination evidence; VM hard-off and physical storage-cache loss remain untested.
Checks: Stable MSVC 1.97.1 `cargo check --locked --features experimental-ui`; full `cargo test --locked --features experimental-ui` (120 library + 2 CLI; doctests pass); `cargo fmt --all -- --check`; and strict all-target Clippy all pass. The targeted 31-case kill/reopen/raw-verify test passes. Six existing Sciter vendor warnings remain. Win7 x86/x64 release builds and selected import/subsystem audits passed after the production volume-policy change; the current delta is test/docs only.
Next: Run the guest probe on disposable Win7 x86/x64 NTFS VMs, including rotation cases, then add equivalent coverage for typed checkpoints. Keep this evidence distinct from actual storage-cache power loss; keep system-changing entrypoints disabled.

## A0 continuation — agent instructions and host CI alignment (2026-10-28)

Task / status: A0 agent-readiness corrections implemented locally; not a project completion claim. Remote CI has not run.
Scope: `.github/workflows/rust.yml`, `AGENTS.md`, `AGENT_START.md`, this handoff.
Result: Host CI now installs the verified MSVC Rust 1.97.1 toolchain and runs locked checks, tests, format and strict Clippy for default and `experimental-ui` configurations. `AGENTS.md` now maps actual directories and warns against inventing `src/core/`, `src/cli/`, `src/engine/tweaks.rs`, a Rust UI bridge, or root `config/tweaks.json`; it clarifies that Preact is preview-only and system actions need a persisted transaction contract. A conflicting raw-handle threading rule was corrected: integer casts do not prove ownership; cross-thread use requires a reviewed RAII `Send` contract. This corrects agent directions that previously pointed at nonexistent files or could encourage unsafe handle transfer. The Win7 target workflow remains separate and pinned to nightly 2026-10-27.
Checks: Local stable MSVC 1.97.1 A0 matrix rerun with isolated `CARGO_TARGET_DIR=target/a0-ci-local-final`: locked default and `experimental-ui` checks; full locked tests (125 library + 2 CLI default, 127 library + 2 CLI with feature); format check; and strict all-target Clippy for both configurations all pass. Six existing Sciter vendor warnings appear only in feature builds. `npm --prefix ui run build` passes with local Node 26.5.0/npm 11.17.0, and the tracked `ui/dist` assets remain unchanged; the workflow's Node 22 + `npm ci` path is not locally replicated. Origin `HEAD` and `main` both resolve to `c10b3bd943ebbe471bc41ea2c5e57c24b8a3766f`, which is the local initial commit; `gh run list --repo FeoreV/tweaker` could not query Actions because GitHub CLI is not authenticated.
Risks / gaps: Commit `f0256a68e7b8fc2358f302212319c284273f7d9b` establishes the 96-file project baseline locally on `main`, excluding the pre-existing `.agents/memory/*` edits. Fresh worktrees from local `main` now include the project; worktrees created from `origin/main` will not until this commit is pushed. Remote CI result is unavailable because the branch has not been pushed and GitHub CLI is not authenticated.
Next: When publishing is authorized and GitHub authentication is available, push the reviewed baseline and confirm the workflow on GitHub Actions. Continue the remaining A1-A3/B gates with disjoint file ownership. A0 remains the first stage, not the project end state.

## A2 continuation — rollback contract synchronized to implementation (2026-10-28)

Task / status: A2 contract correction implemented; release gates remain open.
Scope: `docs/rollback-contract.md`.
Result: Corrected two stale assertions that could mislead follow-on agents: checkpoint identity now reflects the current v4 typed format (with v3 registry-only compatibility), and the service startup-type writer/recovery prototype is described as implemented but not approved or exercised against live SCM. The contract now distinguishes implemented registry behavior from the unresolved external-change conflict race and records the actual 31-cut process-termination probe. Filesystem mutation remains unsupported; service and registry app entrypoints remain disabled.
Checks: Source cross-check against `src/data/checkpoint.rs`, managed session code in `src/data/file_store.rs`, and `src/engine/services.rs`; documentation-only update, no runtime behavior changed.
Risks / gaps: The service setter has not been run against SCM; no Win7 VM or power-loss test has been run. The documentation is still a design/release gate, not authorization to enable application mutations.
Next: Keep A2 acceptance gaps explicit and use `docs/service-mutation-contract.md` for typed SCM work; continue with the next unverified A1/A3 release-gate item after reconciling the provenance/compatibility evidence.

## A1 continuation — inventory claims audited against frozen source (2026-10-28)

Task / status: Initial catalog count and discrepancy audit verified; full per-entry provenance and compatibility audit remains open.
Scope: `docs/source-inventory.md`, `TWEAKS_INVENTORY.md`; read-only inputs under `winutil-26.08.19/`.
Result: Re-ran the documented Python parse/count method against frozen `tweaks.json` and `applications.json`. Counts match: 66 tweak keys, 50 registry, 2 service, 25 invoke-script, 19 undo-script, 29 Type, 2 ComboItems; 232 apps, 232 winget, 230 choco. Added `scripts/audit-source-inventory.py` to reproduce counts, hashes, and draft ID reconciliation. The 66-row draft inventory has only 42 exact ID matches: 24 draft IDs are absent upstream and 24 upstream IDs are absent from the draft; no alias/rename mapping is established. Rechecked `WPFTweaksActivity` and `WPFTweaksHiber`: Activity has three `<RemoveEntry>` originals; Hiber has two registry entries plus `powercfg.exe /hibernate off` and `/on` scripts. Recorded SHA-256 hashes for source files and license. The old inventory called itself authoritative and asserted unverified compatibility/port totals and runtime behavior; it is now explicitly a provisional draft, and those estimates are not evidence or implementation behavior.
Checks: `python scripts/audit-source-inventory.py` passed and reproduced the counts, hashes, exact ID overlap, mismatch sets, and sample records. No files under frozen upstream were modified.
Risks / gaps: Full 66-entry field/action-by-action reconciliation, explanation of the 24 ID divergences, package availability, licensing beyond the upstream MIT file, and per-OS compatibility are not verified. Do not dispatch B4 to implement from the draft inventory.
Next: Complete A1 traceable per-entry audit before B4; continue A3 primary-source feasibility review and keep runtime-dependent claims marked unverified.

## A1 continuation — upstream dispatcher semantics traced (2026-10-28)

Task / status: Handler-level spot check completed; full per-entry provenance and compatibility audit remains open.
Scope: Read-only `winutil-26.08.19/functions/private/Invoke-WinUtilTweaks.ps1`, `Set-WinUtilRegistry.ps1`, and `pester/tweaks.Tests.ps1`; documentation update in `docs/source-inventory.md`.
Result: Traced apply/undo field selection and confirmed catalog `OriginalValue`/`OriginalType` are directly used as configured restoration inputs, not pre-mutation snapshots. The generic registry helper can create a missing key before writing/removing a value and logs caught errors; `<RemoveEntry>` removes the named value. Ordinary dispatch filters out registry entries with a `Values` member. Service apply conditionally preserves non-default startup types, while undo directly uses `OriginalType`; the not-found catch warns but leaves the setter path enabled. Script strings are executed as PowerShell scriptblocks through a helper, and AppX removal is apply-only in this dispatcher. Pester tests mock the effectful helpers and establish argument routing only, not OS mutation, atomicity, or rollback correctness. Recorded these limits in the source audit; no upstream files were changed.
Checks: Re-run `python scripts/audit-source-inventory.py`; compare output to recorded counts, hashes and ID discrepancy; inspect `git status --short -- winutil-26.08.19` to confirm the frozen directory remains untouched.
Risks / gaps: This is a dispatcher spot check, not the promised 66-entry field/action audit. Script effects, nested helper behavior, package availability, per-entry OS compatibility, aliases for the 24 ID differences and true rollback behavior remain open. Do not use the draft as B4 implementation input.
Next: Continue the field-by-field reconciliation of the 42 matching IDs and explicitly resolve the 24 source/draft-only IDs before any catalog implementation.

## A1 continuation — reproducible per-entry source fact extract (2026-10-28)

Task / status: Deterministic source fact extraction added; interpretation/provenance review remains in progress.
Scope: `scripts/audit-source-inventory.py`, `docs/upstream-tweaks-facts.json`, `docs/source-inventory.md`.
Result: Added optional `--write-facts` mode. It extracts all 66 upstream tweak IDs and their raw registry/service/AppX/ComboItems declarations, labels, extra field names, and hashes/UTF-8 lengths of each apply/undo script without copying executable script bodies. The JSON marks itself as source facts only, with no compatibility/safety/reversibility/implementation approval implied. This gives follow-on agents a reproducible per-entry evidence baseline and makes upstream changes visible by diff.
Checks: Ran `python scripts/audit-source-inventory.py --write-facts` twice; both runs produced identical SHA-256 `2DCDBB30FECF7228F5C51309E6225D463B828E90C406E5492CE123FD6D18762C` for the facts JSON. Parsed generated JSON and confirmed 66 entries and 44 script records. The audit reproduced upstream catalog hashes/counts and 42 exact inventory ID matches. `git diff --check` passed; `git status --short -- winutil-26.08.19` was empty.
Risks / gaps: This is a structured extraction, not semantic validation of registry values, scripts, OS support, package availability, license scope, or rollback. The inventory's 24/24 ID mismatch remains unresolved. Keep B4 blocked on A1 acceptance.
Next: Use the fact extract to perform traceable review of matching entries, starting with any catalog claims currently marked Win7-compatible or “undo yes”; separately establish evidence for any proposed aliases.

## A1 continuation — exact-ID draft/source row comparison (2026-10-28)

Task / status: Machine-readable side-by-side claim comparison added; human semantic audit remains in progress.
Scope: `scripts/audit-source-inventory.py`, `docs/inventory-source-comparison.json`, `docs/source-inventory.md`.
Result: The existing `--write-facts` run now also extracts draft table fields and joins them to upstream entries only by exact ID. The comparison covers the 90-ID union: 42 exact matches, 24 source-only, and 24 draft-only. For matched entries it puts draft labels/operation/apply/undo/compatibility claims next to upstream display label/category and declared counts for registry/service/scripts/AppX. No alias is inferred and prose is not treated as a validated operation. Verified sample rows demonstrate discrepancies: `WPFTweaksHiber` upstream has 2 registry records + 1 apply + 1 undo script, while the draft operation text is simply captured verbatim; `WPFTweaksTelemetry` upstream declares 12 registry entries + apply/undo scripts, so its draft summary is not a full source trace.
Checks: `python scripts/audit-source-inventory.py --write-facts` passed; generated JSON parsed successfully, had 90 union entries and 42 exact matches, and all exact matches had draft row data. Current generated hashes: source facts `2DCDBB30FECF7228F5C51309E6225D463B828E90C406E5492CE123FD6D18762C`; row comparison `4384C11499F41AD58F51FCAEB065A7B0E7B53194B27CB94B745B6C4282E259EB`. Frozen source status remains empty.
Risks / gaps: Text extraction is a mechanical Markdown-table read, not semantic reconciliation; source-only/draft-only names are unresolved, and compatibility/undo claims remain unverified. A1 acceptance still requires source trace and action review; B4 must not consume the draft as implementation input.
Next: Review exact matches with claimed Win7 compatibility or undo support against their raw declarations and actual upstream action handlers; independently resolve the 48 unmatched IDs.

## A1 continuation — selected ID divergence candidates reviewed (2026-10-28)

Task / status: Ten high-similarity pairs manually traced; full divergence reconciliation remains open.
Scope: Read-only catalog/handler review in `winutil-26.08.19/`; findings in `docs/id-divergence-review.md`, linked from `docs/source-inventory.md`.
Result: Confirmed feature-level correspondences for the background-app registry tweak (with a draft-only DeleteValue undo claim), mouse acceleration (same three DWord values), Ultimate Performance enable (but no upstream Win7 fallback), and MPO (draft covers only one of two upstream registry records). Rejected equivalence for Ultimate Performance reset (upstream `powercfg /restoredefaultschemes`, draft activates Balanced), widgets (AppX removal vs taskbar button), unsigned RDP warnings (different values/semantics), Home/Gallery (three per-user registry entries vs split key-delete claims), and OneDrive (upstream script removes data, adjusts ACLs and service in addition to uninstall; draft claims a different registry action and incomplete undo). Device Metadata remains unresolved because the draft's path differs and upstream Content describes Device Companion Apps. Similar IDs are not accepted as aliases automatically.
Checks: Cross-checked generated exact-ID comparison and raw frozen catalog. Traced button dispatch and `Invoke-WPFUltimatePerformance.ps1`; read OneDrive and Widgets action strings without executing them. Frozen source remained untouched.
Risks / gaps: This selection is evidence-driven but not exhaustive. Remaining unmatched IDs, all per-entry OS claims, script helper side effects, and rollback correctness remain unresolved. Do not implement B4 from the draft.
Next: Continue candidates, then perform per-entry compatibility/undo review for the exact-ID matches before treating A1 as accepted.

## A1 continuation — compatibility and undo claim evidence audit (2026-10-28)

Task / status: Aggregate claim audit recorded; per-entry OS/action validation remains open.
Scope: Exact-ID rows from `docs/inventory-source-comparison.json`, recursive field review of frozen `tweaks.json`, and `docs/inventory-claim-audit.md`.
Result: Of 42 exact-ID draft rows, 25 claim compatibility (20 plain plus 5 decorated), 11 claim Win10+, and 6 claim Win11-only. The upstream objects have no target OS/version/build support fields; the one `Disabled (Compatibility)` combo label describes a UI option, not an OS matrix. Thus none of those compatibility claims is verified from the catalog. 39 exact-ID rows claim undo support and 3 say no. Source mechanisms vary across fixed registry `OriginalValue`, service `OriginalType`, and executable `UndoScript`; none is itself a captured pre-mutation state. Registry entries with a `Values` member bypass the ordinary setter loop. Report now states what this aggregate evidence proves and the remaining per-entry work.
Checks: Recomputed claim counts from the exact-ID comparison JSON (42 total; 25/11/6 compatibility groups; 39/3 undo claims). Recursively inspected source keys using specific OS/version/build metadata names; only the UI label `Disabled (Compatibility)` matched. No upstream files changed.
Risks / gaps: This does not evaluate actual Win7 behavior, every script/helper, or restore symmetry. The count audit is not a substitute for manual or VM evidence. Keep all compatibility and undo claims unverified and B4 unstarted.
Next: Trace each exact-match operation's source script/helper and authoritative OS constraints; update row evidence only when behavior is established.

## A1 continuation — upstream O&O launch implementation traced (2026-10-28)

Task / status: One additional source/draft divergence candidate traced; full A1 remains in progress.
Scope: Read-only `Invoke-WPFButton.ps1`, `Invoke-WPFOOSU.ps1`, and `Save-WinUtilFile.ps1`; one row added to `docs/id-divergence-review.md`.
Result: Confirmed source-only `WPFOOSUbutton` corresponds to draft-only `WPFTweaksOO` at the feature level: source dispatches to `Invoke-WPFOOSU`, which downloads `OOSU10.exe` and launches it. The downloader is .NET `System.Net.WebRequest` rather than WinHTTP as the draft claims. Recorded this discrepancy and left binary integrity, current availability, licensing, and OS compatibility unverified.
Checks: Read the exact dispatcher/helper source without running it or performing a network download; added whitespace check for modified text.
Risks / gaps: No downloaded executable was inspected or launched. This is only one more candidate resolution; the remaining ID divergences and per-entry platform claims remain open.
Next: Continue tracing the remaining source-only feature candidates and exact-match custom actions; preserve source-only cases as omitted until evidence supports a mapping.

## A1 continuation — all upstream custom action scripts reviewed (2026-10-28)

Task / status: All catalog entries containing apply/undo scripts have been read and risk-reviewed; A1 overall remains in progress.
Scope: Read-only review of all 25 `InvokeScript` entries and 19 `UndoScript` entries in frozen `config/tweaks.json`; `docs/custom-action-audit.md` added and linked from `docs/source-inventory.md`.
Result: Recorded each custom action and its undo gap. Six entries have no `UndoScript` (Widgets, Services, Restore Point, Windows AI, Disk Cleanup, Delete Temp Files). Major findings include CIM-based memory discovery in Services; AppX/profile/data/file removals; DISM `/ResetBase`; broad/fixed ACL changes; the Explorer discovery undo repeating deletion of Bags/BagMRU; fixed-state rather than captured-state inverses for hibernation, services, Teredo and IPv6; and scripts that only refresh Explorer in both directions. None was executed. Report explicitly requires direct Win32 replacement for the CIM query, transaction snapshots for retained operations, and omission/separate safety contract for destructive actions.
Checks: Read all script bodies from source JSON and cross-checked helper dispatch semantics already recorded. Source facts still show 25 apply scripts and 19 undo scripts. No runtime, network, registry, service or filesystem action was performed; frozen directory remains read-only.
Risks / gaps: Reading scripts establishes declared code paths but not runtime outcomes, helper behavior for every called function, Windows-version compatibility, network content/integrity, or full restoration semantics. A1's exact-ID semantic audit and OS claims remain incomplete; B4 must remain unstarted.
Next: Trace declarative service/registry entries and custom helper calls for matched candidates, then record remaining omitted/source-only IDs and decide whether A1 acceptance is met.

## A1 continuation — custom action findings cross-checked against all IDs (2026-10-28)

Task / status: The custom-action report now covers every source entry with either script field; A1 as a whole is still in progress.
Scope: `docs/custom-action-audit.md` and source catalog `InvokeScript`/`UndoScript` declarations.
Result: Added exact undo-scope distinctions for six entries without `UndoScript`: some still have generic declarative registry/service handling (e.g. Services/Restore Point/Windows AI), while others have no declared generic reversible resource. The report now enumerates all script-bearing IDs rather than implying that absence of a custom undo means the entire tweak has no declarative undo.
Checks: Compared the 25 table IDs directly against all frozen source entries with a nonempty `InvokeScript` or `UndoScript`; exact set equality passed. Independently counted six entries without `UndoScript`. Documentation whitespace check passed; frozen source directory remains unchanged.
Risks / gaps: Source declarations and script text are still not runtime proof. The generic dispatcher can route catalog originals but does not create actual pre-state snapshots; helper internals and OS constraints need more trace.
Next: Continue source-level action tracing for declarative entries and helper effects, then decide whether A1 provides adequate provenance for a separately designed B4 catalog subset.

## A1 continuation — registry/service/combo/DNS dispatch traced (2026-10-28)

Task / status: Additional declarative/custom UI paths traced; A1 overall remains open.
Scope: Read-only `Set-WinUtilRegistry.ps1`, `Set-WinUtilService.ps1`, registry combo read/state helpers, `Invoke-WPFUIElements.ps1`, `Invoke-WPFtweaksbutton.ps1`, `Set-WinUtilDNS.ps1`, and frozen DNS catalog; findings in `docs/declarative-handler-audit.md`.
Result: Confirmed ordinary registry writes skip `Values` records and swallow/log failures; service apply uses a fixed `OriginalType` comparison guard, while undo writes that fixed type. The MPO combo path does an in-memory exists/data snapshot and verifies rollback after failure, but does not capture native type or survive process loss. The `WPFchangedns` selection runs before tweak iteration, mutates all up adapters (DNS, optional DoH registrations and registry state), and can return failure after partial application without restoring earlier adapters. Generic helper success logging is not transactional evidence.
Checks: Cross-checked dispatcher branches and helper source; verified the only `Values`-mapped registry tweak is MPO and the two `ComboItems` entries are MPO and DNS. No handlers were executed and no local/upstream Windows state was changed.
Risks / gaps: No real Windows behavior or OS support tested; helpers may partially mutate despite errors; no persisted snapshots exist in these upstream paths. Keep DNS and combo features out of implementation until adapted to rustweaker's typed durable transaction contract.
Next: Finish per-entry declarative review and provenance/license notes; ensure any B4 candidate set is derived from audited facts rather than unverified rows.

## A1 handoff — source and provenance report acceptance (2026-10-28)

Task / status: A1 acceptance complete as a source/provenance investigation. This does not validate the legacy inventory's per-entry compatibility or undo claims.
Scope: `docs/source-inventory.md` and linked deterministic extracts/reviews: `scripts/audit-source-inventory.py`, `docs/upstream-tweaks-facts.json`, `docs/inventory-source-comparison.json`, `docs/id-divergence-review.md`, `docs/inventory-claim-audit.md`, `docs/custom-action-audit.md`, and `docs/declarative-handler-audit.md`. Frozen source remains read-only.
Result: Reproducible method and hashes/counts for 66 tweaks and 232 apps; exact-ID comparison with 42 matches and 24 unmatched IDs on each side; per-entry fact extraction with custom script hashes; all 25 custom action entries read and risk-reviewed; selected ID correspondences checked against handlers; registry/service/combo/DNS/application paths traced. Upstream source license identified as MIT and notice requirement recorded. Third-party package licenses, availability and OS support remain explicitly outside evidence. The small `WPFTweaksActivity`/`WPFTweaksHiber` samples are directly checked. The report marks all inferred claims and unknowns rather than treating them as facts.
Checks: `python scripts/audit-source-inventory.py` reproduces hashes/counts/mismatches; `--write-facts` deterministically emits the per-entry source and comparison JSON. Assertions confirm 90-ID union / 42 exact matches / 24+24 unmatched, all 25 script-bearing IDs documented, and combo/service field inventories. Modified-document whitespace check passes. Frozen-source worktree status is empty.
Risks / gaps: A1 is not approval of `TWEAKS_INVENTORY.md`; its per-entry OS and undo claims remain unverified, 24+24 ID divergence needs product decision, and application package licensing/availability is not audited. Do not implement from that draft. A1 deliverable is a provenance/reporting baseline with open questions.
Next: B4 depends on the new schema contract in `src/data/catalog.rs` and a separately reviewed small candidate subset. The schema is not yet wired to a built-in catalog or loader. Keep runtime mutation entrypoints disabled.

## B1 continuation — declarative catalog schema v1 (2026-10-28)

Task / status: Typed catalog schema slice implemented and verified; B1 overall remains in progress.
Scope: `src/data/catalog.rs` (new), `src/data/mod.rs`, `docs/catalog-schema.md`, `AGENT_START.md`, and this handoff.
Result: Added versioned serde types with source path/hash, per-entry provenance, explicit unverified/verified OS evidence, and typed registry set/delete or service startup-type operations. Registry changes reuse exact native type/data bytes. Unknown fields/operation variants are rejected; validation checks IDs, duplicate IDs, source hash/path, evidence/range, operation target and SCM startup-type bounds. No fixed rollback state, script/shell, package, filesystem operation, loader, configured catalog, executor, or UI/CLI route exists. The transaction engine remains responsible for obtaining and persisting actual pre-state.
Checks: Rustup stable MSVC 1.97.1 with isolated `CARGO_TARGET_DIR=target/b1-catalog-final`: `cargo fmt --all -- --check`; default `cargo test --locked --lib` (122 passed), `cargo check --locked`, strict all-target Clippy; feature `experimental-ui` library tests and strict all-target Clippy passed (existing six vendor Sciter warnings). Nightly 1.101.0 x86/x64 Win7 release builds passed with `-Z build-std=std,panic_abort --release`; `scripts/audit-win7-imports.ps1` selected import/CRT/subsystem checks passed for both outputs. No Win7 VM was run.
Risks / gaps: Schema validation does not establish that source evidence is truthful, operations are safe, or OS support is real. There is no loader/config sample or integration with an executor. Full B1 namespace/power-loss release gates and actual Win32/SCM validation remain open; application mutation stays disabled.
Next: B4 may implement a small schema-validated, source-traced catalog fixture with explicit unverified support and mock transaction tests only after selecting reviewed actions. Add loader integration separately; do not expose an execution path until the release gates are reviewed.

## B4 continuation — first schema-validated catalog fixture (2026-10-28)

Task / status: One source-traced data fixture, bounded decoder, and compile-time access gate added; B4 implementation/release remains in progress.
Scope: `config/catalog.v1.json`, `src/data/catalog.rs`, `docs/catalog-schema.md`, `AGENT_START.md`.
Result: Added the three `WPFTweaksActivity` DWORD=0 declarations from frozen source as registry64 value operations. The entry's support remains explicitly `unverified`; no upstream `<RemoveEntry>` strings are carried forward as rollback values. The byte decoder rejects input above 1 MiB, deserializes with unknown-field rejection, then validates schema and operation data. The fixture is included only by a unit test; no production code loads or executes it. The operations field is private to the schema module; `verified_operations()` returns `None` for `Unverified` metadata and the fixture test asserts this. This only prevents accidental API access: the public/deserializable `Verified` metadata can be authored directly and is not a trusted approval or evidence verifier.
Checks: Rustup stable MSVC 1.97.1, isolated `CARGO_TARGET_DIR=target/b4-verified-gate-final`: `cargo fmt --all -- --check`, `cargo test --locked --lib` (125 passed, including crash child-process probes), `cargo check --locked`, and `cargo clippy --locked --all-targets -- -D warnings` all passed. Nightly MSVC 1.101.0 x86/x64 Win7 release builds with `-Z build-std=std,panic_abort --release` both passed; `scripts/audit-win7-imports.ps1` selected import/CRT/subsystem checks passed for both outputs. Prior Python verification compared fixture values and source SHA-256 to the frozen registry source. Frozen upstream status is empty.
Risks / gaps: This verifies source transcription, serialization, host behavior and target compilation/selected PE properties only; OS behavior, key creation policy, support range and transaction-backed application are not verified. No Win7 VM, power-loss test, production loader or operation executor exists. Keep the fixture test-only. A future loader needs an independently trusted approval policy, plus the remaining namespace, power-loss, actual SCM and Win7 VM gates.
Next: Continue B1/B2 contract gaps before exposing any catalog. Select the next discrete task from remaining namespace replacement/external-writer or real SCM race work, preserving the no-live-mutation boundary.

## A3 continuation — current Sciter license and Win7 evidence (2026-10-28)

Task / status: Primary-source review updated; runtime distribution and actual Win7 execution remain unresolved.
Scope: `docs/compatibility-feasibility.md`; read-only review of vendor Rust binding, active SDK/EULA/changelog, and Rust target documentation.
Result: Corrected the source location: the old GitHub Sciter.JS SDK is archived and points to the active `sciter-engine/sciter-js-sdk` GitLab project. Separated its BSD-3-Clause SDK license from the engine EULA. The current EULA names `sciter.dll`, permits using it to build commercial/noncommercial applications, and requires an About attribution link, but it does not expressly explain runtime DLL redistribution or give a version-specific distribution grant. Recorded that the active changelog contains a Win7 compatibility note for version 6.0.3.18; this is not evidence for the project's archived 4.4.8.34 evaluation DLL running on Win7. Updated Rust target status from current upstream docs: both project target triples are Tier 3 and need build/runtime evidence.
Checks: Browsed primary Rust and Sciter vendor sources; compared them to the local `sciter-rs` 0.5.58 dependency and existing report. No runtime was downloaded, packaged, or executed in this continuation.
Risks / gaps: No written vendor clarification of redistribution terms; no Win7 VM run; no proof that current SDK CSS/DOM behavior matches the project's Preact preview; binding ownership/ABI audit remains open. Keep Sciter experimental and unbundled.
Next: Obtain written runtime distribution clarification or choose a runtime with clear redistribution rights, then test that exact version and architecture in disposable Win7 VMs. Continue A1's complete per-entry source audit before any catalog implementation.
