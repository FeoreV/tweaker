> **Project status (2026-10-27):** This document describes a proposed system, not a released or verified implementation. A minimal Rust skeleton and standalone UI preview exist; the application is not implemented. Validate technical claims and catalog entries against source and tests before implementation. See [AGENT_START.md](AGENT_START.md).

# rustweaker

> High-performance, zero-dependency universal Windows system tweaker, diagnostic toolkit, and debloat utility engineered for computer repair technicians.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform: Windows 7–11](https://img.shields.io/badge/Platform-Windows%207%20SP1%20--%2011%20(x86%20%2F%20x64)-0078d7.svg)](ARCHITECTURE.md)
[![UI: Sciter.js + Preact](https://img.shields.io/badge/UI-Sciter.js%20%2B%20Preact%2010-3b82f6.svg)](UI_SPEC.md)

---

## ⚡ Quick Start

### Running the Utility
`rustweaker` is distributed as a single portable executable with zero external dependencies (no .NET, no Visual C++ Redistributable, no WebView2):

* **Universal 32-bit (x86)**: Runs on any 32-bit or 64-bit Windows from Windows 7 SP1 to Windows 11.
* **Native 64-bit (x64)**: Maximum performance on 64-bit Windows installations.

Double-click `rustweaker-x86.exe` or `rustweaker-x64.exe` to launch the graphical user interface.

### Headless CLI Automation (WinPE & Batch Servicing)
```cmd
:: Run the Standard debloat preset silently
rustweaker-x86.exe --preset Standard --silent

:: Run temporary file and system junk cleanup
rustweaker-x86.exe --clean

:: Export active tweak configurations to a custom JSON preset
rustweaker-x86.exe --export "D:\presets\office_pc.json"

:: Import and apply a custom preset
rustweaker-x86.exe --import "D:\presets\office_pc.json" --silent
```

---

## 🛠️ Key Features

- **Universal Compatibility (Win 7 SP1 – Win 11)**: Operates seamlessly on Windows 7 SP1, 8.1, 10, and 11 across both 32-bit and 64-bit architectures, including **Windows Safe Mode** and **WinPE**.
- **Instant Native Startup**: Written in **Rust** with direct Win32 API calls (`Advapi32`, `Kernel32`, `Ntdll`), starting in under 20 milliseconds without PowerShell or WMI overhead.
- **Modern Dark UI**: High-contrast `#0d0d0d` dark theme with smooth CSS3 animations, built with **Preact 10.x** running on **Sciter.js** (Direct2D with automatic GDI+ software raster fallback).
- **66 System Tweaks & Debloat**: Complete catalog of privacy tweaks, telemetry disabling, service optimization, and visual settings mapped with full undo support.
- **Safety First & 1-Click Rollback**: Automatically creates a Windows System Restore Point and snapshots prior registry/service states before committing any change.
- **Zero-WMI Hardware Diagnostics**: Instant CPU, RAM, and storage SMART drive telemetry via kernel IOCTLs, immune to corrupted WMI repositories.
- **232 Essential Applications**: Dual-engine software installer supporting WinGet (Win 10/11) and Chocolatey (Win 7/8.1) with offline installer support.
- **System Repair Suite**: One-click actions for DISM, SFC, Chkdsk, network stack reset (Winsock/TCP-IP), and Windows Update cache reset.

---

## 🗺️ System Architecture

```text
┌────────────────────────────────────────────────────────┐
│      Presentation Layer (Sciter.js + Preact 10.x)      │
│     Modern Dark Theme (#0d0d0d) | Direct2D / GDI+      │
└───────────────────────────┬────────────────────────────┘
                            │ Sciter FFI (JSON-RPC)
┌───────────────────────────▼────────────────────────────┐
│      Coordination Bridge (src/ui/bridge.rs)            │
│   Worker Thread Pool (crossbeam-channel MPMC Queue)    │
└───────────────────────────┬────────────────────────────┘
                            │ Direct Kernel Calls
┌───────────────────────────▼────────────────────────────┐
│          Native Win32 Subsystems (Rust Engine)         │
│  Registry | Services | Diagnostics | Cleaner | Restore │
└────────────────────────────────────────────────────────┘
```

---

## 📚 Technical Documentation Suite

Complete documentation for developers and AI coding agents:

- **[ARCHITECTURE.md](ARCHITECTURE.md)**: System design, technology stack justifications, ABI breaking changes, and build matrix.
- **[CODEBASE.md](CODEBASE.md)**: Source file layout, module dependency graph, data flow pipelines, and development invariants.
- **[AGENTS.md](AGENTS.md)**: Operating instructions, build/test commands, HANDLE safety rules, and module ownership for AI agents.
- **[TWEAKS_INVENTORY.md](TWEAKS_INVENTORY.md)**: Complete 66-tweak catalog with registry keys, values, and Win7 compatibility flags.
- **[UI_SPEC.md](UI_SPEC.md)**: Design token palette, Preact component tree, animation parameters, and Sciter IPC schema.
- **[MEMORY.md](.agents/memory/MEMORY.md)**: Ground-truth project memory, locked architectural decisions, and open questions.

---

## 🏗️ Development & Building

### Prerequisites
- Node.js 18+ (for bundling UI assets)
- Rust Nightly (for Tier 3 `-win7-windows-msvc` builds with `-Z build-std`)

### Build Steps
```bash
# 1. Bundle Preact frontend assets
npm --prefix ui run build

# 2. Compile Universal 32-bit Release (Windows 7–11)
cargo +nightly build --target i686-win7-windows-msvc -Z build-std=std,panic_abort --release

# 3. Compile Native 64-bit Release (Windows 7–11)
cargo +nightly build --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort --release

# 4. Run in-memory unit tests
cargo test --lib
```

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
Reference upstream code by Chris Titus Tech (`winutil-26.08.19/`) is licensed under its respective open-source license.
