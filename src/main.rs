use rustweaker::engine::registry::{Hive, RegistryView, ValueTarget};

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Version,
    Diagnostics,
    PreviewUi(String),
    SciterVersion(String),
    SciterSmoke(String),
    DiskInfo(u8),
    ServiceStatus(String),
    ServiceConfig(String),
    ReadRegistry(ValueTarget),
}

const HELP: &str = "rustweaker (development build)\n\nUsage:\n  rustweaker --help\n  rustweaker --version\n  rustweaker --diagnostics\n  rustweaker --ui-preview <absolute-sciter-dll-path>\n  rustweaker --sciter-version <absolute-sciter-dll-path>\n  rustweaker --sciter-smoke <absolute-sciter-dll-path>\n  rustweaker --disk-info <physical-drive-number>\n  rustweaker --service-status <service-name>\n  rustweaker --service-config <service-name>\n  rustweaker --read-registry <hive> <view> <key-path> <value-name>\n\nHives: HKCU, HKLM, HKU, HKCR, HKCC\nViews: native, 64, 32\nUse an empty value name for the default value. Registry access is read-only.";

fn parse(args: &[String]) -> Result<Command, &'static str> {
    match args {
        [] => Err("GUI is not available yet; use --help for available commands"),
        [flag] if flag == "--help" || flag == "-h" => Ok(Command::Help),
        [flag] if flag == "--version" || flag == "-V" => Ok(Command::Version),
        [flag] if flag == "--diagnostics" => Ok(Command::Diagnostics),
        [flag, path] if flag == "--sciter-smoke" && !path.is_empty() => {
            Ok(Command::SciterSmoke(path.clone()))
        }
        [flag, path] if flag == "--sciter-version" && !path.is_empty() => {
            Ok(Command::SciterVersion(path.clone()))
        }
        [flag, path] if flag == "--ui-preview" && !path.is_empty() => {
            Ok(Command::PreviewUi(path.clone()))
        }
        [flag, index] if flag == "--disk-info" => Ok(Command::DiskInfo(
            index.parse().map_err(|_| "invalid physical drive number")?,
        )),
        [flag, name] if flag == "--service-config" && !name.is_empty() => {
            Ok(Command::ServiceConfig(name.clone()))
        }
        [flag, name] if flag == "--service-status" && !name.is_empty() => {
            Ok(Command::ServiceStatus(name.clone()))
        }
        [flag, hive, view, key_path, value_name] if flag == "--read-registry" => {
            let hive = match hive.to_ascii_uppercase().as_str() {
                "HKCU" => Hive::CurrentUser,
                "HKLM" => Hive::LocalMachine,
                "HKU" => Hive::Users,
                "HKCR" => Hive::ClassesRoot,
                "HKCC" => Hive::CurrentConfig,
                _ => return Err("unknown registry hive"),
            };
            let view = match view.as_str() {
                "native" => RegistryView::Native,
                "64" => RegistryView::Registry64,
                "32" => RegistryView::Registry32,
                _ => return Err("unknown registry view"),
            };
            if key_path.is_empty() {
                return Err("key path must not be empty");
            }
            Ok(Command::ReadRegistry(ValueTarget {
                hive,
                view,
                key_path: key_path.clone(),
                value_name: value_name.clone(),
            }))
        }
        _ => Err("invalid arguments; use --help for usage"),
    }
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Help => println!("{HELP}"),
        Command::Version => println!("rustweaker {} (development)", env!("CARGO_PKG_VERSION")),
        Command::ReadRegistry(target) => read_registry(&target)?,
        Command::Diagnostics => diagnostics()?,
        Command::PreviewUi(path) => preview_ui(&path)?,
        Command::SciterVersion(path) => sciter_version(&path)?,
        Command::SciterSmoke(path) => sciter_smoke(&path)?,
        Command::DiskInfo(index) => disk_info(index)?,
        Command::ServiceStatus(name) => service_status(&name)?,
        Command::ServiceConfig(name) => service_config(&name)?,
    }
    Ok(())
}

#[cfg(all(windows, feature = "experimental-ui"))]
fn sciter_smoke(path: &str) -> Result<(), String> {
    println!("{}", rustweaker::ui::smoke_test(path)?);
    Ok(())
}
#[cfg(not(all(windows, feature = "experimental-ui")))]
fn sciter_smoke(_: &str) -> Result<(), String> {
    Err("UI preview is not enabled in this build".into())
}

#[cfg(all(windows, feature = "experimental-ui"))]
fn sciter_version(path: &str) -> Result<(), String> {
    println!("{}", rustweaker::ui::runtime_version(path)?);
    Ok(())
}
#[cfg(not(all(windows, feature = "experimental-ui")))]
fn sciter_version(_: &str) -> Result<(), String> {
    Err("UI preview is not enabled in this build".into())
}

#[cfg(all(windows, feature = "experimental-ui"))]
fn preview_ui(path: &str) -> Result<(), String> {
    rustweaker::ui::preview(path)
}
#[cfg(not(all(windows, feature = "experimental-ui")))]
fn preview_ui(_: &str) -> Result<(), String> {
    Err("UI preview is not enabled in this build".into())
}

#[cfg(windows)]
fn disk_info(index: u8) -> Result<(), String> {
    let info = rustweaker::engine::storage::query_physical_drive(index)
        .map_err(|e| format!("disk query failed: {e:?}"))?;
    println!(
        "{}",
        serde_json::to_string(&info).map_err(|e| e.to_string())?
    );
    Ok(())
}
#[cfg(not(windows))]
fn disk_info(_: u8) -> Result<(), String> {
    Err("disk queries are available only on Windows".into())
}

#[cfg(windows)]
fn service_config(name: &str) -> Result<(), String> {
    let config = rustweaker::engine::services::query_config(name)
        .map_err(|e| format!("service configuration query failed: {e:?}"))?;
    println!(
        "{}",
        serde_json::to_string(&config).map_err(|e| e.to_string())?
    );
    Ok(())
}
#[cfg(not(windows))]
fn service_config(_: &str) -> Result<(), String> {
    Err("service queries are available only on Windows".into())
}

#[cfg(windows)]
fn service_status(name: &str) -> Result<(), String> {
    let status = rustweaker::engine::services::query(name)
        .map_err(|e| format!("service query failed: {e:?}"))?;
    println!(
        "{}",
        serde_json::to_string(&status).map_err(|e| e.to_string())?
    );
    Ok(())
}
#[cfg(not(windows))]
fn service_status(_: &str) -> Result<(), String> {
    Err("service queries are available only on Windows".into())
}

#[cfg(windows)]
fn diagnostics() -> Result<(), String> {
    let snapshot = rustweaker::engine::diagnostics::collect()
        .map_err(|e| format!("diagnostics failed: {e:?}"))?;
    println!(
        "{}",
        serde_json::to_string(&snapshot).map_err(|e| e.to_string())?
    );
    Ok(())
}
#[cfg(not(windows))]
fn diagnostics() -> Result<(), String> {
    Err("diagnostics are available only on Windows".into())
}

#[cfg(windows)]
fn read_registry(target: &ValueTarget) -> Result<(), String> {
    use rustweaker::engine::registry::windows::{read, ReadState};
    let result = read(target).map_err(|e| format!("registry read failed: {e:?}"))?;
    let output = match result {
        ReadState::KeyMissing => serde_json::json!({"status":"key_missing"}),
        ReadState::ValueMissing => serde_json::json!({"status":"value_missing"}),
        ReadState::Present(value) => {
            serde_json::json!({"status":"present", "type":value.native_type, "bytes":value.bytes})
        }
    };
    println!("{output}");
    Ok(())
}
#[cfg(not(windows))]
fn read_registry(_: &ValueTarget) -> Result<(), String> {
    Err("registry access is available only on Windows".into())
}

fn main() {
    let args: Result<Vec<_>, _> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect();
    let result = args
        .map_err(|_| "arguments must be valid Unicode".to_owned())
        .and_then(|args| parse(&args).map_err(str::to_owned))
        .and_then(run);
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }
    #[test]
    fn parses_registry_view_and_empty_default_value() {
        assert_eq!(
            parse(&args(&["--read-registry", "hklm", "64", "SOFTWARE", ""])),
            Ok(Command::ReadRegistry(ValueTarget {
                hive: Hive::LocalMachine,
                view: RegistryView::Registry64,
                key_path: "SOFTWARE".into(),
                value_name: String::new()
            }))
        );
    }
    #[test]
    fn rejects_invalid_commands_and_missing_arguments() {
        for v in [
            &[][..],
            &["--clean"][..],
            &["--read-registry", "HKLM"][..],
            &["--read-registry", "HKLM", "wrong", "path", "value"][..],
        ] {
            assert!(parse(&args(v)).is_err());
        }
    }
}
