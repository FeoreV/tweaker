//! Experimental Sciter host for the embedded read-only preview.
//! Sciter runtime licensing, compatibility, rendering and Win7 support are not
//! yet established; the caller explicitly supplies its own runtime.
use std::path::Path;

const SCRIPT: &str = include_str!("../../ui/dist/bundle.js");
const STYLE: &str = include_str!("../../ui/dist/styles.css");

fn markup() -> Result<String, String> {
    // Never let bundled text close its containing element.
    if SCRIPT.to_ascii_lowercase().contains("</script")
        || STYLE.to_ascii_lowercase().contains("</style")
    {
        return Err("embedded UI assets contain an unsafe closing tag".into());
    }
    Ok(format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>rustweaker preview</title><style>{STYLE}</style></head><body><div id=\"app\"></div><script type=\"module\">{SCRIPT}</script></body></html>"))
}

fn load_runtime(runtime_path: &str) -> Result<(), String> {
    let path = Path::new(runtime_path);
    if !path.is_absolute() || !path.is_file() {
        return Err("provide an absolute path to an existing Sciter DLL".into());
    }
    sciter::set_library(runtime_path)
        .map_err(|error| format!("could not load Sciter runtime: {error}"))
}

/// Load the explicitly selected runtime and report its version without a window.
pub fn runtime_version(runtime_path: &str) -> Result<String, String> {
    load_runtime(runtime_path)?;
    Ok(sciter::version())
}

fn create_window() -> sciter::Window {
    // sciter-rs 0.5.58 combines C bitflags by transmuting them into a Rust
    // enum, which is undefined behavior for combinations. Use one flag only.
    let rect = sciter::types::RECT {
        left: 0,
        top: 0,
        right: 1000,
        bottom: 700,
    };
    // SAFETY: no external parent handle is supplied; Sciter creates the window.
    unsafe { sciter::Window::create(rect, sciter::window::Flags::SW_TITLEBAR, None) }
}

/// Load the UI in a hidden window and verify that its initial heading exists.
/// This checks DOM initialization, not visible rendering or interactive behavior.
pub fn smoke_test(runtime_path: &str) -> Result<String, String> {
    let html = markup()?;
    load_runtime(runtime_path)?;
    let mut window = create_window();
    window.collapse(true);
    if !window.load_html(html.as_bytes(), None) {
        return Err("could not load UI preview".into());
    }
    // SAFETY: the handle comes from the live Sciter window owned in this scope.
    let root = unsafe { sciter::Element::from_window(window.get_hwnd()) }
        .map_err(|e| format!("could not inspect UI: {e:?}"))?;
    let heading = root
        .find_first("#app h1")
        .map_err(|e| format!("could not inspect heading: {e:?}"))?;
    let text = heading
        .ok_or("UI did not render its initial heading")?
        .get_text();
    if text != "Overview" {
        return Err(format!("unexpected UI heading: {text:?}"));
    }
    root.eval_script("document.querySelectorAll('nav button')[1].click()")
        .map_err(|e| format!("could not activate navigation: {e:?}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        pump_messages();
        let heading = root
            .find_first("#app h1")
            .map_err(|e| format!("could not inspect heading: {e:?}"))?
            .ok_or("UI heading disappeared after navigation")?
            .get_text();
        if heading == "Tweaks" {
            return Ok(format!("{text} -> {heading}"));
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("navigation did not update heading: {heading:?}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn pump_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    // SAFETY: zero is a valid initial state for MSG and all messages are taken
    // from the current thread's queue and dispatched while the hidden window lives.
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Launch the preview with an explicitly selected, existing Sciter DLL.
/// No download, extraction, bridge or system mutation is performed.
pub fn preview(runtime_path: &str) -> Result<(), String> {
    let html = markup()?;
    load_runtime(runtime_path)?;
    let mut window = create_window();
    if !window.load_html(html.as_bytes(), None) {
        return Err("could not load UI preview".into());
    }
    window.run_app();
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_preview_contains_assets_and_root() {
        let page = super::markup().unwrap();
        assert!(page.contains("id=\"app\""));
        assert!(page.contains("<script type=\"module\">"));
        assert!(page.contains(super::STYLE));
        assert!(page.contains(super::SCRIPT));
    }
    #[test]
    fn relative_runtime_path_is_rejected_before_loading() {
        assert!(super::preview("sciter.dll").is_err());
    }
}
