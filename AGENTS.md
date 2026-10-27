---
last_updated: 2026-10-28
maintained_by: Gemini 4 Argon (auto-research) & Antigravity
status: verified
title: Operating Instructions for AI Coding Agents
project: rustweaker
---

# AGENTS.md: Operating Instructions for rustweaker

> **Start here:** Read [AGENT_START.md](AGENT_START.md) for the verified repository state, task order, and handoff rules. Existing architecture and inventories are plans to validate, not evidence of implemented behavior.


> **Mandatory rules and operating guidelines for all AI coding agents working on the rustweaker project.**  
> Working code only. Finish the job. Plausibility is not correctness.

---

## 0. Non-Negotiables

These rules override any conflicting instructions:

1. **`winutil-26.08.19/` is strictly READ-ONLY.** Never modify, format, delete, or touch any file inside `winutil-26.08.19/`. It is the frozen upstream reference source.
2. **Never break Windows 7 SP1 compatibility.** Every line of Rust code must compile cleanly for `i686-win7-windows-msvc` and `x86_64-win7-windows-msvc` targeting `NTDDI_WIN7 (0x06010000)`.
3. **Handle Safety in `windows-sys 0.59.0`:**
   - `HANDLE` and `HKEY` are raw pointers (`*mut core::ffi::c_void`), NOT integers.
   - Always initialize handles to `std::ptr::null_mut()`.
   - Check validity via `!h.is_null() && h != INVALID_HANDLE_VALUE`.
   - Wrap every handle in an RAII struct implementing `Drop` (`CloseHandle`, `RegCloseKey`, `CloseServiceHandle`).
   - Keep handles on their owning thread by default. Cross-thread transfer requires a uniquely owning RAII wrapper and a reviewed `unsafe impl Send` whose safety comment proves lifetime, exclusive ownership, and exactly-once close semantics. Casting a raw handle to `usize` only changes its representation; it does not establish ownership or make cross-thread use safe. Do not add `Sync` without separately proving concurrent API use is valid.
4. **No Unbacked Mutations:** No registry key, service state, or filesystem object may be altered without first querying and persisting the current state to the active `BackupTransaction`.
5. **Zero WMI Policy:** Never use WMI crates or `wbemdisp.dll`. Use direct Win32 APIs (`RtlGetVersion`, `GlobalMemoryStatusEx`, `DeviceIoControl`).
6. **Zero Silent Crashes on 64-bit Windows:** When compiling or running the 32-bit binary, always pass `KEY_WOW64_64KEY (0x0100)` for registry writes to avoid silent WoW64 redirection to `Wow6432Node`.
7. **Never Fabricate:** Do not invent file paths, function signatures, command output, or crate versions. Read the file or run the command.
8. **Verify Before Declaring Done:** A plausible-looking diff is not proof. Run `cargo check`, `cargo test`, and verify outputs.

---

## 1. Key Commands

### Frontend Build (Preact + Sciter)
```bash
# Bundle UI assets into ui/dist/bundle.js and ui/dist/styles.css
npm --prefix ui run build
```

### Rust Compilation (Windows 7–11 Universal & Native)
```bash
# Build 32-bit Universal Release (runs on 32-bit Win7-10 and 64-bit Win7-11)
cargo +nightly build --target i686-win7-windows-msvc -Z build-std=std,panic_abort --release

# Build 64-bit Native Release (runs on 64-bit Win7 SP1, 8.1, 10, 11)
cargo +nightly build --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort --release

# Standard development check (host compiler)
cargo check
```

### Testing & Quality Assurance
```bash
# Run unit tests against in-memory mock registry and services
cargo test --lib

# Run linter with strict error gates
cargo clippy --all-targets -- -D warnings

# Check code formatting
cargo fmt --check
```

### PE Import Table Audit (Verify Win7 Compatibility)
```bash
# Ensure NO forbidden imports exist (WaitOnAddress, CreateFile2, SetThreadDescription)
llvm-objdump -p target/i686-win7-windows-msvc/release/rustweaker.exe | Select-String "WaitOnAddress"
```

---

## 2. Module Ownership & Agent Domains

| Directory / Subsystem | Assigned Role | Dependency policy |
|:---|:---|:---|
| Root manifests, `src/lib.rs`, `src/main.rs`, `.cargo/`, `.github/`, agent handoff docs | **Integrator / A0 owner** | Shared files: coordinate before edits; keep dependencies locked and tested on the declared toolchain. |
| `src/data/` | **Data & Journal Agent** | Existing storage/schema code uses `serde`, `serde_json`, and `windows-sys`; do not add crates without a reviewed need. |
| `src/engine/` | **Engine & Win32 Agent** | Existing engine modules own registry, service, storage, transaction, recovery and diagnostics. Keep UI imports out. All system writes remain behind the persisted transaction/recovery contracts. |
| `src/ui/` | **Experimental Sciter Host Agent** | Windows-only, feature-gated preview host. The runtime is explicitly supplied; no download or system mutation. |
| `ui/` | **Preact Preview Agent** | Standalone preview build; it is not wired to a Rust bridge or system-action backend. |

These are the current directories, not proposed ones. There is no `src/core/`, `src/cli/`, `src/engine/tweaks.rs`, or root `config/tweaks.json` in the verified checkout. Task-specific ownership and dependencies in `AGENT_TASKS.md` take precedence; keep agents on disjoint owned paths and route shared-file integration through A0.

---

## 3. Step-by-Step Workflows

### Before Adding a Tweak
1. Inspect `AGENT_TASKS.md`, `CODEBASE.md`, and the actual `src/data/`, `src/engine/`, and `ui/` files. The proposed architecture and inventory are not implemented interfaces.
2. The current UI data is a preview catalog; no production tweak schema or tweak-application module is established. Do not create an assumed `config/tweaks.json`, `src/engine/tweaks.rs`, or `NativeAction` API and describe it as integrated.
3. Agree the schema and transaction contract with the integrator. Any mutation must first persist exact pre-state to the active durable transaction and provide tested recovery. Keep actions unavailable where that guarantee is absent.
4. Add mock-backed apply/undo and failure tests in the existing owning module, then update the provenance and inventory documents with verified source evidence.

### Before Adding a UI Screen
1. Inspect the actual `ui/src/` tree and `src/ui/`; do not assume the proposed screens, Sidebar, router or bridge files exist.
2. Keep preview actions visibly unavailable unless a real backend contract exists. The current preview is not connected to Rust system operations.
3. Build with `npm --prefix ui run build` and verify the generated assets; do not claim Sciter or Windows 7 rendering based on a browser preview.

---

## 4. Pre-Edit Checklist

Before modifying any file, verify:
- [ ] Have I checked `CODEBASE.md` for dependent modules?
- [ ] Does this change affect 64-bit systems when called from 32-bit processes? (Pass `KEY_WOW64_64KEY`)
- [ ] Are all raw Win32 handles wrapped in RAII `Drop` guards?
- [ ] Does this operation run on a background thread instead of blocking the Sciter GUI thread?
- [ ] Is an automatic rollback record created before mutating the system?
