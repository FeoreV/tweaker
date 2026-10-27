# Win7 and Sciter feasibility: evidence and remaining checks

Updated 2026-10-28. No release compatibility is asserted by this report.

## Rust

The current [Rust Win7 target documentation](https://doc.rust-lang.org/nightly/rustc/platform-support/win7-windows-msvc.html) lists `i686-win7-windows-msvc` and `x86_64-win7-windows-msvc` as tier 3. It says precompiled standard libraries are not distributed and host tools may not work; the normal [Windows MSVC target documentation](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html) specifies Windows 10 or newer. These targets therefore require their own build and runtime evidence. The documentation page reports nightly testing on a maintainer's private infrastructure; this is not a project guarantee.

Initial checks found only stable x86_64 MSVC. Nightly 1.101.0 (2026-10-26) and rust-src were subsequently installed for the build experiment below. `vswhere` identifies Visual Studio 2022 Community; that alone does not prove x86/x64 linkers, SDK libraries, or required components are installed. PE inspection tools were not found in PATH, but Visual Studio dumpbin was located and used in the build experiment below. Execution on Windows 7 remains untested. In particular `NTDDI_WIN7`, CRT linkage, target dependencies and Sciter runtime imports require verification on the resulting binaries.

Remaining work: audit the complete import tables for Win7 availability and test in clean x86 and x64 Win7 SP1 VMs.

## Sciter

The vendor's [Rust binding repository](https://github.com/sciter-sdk/rust-sciter) documents Sciter.JS support and requires a separately obtained engine; it explicitly says the engine has separate license terms. The archived [Sciter.JS SDK](https://github.com/c-smile/sciter-js-sdk) moved to the active [`sciter-engine/sciter-js-sdk` GitLab project](https://gitlab.com/sciter-engine/sciter-js-sdk/-/tree/main). That project's README lists published Windows x86/x64/ARM64 builds, but does not state a minimum Windows version. Its [SDK LICENSE](https://gitlab.com/sciter-engine/sciter-js-sdk/-/raw/main/LICENSE) is BSD-3-Clause; that does not establish rights for the runtime DLL. The separate [engine EULA](https://gitlab.com/sciter-engine/sciter-js-sdk/-/raw/main/SCITER-ENGINE-EULA.md) defines `sciter.dll`, permits using it to build commercial or non-commercial applications, and requires an attribution link in an About dialog or similar place. The text does not expressly describe redistribution of the runtime binary or give a version-specific redistribution grant. Do not infer runtime redistribution rights from the SDK license or the Rust binding's MIT license; get written clarification for the exact runtime and distribution model before packaging it.

The active SDK's [changelog](https://gitlab.com/sciter-engine/sciter-js-sdk/-/raw/main/CHANGELOG.md) records Windows XP support restored in 6.0.3.18 and an XP/7 compatibility fix for that release; it also records a raster-only Windows XP build in 6.0.2.21. These are upstream release notes, not a Win7 execution result for this project or its selected engine. The project's previously tested evaluation DLL is archived Sciter.JS 4.4.8.34, loaded only on the current host. No actual Win7 run or runtime redistribution is established.

Sciter documents [ES2020](https://docs.sciter.com/docs/JS/) but its [Document](https://docs.sciter.com/docs/DOM/Document/) differs from browsers. The current Preact preview uses browser DOM APIs and CSS; support for the full set in the selected Sciter runtime is unverified. No automatic GDI+ fallback, Safe Mode or WinPE claim has been established. The preview has only been exercised in a browser.

Next experiment: obtain a specifically identified runtime under its applicable terms, inspect x86/x64 PE imports, build a minimal non-mutating host with the selected binding, load the actual bundle and test rendering, navigation, input, styles and bridge calls. Run the same binary on a Win7 SP1 VM and separately test Safe Mode and WinPE. Retain the browser preview as a development tool until that evidence exists.

## Build experiment (2026-10-27)

Using nightly 1.101.0, rust-src and MSVC, both project Win7 target commands passed for the current CLI. Initial binaries imported VCRUNTIME140 and UCRT DLLs; `.cargo/config.toml` now requests `+crt-static` for both Win7 targets and sets console subsystem 6.01 for the current CLI. Rebuilt binaries import `ntdll.dll`, `kernel32.dll` and `advapi32.dll`; dumpbin found none of WaitOnAddress, WakeByAddress, CreateFile2, SetThreadDescription or srclient.dll in their import tables. Both headers report OS and subsystem 6.01. Both binaries ran `--version` and `--diagnostics` on the current 64-bit Windows host.

This is a partial import audit, not proof that every imported function supports Win7. No Win7 VM, Sciter runtime, GUI, Safe Mode or WinPE test has been run. A future GUI must reassess the console subsystem.

## Sciter runtime probe (2026-10-27)

For evaluation only, downloaded x86/x64 Sciter.JS DLLs from the archived vendor SDK at commit `e3f9580f25a2ca09eca8b7a725c0af7ec4e80f54` into ignored `target/sciter-evaluation/`. Git blob hashes match the GitHub API: x86 `0de3e7449a1f3fb5f01c4394a7e80169c7f25456`, x64 `d0873d953e15d5d0d00f094579d69f573b0bb75f`. The x86/x64 experimental Win7 binaries loaded their respective DLLs and reported version `4.4.8.34` on the current host with `--sciter-version`. The x64 DLL's selected forbidden-import search was empty; this is not a complete API compatibility audit.

The archived SDK [repository license](https://github.com/c-smile/sciter-js-sdk/blob/main/LICENSE) is BSD-3-Clause, but scope and current distribution terms for the runtime have not been established. DLLs remain ignored evaluation artifacts and are not packaged. Loading a version does not establish that Preact renders in Sciter or runs on Win7; neither has been tested.

## Hidden Sciter DOM smoke test (2026-10-27)

Both Win7-target binaries, each with matching Sciter.JS 4.4.8.34 DLL, loaded the embedded Preact UI in a hidden window on the current host. The test observed the initial `Overview` heading, invoked navigation, pumped the thread message queue and observed `Tweaks`. Browser-style `max-width` media syntax and `:focus-visible` produced Sciter warnings; changing to `(width <= 700px)` and `:focus` removed those warnings in this test. This confirms a small DOM interaction, not actual visual layout, complete CSS support, safe window lifetime, or Win7 behavior.

`sciter-rs` 0.5.58 `Window::new()` combines window flags by transmuting an arbitrary bitmask into a Rust enum. On this toolchain it aborted on invalid enum value 0x84. The experimental host now calls `Window::create` with one valid `SW_TITLEBAR` value instead. This avoids that particular path but does not establish soundness of the remaining binding or compatibility with future notifications; notably the binding also transmutes callback codes. Production use requires auditing or replacing the binding, and should not assume the single-flag workaround fixes its other unsafe paths.

## Local binding patch (2026-10-27)

A local MIT-licensed copy of `sciter-rs` 0.5.58 in `vendor/sciter-rs` replaces unchecked host notification enum transmutation with integer matching. It also removes upstream `Value` indexing implementations that cast a shared reference to a mutable one. `PATCHES.md` documents the scope. Other unsafe enum conversions and mutable statics remain; compiler warnings are emitted, so this is not a full binding safety audit. Both architecture smoke tests still pass on the current host.

## Callback enum audit continuation (2026-10-27)

The event-dispatch path was then hardened for raw codes supplied by Sciter. `EVENT_GROUPS` and `BEHAVIOR_EVENTS` use transparent integer wrappers, preserving C-width layout while accepting bit combinations and arbitrary application event IDs. Initialization, SOM, and draw commands are read as raw `UINT`; draw layers and phases are validated before conversion. Click/edit reasons are mapped when known and otherwise preserved as `EventReason::Unknown`. Unknown commands and null callback parameters are ignored safely. `vendor/sciter-rs/PATCHES.md` describes the local changes.

Targeted vendor tests for combinations, custom event IDs, and unknown phase/layer/reason values pass. The full standalone vendor test run reported 11 passing tests and 2 failures because the ABI/value tests tried to load `sciter.dll`, which was not available under that filename in the test process path. Root tests with the optional UI feature pass (40 library and 2 CLI tests). The vendor still emits warnings about mutable statics, dropped ownership and lifetimes, and other upstream transmutations outside the event-dispatch path remain unaudited.

After this patch, both nightly Win7 target release builds with `experimental-ui` passed. The selected import/subsystem audit and hidden Sciter smoke passed for both architectures on the current Windows host. This does not establish actual Windows 7 behavior, full PE API compatibility, runtime redistribution rights, or complete binding safety.

## DOM and window bitmask audit continuation (2026-10-27)

`ELEMENT_STATE_BITS` and `SCITER_CREATE_WINDOW_FLAGS` also represented open-ended bitmasks as Rust enums and used transmute. They are now transparent `UINT` wrappers with associated constants and bitwise operations. DOM state reads retain unknown bits safely; window creation and `WindowBuilder` operate on the raw ABI value. Focused tests cover combined and unknown bits. This also removes the prior `Window::new` workaround that supplied only one valid enum flag.

After these changes, root `experimental-ui` tests (40 library, 2 CLI), formatting, strict feature Clippy and feature check passed. Both nightly Win7 release targets rebuilt; selected import/subsystem audits passed, and matching x86/x64 Sciter smoke runs returned `Overview -> Tweaks` on the current Windows host.

## Loader shared-state audit (2026-10-27)

The binding's configurable custom DLL path and extension host API were also stored in `static mut`. Both are now mutex-protected; loader calls clone the path under the lock before calling the platform loader, and poisoned locks recover their current values. This removes those two mutable-static races, not all thread-safety concerns in the binding. Root feature check/tests/fmt/strict Clippy and standalone `extension` feature check passed. Both Win7 release targets rebuilt and passed the selected import/subsystem audit and hidden x86/x64 Sciter smoke on the current host.

Remaining concerns include raw-pointer ownership and lifetimes, function-pointer ABI assumptions, upstream compiler warnings, Windows 7 VM behavior, full import compatibility, and runtime licensing. Sciter remains experimental; no production safety or redistribution conclusion follows.

## SOM asset ownership continuation (2026-10-27)

The binding previously exposed `IAsset::from_raw(&*mut som_asset_t) -> &mut IAsset<T>` as a safe method and used that whole-object mutable reference in callbacks for reference counting and passport caching. It is removed. Add/release callbacks now project only the `AtomicI32` field from the raw object pointer, and the immutable passport is obtained during construction before the asset is published. `IAssetRef::from_raw` is now unsafe with a documented liveness precondition. Tests exercise clone/release lifetime, passport access and null callback handles. The wrappers `Element`, `Value` and `AssetPtr<T>` no longer assert undocumented cross-thread `Send`/`Sync` guarantees.

`AssetPtr::adopt` is also unsafe with a documented pointer/vtable precondition; the safe `From<*mut T>` implementation was removed, and interface-query results are checked for null. Root feature tests/check/fmt/strict Clippy, standalone extension check and both focused SOM tests pass. Both Win7 release targets rebuilt after the edits; selected import/subsystem audits and x86/x64 hidden Sciter smoke passed on the current Windows host. Other raw-pointer and function-pointer boundaries remain under review.

The public safe `From<HELEMENT>` conversion has also been replaced by `unsafe Element::from_raw`; internal code uses a private conversion only for handles returned from Sciter API calls. The binding's value examples were updated after removal of the unsound indexing implementations. With the already-present local x64 evaluation DLL exposed as `sciter.dll` only for testing, 33 enabled vendor doctests pass and 10 are ignored. Without the runtime, 13 Sciter-dependent doctests fail at DLL load; none of those failures should be presented as code/test success. Both Win7 builds and selected audits/smoke were rerun after the Element change on the current host.

`Host::attach`, `Host::attach_with`, `Window::attach`, and `Window::attach_intercepted` now require unsafe calls with documented live-window/thread preconditions. Compile-fail doctests assert that DOM, Value and video asset pointer wrappers stay non-`Send`/non-`Sync`. The complete vendor doc suite passes with the local evaluation DLL (38 passed, 10 ignored); two SOM ownership tests pass. Root feature checks/tests/fmt/strict Clippy pass. Both Win7 target builds, selected import/subsystem audits and hidden smoke pass after these attachment API changes on the current host; this is not execution on Windows 7.

`Window::create` and `Builder::with_parent` now also make the native parent HWND precondition explicit. The preview passes `None` and records its safety argument at the call site. All root feature gates, vendor docs, Win7 builds, selected audits and smoke were rerun after this change on the current host.

The pointer-input pass now marks public DOM lookups/child-window attachment, raw UTF conversion helpers, windowless message handling, and video interface lookup unsafe where callers provide raw external pointers. Event/request source parameters borrow `&Element` instead of accepting caller-supplied `HELEMENT`. The full vendor unit suite passes (17 tests), and its docs pass with the local runtime (38 passed, 10 ignored); root feature tests/check/fmt/strict Clippy also pass. Both Win7 builds/audits and hidden Sciter smoke passed after these changes on the current host.

A3 remains open for validity/lifetime guarantees of non-null callback buffers supplied by Sciter, remaining ownership edges, function-pointer ABI assumptions, runtime license terms and Win7 VM execution.

## Callback payload boundary pass (2026-10-27)

The subsequent source pass hardens image save/paint and windowless paint callbacks: null payloads are rejected, byte lengths are checked against Rust slice limits, bitmap dimensions/rectangle endpoints use checked arithmetic, and unwinding panics from Rust closures are caught at the FFI boundary. With `panic=abort`, panics still abort the process. Value enumeration rejects null context/key/value pointers. This addresses null, arithmetic and unwind behavior only; the binding cannot prove that non-null buffer pointers supplied by Sciter are valid for the announced length. The vendor suite passed with the local evaluation DLL; both Win7 target builds and selected import/subsystem checks also passed after the patch. After the B1 file-store additions were compiled into the current Win7 binaries, both x86/x64 smoke runs still reported `Overview -> Tweaks` on this host. Remaining function-pointer ABI, ownership/lifetime, license and Win7 VM issues are unchanged.
