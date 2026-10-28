---
name: rustweaker Neverlose-Fluent Hybrid
version: 2.0.0
description: "Single source of truth design system for rustweaker. Inspired by NEVERLOSE dark cyber-tech menu and modern SaaS dashboards (hkcloud) for Sciter.js Direct2D/GDI+ desktop UI on Windows 7 SP1 through Windows 11."
colors:
  bg-canvas: "#080c14"
  bg-sidebar: "#090e18"
  bg-surface-card: "#0d1424"
  bg-surface-hover: "#141f36"
  bg-surface-active: "#16274a"
  border-subtle: "#16223b"
  border-strong: "#23355b"
  border-focus: "#0094ff"
  accent-primary: "#0094ff"
  accent-primary-hover: "#0080e0"
  accent-success: "#10b981"
  accent-warning: "#f59e0b"
  accent-danger: "#ef4444"
  toggle-track-off: "#2a3854"
  toggle-track-on: "#0094ff"
  text-primary: "#ffffff"
  text-secondary: "#8fa3bf"
  text-muted: "#4d6282"
typography:
  headline-display:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "22px"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  headline-lg:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "18px"
    fontWeight: 700
    lineHeight: 1.2
  headline-md:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1.3
  body-lg:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.4
  body-md:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.4
  body-sm:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.4
  mono-code:
    fontFamily: "Consolas, Cascadia Code, monospace"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  none: "0px"
  sm: "4px"
  md: "8px"
  lg: "12px"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  2xl: "32px"
components:
  card:
    backgroundColor: "{colors.bg-surface-card}"
    rounded: "{rounded.lg}"
    padding: "14px"
  tweak-row:
    height: "36px"
  toggle-switch:
    size: "34px"
    height: "18px"
    rounded: "{rounded.full}"
  button-primary:
    backgroundColor: "{colors.accent-primary}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.sm}"
    padding: "8px 16px"
  pill-active:
    backgroundColor: "{colors.accent-primary}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.full}"
  pill-inactive:
    backgroundColor: "{colors.bg-surface-card}"
    textColor: "{colors.text-secondary}"
    rounded: "{rounded.full}"
  job-drawer:
    backgroundColor: "{colors.bg-sidebar}"
    rounded: "{rounded.none}"
---

# rustweaker Design System (Neverlose-Fluent Hybrid)

## Overview
`rustweaker` is a high-performance Windows servicing, debloating, and optimization utility designed for professional computer technicians.
The visual identity unites the legendary, razor-sharp dark gaming utility aesthetic of **NEVERLOSE** with the polished, structured card dashboard design of **hkcloud**.

Key operating environment requirements:
- **Surface Mode**: *Operate*. Maximum clarity, scanability, and deterministic feedback.
- **Engine**: Sciter.js (Direct2D with automatic GDI+ software fallback for Windows Safe Mode and WinPE).
- **Zero Heavy Glassmorphism**: No CPU-taxing `backdrop-filter: blur()`. The aesthetic relies on crisp 1px borders (`#16223b`), tonal contrast between `#080c14` and `#0d1424`, and luminous electric cyan `#0094ff` accents.

## Colors
- **Canvas (`#080c14`)**: Deep midnight black-navy backdrop providing maximum visual rest and contrast.
- **Sidebar (`#090e18`)**: Elevated navigation rail.
- **Card Surface (`#0d1424`)**: Functional container panels (2x2 grid) with distinct 1px outline.
- **Primary Accent (`#0094ff`)**: Signature Neverlose electric cyan-blue. Used for active switches, selected pill tabs, primary action triggers, and progress indicators.
- **Success (`#10b981`)**: Emerald green for safe operations, completed batch steps, and healthy SMART status.
- **Warning (`#f59e0b`)**: Amber for caution tweaks requiring technician awareness.
- **Danger (`#ef4444`)**: Crimson red for destructive actions (UWP/Edge removals, network stack purge).

## Typography
- **UI Text**: `Segoe UI`, system-ui fallback. Clean, high legibility at 11–15px.
- **Telemetry & Logs**: `Consolas`, `Cascadia Code`. Used strictly for numbers, speeds, hardware IDs, registry keys, and live execution streaming.

## Layout
- **Window Base Size**: 1040 × 720px, scalable up to 4K High-DPI.
- **Sidebar Width**: 210px fixed rail with grouped categories (`SYSTEM`, `SOFTWARE`, `MAINTENANCE`, `AUDIT`).
- **Header**: Persistent preset selector quick-bar (`[Standard]` `[Minimal]` `[Advanced]` `[Custom ▾]`), 1-click batch apply trigger, and window controls.
- **Content Area**: 2x2 grid layout inside views for high density and zero vertical fatigue.
- **JobDrawer**: Pinned bottom tray (50px collapsed) with live batch counter; expands upward to 260px during batch execution with embedded terminal.

## Elevation & Depth
Depth is created strictly via tonal luminance steps and 1px borders, avoiding soft drop-shadow blur that degrades GDI+ software rendering:
- Level 0 (Canvas): `#080c14`
- Level 1 (Sidebar / Drawer): `#090e18`, border: `1px solid #16223b`
- Level 2 (Panels / Cards): `#0d1424`, border: `1px solid #16223b`
- Level 3 (Inputs / Selects): `#0a101d`, border: `1px solid #23355b`
- Level 4 (Active Pills / Buttons): `#0094ff` with white foreground.

## Shapes
- Cards: `12px` rounded corners.
- Pill Tabs & Buttons: `9999px` fully rounded capsules.
- Toggle Switches: `9999px` rounded capsule track (34×18px) with a 14×14px circular white thumb.
- Action Buttons & Inputs: `4px` or `6px` subtle bevel for a solid native tool feel.

## Components

### Tweak Row
- Height: 36px flex row.
- Left: Tweak title (`#ffffff`), optional `[⚙]` micro-button for sub-parameters.
- Right: Mini toggle switch. When active, background glows `#0094ff`. When inactive, muted `#2a3854`.

### App Tile
- Height: 52px card.
- Icon on left, package name, version/status, WinGet/Choco badge (`#16274a`), checkbox on right.

### Action Card (Diagnostics)
- Header with icon and title, description, risk badge, and a right-aligned action trigger button.

### JobDrawer Console
- Background: `#05080e`.
- Terminal text: Consolas 11px with color-coded log lines (`#10b981` info, `#f59e0b` warn, `#ef4444` error).

## Do's and Don'ts
- **DO** keep borders exactly 1px for crisp pixel alignment on non-integer DPI scaling.
- **DO** use real vector SVG icons instead of emoji or unicode glyphs.
- **DO** provide clear `disabled` opacity (0.55) and tooltip explanations on tweaks incompatible with the host Windows version.
- **DON'T** use `backdrop-filter: blur()` or heavy multi-layer box shadows.
- **DON'T** use gradient text.
- **DON'T** hide critical technician details behind multi-level nested modal dialogs.
