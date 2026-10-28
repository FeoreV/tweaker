use rustweaker::engine::registry::{Hive, RegistryView, ValueTarget};
use serde::Serialize;
use serde_json::Value;
use std::fmt::Debug;
use std::io::Read;

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
    ServicesReport(Vec<String>),
    WinsockReport,
    PlanPackages {
        action: rustweaker::engine::package_plan::PackageAction,
        preference: rustweaker::engine::package_plan::PackagePreference,
        catalog_ids: Vec<String>,
    },
    PlanUpgradeAll {
        preference: rustweaker::engine::package_plan::PackagePreference,
    },
    PlanImportedPackages {
        preference: rustweaker::engine::package_plan::PackagePreference,
        action: rustweaker::engine::package_plan::PackageAction,
    },
    DecodeSelection,
    ExportSelection(Vec<String>),
    PresetPreview {
        name: String,
        filter: String,
    },
    ExportApplicationSelection(Vec<String>),
    ApplicationsList,
    ApplicationShow(String),
    InstalledRegistryApps,
    ToggleStatus {
        id: String,
        view: RegistryView,
    },
    UtcStatus(RegistryView),
    LocationStatus(RegistryView),
    MatchChocolateyList,
    MatchWingetList,
    ReadRegistry(ValueTarget),
}

const HELP: &str = "rustweaker (development build)\n\nUsage:\n  rustweaker --help\n  rustweaker --version\n  rustweaker --diagnostics\n  rustweaker --applications-list\n  rustweaker --application-show <catalog-id>\n  rustweaker --installed-registry-apps (read-only uninstall registry inventory; not package-manager state)\n  rustweaker --toggle-status <WPFToggleDetailedBSoD|WPFToggleNumLock|WPFToggleWindowSnapping> <native|64|32> (source-audited read-only subset)\n  rustweaker --match-choco-list (reads UTF-8 text from stdin; never runs Chocolatey)\n  rustweaker --match-winget-list (reads UTF-8 text from stdin; never runs WinGet)\n  rustweaker --plan-install <choco|winget> <catalog-id>... (data only; does not install)\n  rustweaker --plan-uninstall <choco|winget> <catalog-id>... (data only; does not uninstall)\n  rustweaker --plan-upgrade-all <choco|winget> (data only; does not upgrade)\n  rustweaker --plan-import <choco|winget> <install|uninstall> (reads selection JSON from stdin; preview catalog only)\n  rustweaker --export-app-selection <catalog-id>... (prints WinUtil-compatible JSON; preview catalog only)\n  rustweaker --ui-preview <absolute-sciter-dll-path>\n  rustweaker --sciter-version <absolute-sciter-dll-path>\n  rustweaker --sciter-smoke <absolute-sciter-dll-path>\n  rustweaker --disk-info <physical-drive-number>\n  rustweaker --service-status <service-name>\n  rustweaker --service-config <service-name>\n  rustweaker --services-report <service-name> [<service-name> ...] (1-32 names)\n  rustweaker --utc-status <native|64|32> (source-audited read-only subset)\n  rustweaker --location-status <native|64|32> (source-audited read-only subset)\n  rustweaker --winsock-report\n  rustweaker --read-registry <hive> <view> <key-path> <value-name>\n\nHives: HKCU, HKLM, HKU, HKCR, HKCC\nViews: native, 64, 32\nUse an empty value name for the default value. Registry access is read-only.";

fn parse(args: &[String]) -> Result<Command, &'static str> {
    match args {
        [] => Err("GUI is not available yet; use --help for available commands"),
        [flag] if flag == "--help" || flag == "-h" => Ok(Command::Help),
        [flag] if flag == "--version" || flag == "-V" => Ok(Command::Version),
        [flag] if flag == "--diagnostics" => Ok(Command::Diagnostics),
        [flag] if flag == "--decode-selection" => Ok(Command::DecodeSelection),
        [flag, name, filter] if flag == "--preset-preview" => Ok(Command::PresetPreview {
            name: name.clone(),
            filter: filter.clone(),
        }),
        [flag] if flag == "--winsock-report" => Ok(Command::WinsockReport),
        [flag, preference] if flag == "--plan-upgrade-all" => {
            let preference = rustweaker::engine::package_plan::PackagePreference::parse(preference)
                .map_err(|_| "package manager must be choco or winget")?;
            Ok(Command::PlanUpgradeAll { preference })
        }
        [flag, preference, action] if flag == "--plan-import" => {
            let preference = rustweaker::engine::package_plan::PackagePreference::parse(preference)
                .map_err(|_| "package manager must be choco or winget")?;
            let action = match action.to_ascii_lowercase().as_str() {
                "install" => rustweaker::engine::package_plan::PackageAction::Install,
                "uninstall" => rustweaker::engine::package_plan::PackageAction::Uninstall,
                _ => return Err("package action must be install or uninstall"),
            };
            Ok(Command::PlanImportedPackages { preference, action })
        }
        [flag, preference, catalog_ids @ ..]
            if flag == "--plan-install" || flag == "--plan-uninstall" =>
        {
            if catalog_ids.is_empty() {
                return Err("package planning requires at least one catalog ID");
            }
            let preference = rustweaker::engine::package_plan::PackagePreference::parse(preference)
                .map_err(|_| "package manager must be choco or winget")?;
            let action = if flag == "--plan-install" {
                rustweaker::engine::package_plan::PackageAction::Install
            } else {
                rustweaker::engine::package_plan::PackageAction::Uninstall
            };
            Ok(Command::PlanPackages {
                action,
                preference,
                catalog_ids: catalog_ids.to_vec(),
            })
        }
        [flag] if flag == "--applications-list" => Ok(Command::ApplicationsList),
        [flag] if flag == "--installed-registry-apps" => Ok(Command::InstalledRegistryApps),
        [flag, id, view] if flag == "--toggle-status" => {
            let canonical_id = if id
                .eq_ignore_ascii_case(rustweaker::engine::toggle_status::SUPPORTED_TOGGLE_ID)
            {
                rustweaker::engine::toggle_status::SUPPORTED_TOGGLE_ID
            } else if id
                .eq_ignore_ascii_case(rustweaker::engine::numlock_status::SUPPORTED_TOGGLE_ID)
            {
                rustweaker::engine::numlock_status::SUPPORTED_TOGGLE_ID
            } else if id.eq_ignore_ascii_case(
                rustweaker::engine::window_snapping_status::SUPPORTED_TOGGLE_ID,
            ) {
                rustweaker::engine::window_snapping_status::SUPPORTED_TOGGLE_ID
            } else {
                return Err("unsupported source-audited toggle ID");
            };
            let view = match view.as_str() {
                "native" => RegistryView::Native,
                "64" => RegistryView::Registry64,
                "32" => RegistryView::Registry32,
                _ => return Err("registry view must be native, 64, or 32"),
            };
            Ok(Command::ToggleStatus {
                id: canonical_id.to_owned(),
                view,
            })
        }
        [flag, view] if flag == "--utc-status" => {
            let view = match view.as_str() {
                "native" => RegistryView::Native,
                "64" => RegistryView::Registry64,
                "32" => RegistryView::Registry32,
                _ => return Err("registry view must be native, 64, or 32"),
            };
            Ok(Command::UtcStatus(view))
        }
        [flag, view] if flag == "--location-status" => {
            let view = match view.as_str() {
                "native" => RegistryView::Native,
                "64" => RegistryView::Registry64,
                "32" => RegistryView::Registry32,
                _ => return Err("registry view must be native, 64, or 32"),
            };
            Ok(Command::LocationStatus(view))
        }
        [flag] if flag == "--match-choco-list" => Ok(Command::MatchChocolateyList),
        [flag] if flag == "--match-winget-list" => Ok(Command::MatchWingetList),
        [flag, catalog_ids @ ..] if flag == "--export-app-selection" => {
            if catalog_ids.is_empty() {
                return Err("application selection export requires at least one catalog ID");
            }
            Ok(Command::ExportApplicationSelection(catalog_ids.to_vec()))
        }
        [flag, ids @ ..] if flag == "--export-selection" => {
            if ids.is_empty() {
                return Err("selection export requires at least one selection ID");
            }
            Ok(Command::ExportSelection(ids.to_vec()))
        }
        [flag, id] if flag == "--application-show" && !id.is_empty() => {
            Ok(Command::ApplicationShow(id.clone()))
        }
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
        [flag, names @ ..] if flag == "--services-report" => {
            if !(1..=32).contains(&names.len()) {
                return Err("--services-report requires 1 to 32 service names");
            }
            if names
                .iter()
                .any(|name| !rustweaker::data::journal::valid_service_name(name))
            {
                return Err("--services-report contains an invalid service name");
            }
            Ok(Command::ServicesReport(names.to_vec()))
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
        Command::Help => println!("{HELP}\n  rustweaker --decode-selection (reads bounded JSON from stdin; validates source-derived selection IDs)\n  rustweaker --export-selection <WPF-selection-id>... (prints WinUtil JSON; selection metadata only)\n  rustweaker --preset-preview <name> <filter-pattern> (reads current selection JSON from stdin; never applies system actions)"),
        Command::Version => println!("rustweaker {} (development)", env!("CARGO_PKG_VERSION")),
        Command::ReadRegistry(target) => read_registry(&target)?,
        Command::Diagnostics => diagnostics()?,
        Command::PreviewUi(path) => preview_ui(&path)?,
        Command::SciterVersion(path) => sciter_version(&path)?,
        Command::SciterSmoke(path) => sciter_smoke(&path)?,
        Command::DiskInfo(index) => disk_info(index)?,
        Command::ServiceStatus(name) => service_status(&name)?,
        Command::ServiceConfig(name) => service_config(&name)?,
        Command::ServicesReport(names) => services_report(&names)?,
        Command::WinsockReport => winsock_report()?,
        Command::PlanPackages {
            action,
            preference,
            catalog_ids,
        } => plan_packages(&catalog_ids, preference, action)?,
        Command::PlanUpgradeAll { preference } => plan_upgrade_all(preference)?,
        Command::PlanImportedPackages { preference, action } => {
            plan_imported_packages_stdin(preference, action)?
        }
        Command::DecodeSelection => decode_selection_stdin()?,
        Command::ExportSelection(ids) => export_selection(&ids)?,
        Command::PresetPreview { name, filter } => preset_preview_stdin(&name, &filter)?,
        Command::ExportApplicationSelection(catalog_ids) => {
            export_application_selection(&catalog_ids)?
        }
        Command::ApplicationsList => applications_list()?,
        Command::ApplicationShow(id) => application_show(&id)?,
        Command::InstalledRegistryApps => installed_registry_apps()?,
        Command::ToggleStatus { id, view } => toggle_status(&id, view)?,
        Command::UtcStatus(view) => utc_status(view)?,
        Command::LocationStatus(view) => location_status(view)?,
        Command::MatchChocolateyList => match_chocolatey_list_stdin()?,
        Command::MatchWingetList => match_winget_list_stdin()?,
    }
    Ok(())
}

fn applications_list() -> Result<(), String> {
    let catalog =
        rustweaker::data::applications::embedded_applications().map_err(|e| e.to_string())?;
    let entries: std::collections::BTreeMap<_, _> = catalog.iter().collect();
    println!(
        "{}",
        serde_json::to_string(&entries).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn application_show(id: &str) -> Result<(), String> {
    let catalog =
        rustweaker::data::applications::embedded_applications().map_err(|e| e.to_string())?;
    let entry = catalog
        .get(id)
        .ok_or_else(|| format!("unknown application: {id}"))?;
    println!(
        "{}",
        serde_json::to_string(entry).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(windows)]
fn installed_registry_apps() -> Result<(), String> {
    let report = rustweaker::engine::installed_apps_windows::inventory_uninstall_registry();
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(not(windows))]
fn installed_registry_apps() -> Result<(), String> {
    Err("uninstall registry inventory is available only on Windows".into())
}

#[cfg(windows)]
fn toggle_status(id: &str, view: RegistryView) -> Result<(), String> {
    let json = if id == rustweaker::engine::window_snapping_status::SUPPORTED_TOGGLE_ID {
        serde_json::to_string(&rustweaker::engine::window_snapping_status::observe_windows(view))
            .map_err(|error| error.to_string())?
    } else if id == rustweaker::engine::numlock_status::SUPPORTED_TOGGLE_ID {
        serde_json::to_string(
            &rustweaker::engine::numlock_status::observe_windows(id, view)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?
    } else {
        serde_json::to_string(
            &rustweaker::engine::toggle_status::observe_windows(id, view)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?
    };
    println!("{json}");
    Ok(())
}

#[cfg(not(windows))]
fn toggle_status(_: &str, _: RegistryView) -> Result<(), String> {
    Err("toggle status is available only on Windows".into())
}

#[cfg(windows)]
fn utc_status(view: RegistryView) -> Result<(), String> {
    let report = rustweaker::engine::utc_status::observe_windows(
        rustweaker::engine::utc_status::SUPPORTED_TWEAK_ID,
        view,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(not(windows))]
fn utc_status(_: RegistryView) -> Result<(), String> {
    Err("UTC status is available only on Windows".into())
}

#[cfg(windows)]
fn location_status(view: RegistryView) -> Result<(), String> {
    let report = rustweaker::engine::location_status::observe_windows(
        rustweaker::engine::location_status::SUPPORTED_TWEAK_ID,
        view,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(not(windows))]
fn location_status(_: RegistryView) -> Result<(), String> {
    Err("Location status is available only on Windows".into())
}

fn export_application_selection(catalog_ids: &[String]) -> Result<(), String> {
    let catalog = rustweaker::data::applications::embedded_applications()
        .map_err(|error| error.to_string())?;
    let json = encode_application_selection(&catalog, catalog_ids)?;
    println!("{json}");
    Ok(())
}

fn encode_application_selection(
    catalog: &rustweaker::data::applications::ApplicationCatalog,
    catalog_ids: &[String],
) -> Result<String, String> {
    if catalog_ids.is_empty() {
        return Err("application selection export requires at least one catalog ID".into());
    }
    let mut selections = rustweaker::data::selection_import::SelectionCategories::default();
    for requested_id in catalog_ids {
        let canonical_id = catalog
            .iter()
            .find(|(catalog_id, _)| catalog_id.eq_ignore_ascii_case(requested_id))
            .map(|(catalog_id, _)| catalog_id)
            .ok_or_else(|| format!("unknown application catalog ID: {requested_id}"))?;
        selections.apps.push(format!("WPFInstall{canonical_id}"));
    }
    rustweaker::data::selection_export::encode_selection_export(&selections)
        .map_err(|error| format!("application selection export failed: {error:?}"))
}

fn read_bounded_choco_input<R: std::io::Read>(reader: R) -> Result<Vec<u8>, String> {
    let max_bytes = rustweaker::engine::package_state::MAX_CHOCOLATEY_LIST_BYTES;
    let mut input = Vec::new();
    reader
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|error| format!("could not read Chocolatey list input: {error}"))?;
    if input.len() > max_bytes {
        return Err(format!(
            "Chocolatey list input exceeds the {max_bytes}-byte limit"
        ));
    }
    Ok(input)
}

fn match_chocolatey_list_stdin() -> Result<(), String> {
    let stdin = std::io::stdin();
    let input = read_bounded_choco_input(stdin.lock())?;
    let catalog = rustweaker::data::applications::embedded_applications()
        .map_err(|error| error.to_string())?;
    let candidate_catalog_ids =
        rustweaker::engine::package_state::match_chocolatey_list_bytes(&catalog, &input)
            .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::json!({
            "report_kind": "choco_list_text_match",
            "basis": "user_supplied_text_only",
            "executable": false,
            "candidate_catalog_ids": candidate_catalog_ids,
        })
    );
    Ok(())
}

fn match_winget_list_stdin() -> Result<(), String> {
    let stdin = std::io::stdin();
    let max_bytes = rustweaker::engine::package_state::MAX_WINGET_LIST_BYTES;
    let mut input = Vec::new();
    stdin
        .lock()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|error| format!("could not read WinGet list input: {error}"))?;
    if input.len() > max_bytes {
        return Err(format!(
            "WinGet list input exceeds the {max_bytes}-byte limit"
        ));
    }
    let catalog = rustweaker::data::applications::embedded_applications()
        .map_err(|error| error.to_string())?;
    let candidate_catalog_ids =
        rustweaker::engine::package_state::match_winget_list_bytes(&catalog, &input)
            .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::json!({
            "report_kind": "winget_list_text_match",
            "basis": "user_supplied_text_only",
            "executable": false,
            "candidate_catalog_ids": candidate_catalog_ids,
        })
    );
    Ok(())
}

fn plan_packages(
    catalog_ids: &[String],
    preference: rustweaker::engine::package_plan::PackagePreference,
    action: rustweaker::engine::package_plan::PackageAction,
) -> Result<(), String> {
    let catalog = rustweaker::data::applications::embedded_applications()
        .map_err(|error| error.to_string())?;
    let plan = rustweaker::engine::package_plan::build_package_plan_for_action(
        &catalog,
        catalog_ids,
        preference,
        action,
    )
    .map_err(|error| error.to_string())?;
    let mut warnings = Vec::new();
    if plan.edge_filesystem_action_present {
        warnings.push("upstream uninstall runs New-Item -Force at %SystemRoot%\\SystemApps\\Microsoft.MicrosoftEdge_8wekyb3d8bbwe\\MicrosoftEdge.exe; this plan performs no filesystem action");
    }
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "plan_kind": "source_package_selection_only",
            "executable": false,
            "upstream_side_effect_warnings": warnings,
            "selection": plan,
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn plan_upgrade_all(
    preference: rustweaker::engine::package_plan::PackagePreference,
) -> Result<(), String> {
    let plan = rustweaker::engine::package_plan::build_upgrade_all_plan(preference);
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "plan_kind": "upgrade_all_intent_only",
            "executable": false,
            "upgrade": plan,
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn plan_imported_packages_stdin(
    preference: rustweaker::engine::package_plan::PackagePreference,
    action: rustweaker::engine::package_plan::PackageAction,
) -> Result<(), String> {
    let stdin = std::io::stdin();
    let input = read_bounded_selection_import(stdin.lock())?;
    let catalog = rustweaker::data::applications::embedded_applications()
        .map_err(|error| error.to_string())?;
    let result = rustweaker::engine::package_import::plan_package_import(
        &input, &catalog, preference, action,
    )
    .map_err(|error| error.to_string())?;
    let report = package_import_report(result);
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn package_import_report(result: rustweaker::engine::package_import::PackageImportResult) -> Value {
    let warnings = match &result {
        rustweaker::engine::package_import::PackageImportResult::Planned { plan, .. }
            if plan.edge_filesystem_action_present => vec![
                "upstream uninstall runs New-Item -Force at %SystemRoot%\\SystemApps\\Microsoft.MicrosoftEdge_8wekyb3d8bbwe\\MicrosoftEdge.exe; this plan performs no filesystem action",
            ],
        _ => Vec::new(),
    };
    serde_json::json!({
        "report_kind": "selection_package_import_plan",
        "basis": "user_supplied_selection_json",
        "catalog_coverage": "preview_subset_only",
        "executable": false,
        "upstream_side_effect_warnings": warnings,
        "result": result,
    })
}

fn read_bounded_selection_import<R: std::io::Read>(reader: R) -> Result<Vec<u8>, String> {
    let max_bytes = rustweaker::data::selection_import::MAX_SELECTION_IMPORT_BYTES;
    let mut input = Vec::new();
    reader
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|error| format!("could not read selection import input: {error}"))?;
    if input.len() > max_bytes {
        return Err(format!(
            "selection import input exceeds the {max_bytes}-byte limit"
        ));
    }
    Ok(input)
}

fn decode_selection_stdin() -> Result<(), String> {
    let input = read_bounded_selection_import(std::io::stdin().lock())?;
    let report = decode_selection_preview_report(&input)?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn decode_selection_preview_report(input: &[u8]) -> Result<Value, String> {
    use rustweaker::data::selection_import::{
        decode_selection_import, NoChangesReason, SelectionImportResult,
    };

    let catalog = rustweaker::data::selection_catalog::embedded_selection_catalog()
        .map_err(|error| format!("selection catalog failed validation: {error}"))?;
    let decoded = decode_selection_import(input, &catalog.known_ids())
        .map_err(|error| format!("selection decode failed: {error:?}"))?;
    let report = match decoded {
        SelectionImportResult::Replace {
            format,
            selections,
            skipped_legacy,
        } => serde_json::json!({
            "outcome": "replace_preview",
            "format": match format {
                rustweaker::data::selection_import::SelectionImportFormat::Modern => "modern",
                rustweaker::data::selection_import::SelectionImportFormat::Legacy => "legacy",
            },
            "selections": {
                "apps": selections.apps,
                "tweaks": selections.tweaks,
                "toggles": selections.toggles,
                "features": selections.features,
                "appx": selections.appx
            },
            "skipped_legacy": skipped_legacy,
            "catalog_validation": "source_derived_selection_ids",
            "executable": false
        }),
        SelectionImportResult::NoChanges(reason) => serde_json::json!({
            "outcome": "no_changes",
            "reason": match &reason {
                NoChangesReason::EmptyConfiguration => "empty_configuration",
                NoChangesReason::NoSupportedLegacySelections { .. } => "no_supported_legacy_selections",
            },
            "skipped_legacy": match reason {
                NoChangesReason::EmptyConfiguration => Vec::<String>::new(),
                NoChangesReason::NoSupportedLegacySelections { skipped } => skipped,
            },
            "catalog_validation": "source_derived_selection_ids",
            "executable": false
        }),
    };
    Ok(report)
}

fn export_selection(ids: &[String]) -> Result<(), String> {
    let json = selection_export_json(ids)?;
    println!("{json}");
    Ok(())
}

fn selection_export_json(ids: &[String]) -> Result<String, String> {
    let catalog = rustweaker::data::selection_catalog::embedded_selection_catalog()
        .map_err(|error| format!("selection catalog failed validation: {error}"))?;
    let input = serde_json::to_vec(ids).map_err(|error| error.to_string())?;
    let decoded =
        rustweaker::data::selection_import::decode_selection_import(&input, &catalog.known_ids())
            .map_err(|error| format!("selection export rejected: {error:?}"))?;
    let rustweaker::data::selection_import::SelectionImportResult::Replace { selections, .. } =
        decoded
    else {
        return Err("selection export requires a non-empty ID list".into());
    };
    rustweaker::data::selection_export::encode_selection_export(&selections)
        .map_err(|error| format!("selection export failed: {error:?}"))
}

fn preset_preview_stdin(name: &str, filter_pattern: &str) -> Result<(), String> {
    let input = read_bounded_selection_import(std::io::stdin().lock())?;
    let report = preset_preview_report(name, filter_pattern, &input)?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn preset_preview_report(name: &str, filter_pattern: &str, input: &[u8]) -> Result<Value, String> {
    use rustweaker::data::selection_import::{
        decode_selection_import, NoChangesReason, SelectionImportResult,
    };
    use rustweaker::data::selection_state::{PresetCategory, SelectionState};

    let catalog = rustweaker::data::selection_catalog::embedded_selection_catalog()
        .map_err(|error| format!("selection catalog failed validation: {error}"))?;
    let preset = catalog
        .preset(name)
        .ok_or_else(|| format!("unknown preset name: {name}"))?;
    let decoded = decode_selection_import(input, &catalog.known_ids())
        .map_err(|error| format!("selection import failed: {error:?}"))?;

    let mut state = SelectionState::default();
    let skipped_legacy = match &decoded {
        SelectionImportResult::Replace { skipped_legacy, .. } => skipped_legacy.clone(),
        SelectionImportResult::NoChanges(NoChangesReason::NoSupportedLegacySelections {
            skipped,
        }) => skipped.clone(),
        SelectionImportResult::NoChanges(NoChangesReason::EmptyConfiguration) => Vec::new(),
    };
    state.apply_import(&decoded);
    let scope = match filter_pattern.to_ascii_lowercase().as_str() {
        "wpftweak*" | "wpftweaks*" => PresetCategory::Tweaks,
        "wpfinstall*" => PresetCategory::Apps,
        "wpfappx*" => PresetCategory::Appx,
        "wpffeature*" => PresetCategory::Features,
        "wpftoggle*" => PresetCategory::Toggles,
        _ => PresetCategory::Merge,
    };
    state
        .apply_preset(preset, scope, &catalog.known_ids())
        .map_err(|error| format!("preset could not be applied to selection preview: {error:?}"))?;

    Ok(serde_json::json!({
        "outcome": "preset_preview",
        "preset": name,
        "filter_pattern": filter_pattern,
        "selections": {
            "apps": state.apps,
            "tweaks": state.tweaks,
            "toggles": state.toggles,
            "features": state.features,
            "appx": state.appx
        },
        "skipped_legacy": skipped_legacy,
        "catalog_validation": "source_derived_selection_ids",
        "executable": false
    }))
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ReportOutcome {
    Success { data: Value },
    Error { error: String },
}

#[derive(Debug, Serialize)]
struct ServiceReportEntry {
    name: String,
    query: ReportOutcome,
    configuration: ReportOutcome,
}

fn to_report_outcome<T: Serialize>(result: Result<T, String>) -> ReportOutcome {
    match result {
        Ok(value) => match serde_json::to_value(value) {
            Ok(data) => ReportOutcome::Success { data },
            Err(error) => ReportOutcome::Error {
                error: format!("result serialization failed: {error}"),
            },
        },
        Err(error) => ReportOutcome::Error { error },
    }
}

fn aggregate_services_report<Q, C, QE, CE>(
    names: &[String],
    mut query: impl FnMut(&str) -> Result<Q, QE>,
    mut query_config: impl FnMut(&str) -> Result<C, CE>,
) -> Vec<ServiceReportEntry>
where
    Q: Serialize,
    C: Serialize,
    QE: Debug,
    CE: Debug,
{
    names
        .iter()
        .map(|name| ServiceReportEntry {
            name: name.clone(),
            query: to_report_outcome(query(name).map_err(|error| format!("{error:?}"))),
            configuration: to_report_outcome(
                query_config(name).map_err(|error| format!("{error:?}")),
            ),
        })
        .collect()
}

#[cfg(windows)]
fn services_report(names: &[String]) -> Result<(), String> {
    let report = aggregate_services_report(
        names,
        rustweaker::engine::services::query,
        rustweaker::engine::services::query_config,
    );
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}
#[cfg(not(windows))]
fn services_report(_: &[String]) -> Result<(), String> {
    Err("service queries are available only on Windows".into())
}

#[cfg(windows)]
fn winsock_report() -> Result<(), String> {
    let catalogs = rustweaker::engine::winsock_catalog::capture()
        .map_err(|error| format!("Winsock catalog query failed: {error:?}"))?;
    println!(
        "{}",
        serde_json::to_string(&catalogs).map_err(|error| error.to_string())?
    );
    Ok(())
}
#[cfg(not(windows))]
fn winsock_report() -> Result<(), String> {
    Err("Winsock catalog queries are available only on Windows".into())
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
    use std::cell::Cell;
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

    #[test]
    fn parses_read_only_application_catalog_commands() {
        assert_eq!(
            parse(&args(&["--applications-list"])),
            Ok(Command::ApplicationsList)
        );
        assert_eq!(
            parse(&args(&["--application-show", "7zip"])),
            Ok(Command::ApplicationShow("7zip".into()))
        );
        assert!(parse(&args(&["--application-show"])).is_err());
        assert!(parse(&args(&["--application-show", ""])).is_err());
        assert_eq!(
            parse(&args(&["--export-app-selection", "7zip", "edge"])),
            Ok(Command::ExportApplicationSelection(vec![
                "7zip".into(),
                "edge".into()
            ]))
        );
        assert!(parse(&args(&["--export-app-selection"])).is_err());
    }

    #[test]
    fn parses_generic_selection_decode_and_validates_source_selection_ids() {
        assert_eq!(
            parse(&args(&["--decode-selection"])),
            Ok(Command::DecodeSelection)
        );
        assert!(parse(&args(&["--decode-selection", "extra"])).is_err());
        let catalog = rustweaker::data::selection_catalog::embedded_selection_catalog().unwrap();
        let ids = [
            catalog.ids().apps[0].clone(),
            catalog.ids().tweaks[0].clone(),
            catalog.ids().features[0].clone(),
        ];
        let input = serde_json::to_vec(&ids).unwrap();
        let report = decode_selection_preview_report(&input).unwrap();
        assert_eq!(report["outcome"], "replace_preview");
        assert_eq!(report["selections"]["apps"][0], ids[0]);
        assert_eq!(report["selections"]["tweaks"][0], ids[1]);
        assert_eq!(report["selections"]["features"][0], ids[2]);
        assert_eq!(report["catalog_validation"], "source_derived_selection_ids");
        assert_eq!(report["executable"], false);
        assert!(decode_selection_preview_report(br#"["WPFTweaksNotARealId"]"#).is_err());
    }

    #[test]
    fn exports_all_selection_families_and_previews_presets_without_execution() {
        assert_eq!(
            parse(&args(&[
                "--export-selection",
                "WPFInstall7zip",
                "WPFTweaksActivity"
            ])),
            Ok(Command::ExportSelection(vec![
                "WPFInstall7zip".into(),
                "WPFTweaksActivity".into()
            ]))
        );
        assert!(parse(&args(&["--export-selection"])).is_err());
        assert_eq!(
            parse(&args(&["--preset-preview", "Standard", "WPFTweak*"])),
            Ok(Command::PresetPreview {
                name: "Standard".into(),
                filter: "WPFTweak*".into()
            })
        );
        let catalog = rustweaker::data::selection_catalog::embedded_selection_catalog().unwrap();
        let initial = serde_json::to_vec(&vec![catalog.ids().apps[0].clone()]).unwrap();
        let report = preset_preview_report("Standard", "WPFTweak*", &initial).unwrap();
        assert_eq!(report["outcome"], "preset_preview");
        assert_eq!(report["selections"]["apps"][0], catalog.ids().apps[0]);
        assert_eq!(report["selections"]["tweaks"].as_array().unwrap().len(), 12);
        assert_eq!(report["executable"], false);

        let exported = selection_export_json(&[
            catalog.ids().apps[0].clone(),
            catalog.ids().features[0].clone(),
        ])
        .unwrap();
        let imported = decode_selection_preview_report(exported.as_bytes()).unwrap();
        assert_eq!(imported["outcome"], "replace_preview");
        assert_eq!(imported["selections"]["apps"][0], catalog.ids().apps[0]);
        assert_eq!(
            imported["selections"]["features"][0],
            catalog.ids().features[0]
        );
    }

    #[test]
    fn parses_read_only_installed_registry_inventory_command() {
        assert_eq!(
            parse(&args(&["--installed-registry-apps"])),
            Ok(Command::InstalledRegistryApps)
        );
        assert!(parse(&args(&["--installed-registry-apps", "extra"])).is_err());
    }

    #[test]
    fn parses_source_audited_toggle_status_with_explicit_registry_view() {
        assert_eq!(
            parse(&args(&["--toggle-status", "WPFToggleDetailedBSoD", "64"])),
            Ok(Command::ToggleStatus {
                id: "WPFToggleDetailedBSoD".into(),
                view: RegistryView::Registry64,
            })
        );
        assert!(parse(&args(&["--toggle-status", "unknown", "64"])).is_err());
        assert!(parse(&args(&["--toggle-status", "WPFToggleDetailedBSoD", "bad"])).is_err());
        assert_eq!(
            parse(&args(&["--toggle-status", "WPFToggleNumLock", "32"])),
            Ok(Command::ToggleStatus {
                id: "WPFToggleNumLock".into(),
                view: RegistryView::Registry32,
            })
        );
        assert_eq!(
            parse(&args(&["--toggle-status", "wpftogglenumlock", "native"])),
            Ok(Command::ToggleStatus {
                id: "WPFToggleNumLock".into(),
                view: RegistryView::Native,
            })
        );
        assert_eq!(
            parse(&args(&[
                "--toggle-status",
                "WPFToggleWindowSnapping",
                "native"
            ])),
            Ok(Command::ToggleStatus {
                id: "WPFToggleWindowSnapping".into(),
                view: RegistryView::Native,
            })
        );
    }

    #[test]
    fn parses_read_only_utc_status_with_explicit_registry_view() {
        assert_eq!(
            parse(&args(&["--utc-status", "64"])),
            Ok(Command::UtcStatus(RegistryView::Registry64))
        );
        assert_eq!(
            parse(&args(&["--utc-status", "native"])),
            Ok(Command::UtcStatus(RegistryView::Native))
        );
        assert!(parse(&args(&["--utc-status", "bad"])).is_err());
        assert!(parse(&args(&["--utc-status", "64", "extra"])).is_err());
    }

    #[test]
    fn parses_read_only_location_status_with_explicit_registry_view() {
        assert_eq!(
            parse(&args(&["--location-status", "64"])),
            Ok(Command::LocationStatus(RegistryView::Registry64))
        );
        assert_eq!(
            parse(&args(&["--location-status", "native"])),
            Ok(Command::LocationStatus(RegistryView::Native))
        );
        assert!(parse(&args(&["--location-status", "bad"])).is_err());
        assert!(parse(&args(&["--location-status", "32", "extra"])).is_err());
    }

    #[test]
    fn application_selection_export_roundtrips_into_the_package_planner() {
        let catalog = rustweaker::data::applications::embedded_applications().unwrap();
        let encoded = encode_application_selection(&catalog, &["7zip".into()]).unwrap();
        assert_eq!(encoded, r#""WPFInstall7zip""#);
        let imported = rustweaker::engine::package_import::plan_package_import(
            encoded.as_bytes(),
            &catalog,
            rustweaker::engine::package_plan::PackagePreference::Winget,
            rustweaker::engine::package_plan::PackageAction::Install,
        )
        .unwrap();
        assert!(matches!(
            imported,
            rustweaker::engine::package_import::PackageImportResult::Planned {
                selected_catalog_ids,
                ..
            } if selected_catalog_ids == ["7zip"]
        ));
    }

    #[test]
    fn application_selection_export_rejects_invalid_ids_before_returning_json() {
        let catalog = rustweaker::data::applications::embedded_applications().unwrap();
        assert!(encode_application_selection(&catalog, &[]).is_err());
        assert!(
            encode_application_selection(&catalog, &["7zip".into(), "missing".into()]).is_err()
        );
    }

    #[test]
    fn parses_text_only_chocolatey_list_matching_command() {
        assert_eq!(
            parse(&args(&["--match-choco-list"])),
            Ok(Command::MatchChocolateyList)
        );
        assert!(parse(&args(&["--match-choco-list", "extra"])).is_err());
    }

    #[test]
    fn parses_text_only_winget_list_matching_command() {
        assert_eq!(
            parse(&args(&["--match-winget-list"])),
            Ok(Command::MatchWingetList)
        );
        assert!(parse(&args(&["--match-winget-list", "extra"])).is_err());
    }

    #[test]
    fn chocolatey_list_stdin_read_is_bounded() {
        let max = rustweaker::engine::package_state::MAX_CHOCOLATEY_LIST_BYTES;
        assert_eq!(
            read_bounded_choco_input(std::io::Cursor::new(vec![b'x'; max]))
                .unwrap()
                .len(),
            max
        );
        assert!(read_bounded_choco_input(std::io::Cursor::new(vec![b'x'; max + 1])).is_err());
    }

    #[test]
    fn parses_read_only_winsock_report_command() {
        assert_eq!(
            parse(&args(&["--winsock-report"])),
            Ok(Command::WinsockReport)
        );
        assert!(parse(&args(&["--winsock-report", "extra"])).is_err());
    }

    #[test]
    fn parses_package_plan_without_enabling_installation() {
        use rustweaker::engine::package_plan::PackagePreference;

        assert_eq!(
            parse(&args(&["--plan-install", "choco", "7zip", "mpv"])),
            Ok(Command::PlanPackages {
                action: rustweaker::engine::package_plan::PackageAction::Install,
                preference: PackagePreference::Choco,
                catalog_ids: vec!["7zip".into(), "mpv".into()],
            })
        );
        assert_eq!(
            parse(&args(&["--plan-uninstall", "winget", "7zip"])),
            Ok(Command::PlanPackages {
                action: rustweaker::engine::package_plan::PackageAction::Uninstall,
                preference: PackagePreference::Winget,
                catalog_ids: vec!["7zip".into()],
            })
        );
        assert!(parse(&args(&["--plan-install", "winget"])).is_err());
        assert!(parse(&args(&["--plan-install", "auto", "7zip"])).is_err());
        assert!(parse(&args(&["--plan-uninstall", "winget"])).is_err());
    }

    #[test]
    fn parses_global_upgrade_plan_without_enabling_execution() {
        use rustweaker::engine::package_plan::PackagePreference;

        assert_eq!(
            parse(&args(&["--plan-upgrade-all", "choco"])),
            Ok(Command::PlanUpgradeAll {
                preference: PackagePreference::Choco,
            })
        );
        assert_eq!(
            parse(&args(&["--plan-upgrade-all", "winget"])),
            Ok(Command::PlanUpgradeAll {
                preference: PackagePreference::Winget,
            })
        );
        assert!(parse(&args(&["--plan-upgrade-all"])).is_err());
        assert!(parse(&args(&["--plan-upgrade-all", "winget", "7zip"])).is_err());
        assert!(parse(&args(&["--plan-upgrade-all", "auto"])).is_err());
    }

    #[test]
    fn parses_selection_import_plan_without_enabling_package_actions() {
        use rustweaker::engine::package_plan::{PackageAction, PackagePreference};

        assert_eq!(
            parse(&args(&["--plan-import", "CHoco", "UnInStAlL"])),
            Ok(Command::PlanImportedPackages {
                preference: PackagePreference::Choco,
                action: PackageAction::Uninstall,
            })
        );
        assert_eq!(
            parse(&args(&["--plan-import", "winget", "install"])),
            Ok(Command::PlanImportedPackages {
                preference: PackagePreference::Winget,
                action: PackageAction::Install,
            })
        );
        assert!(parse(&args(&["--plan-import", "winget", "repair"])).is_err());
        assert!(parse(&args(&["--plan-import", "auto", "install"])).is_err());
        assert!(parse(&args(&["--plan-import", "winget"])).is_err());
    }

    #[test]
    fn selection_import_stdin_reader_enforces_the_json_byte_limit() {
        let max = rustweaker::data::selection_import::MAX_SELECTION_IMPORT_BYTES;
        assert_eq!(
            read_bounded_selection_import(std::io::Cursor::new(vec![b' '; max]))
                .unwrap()
                .len(),
            max
        );
        assert!(read_bounded_selection_import(std::io::Cursor::new(vec![b' '; max + 1])).is_err());
    }

    #[test]
    fn imported_edge_uninstall_report_includes_source_side_effect_warning() {
        let catalog = rustweaker::data::applications::embedded_applications().unwrap();
        let result = rustweaker::engine::package_import::plan_package_import(
            br#"["WPFInstallEdge"]"#,
            &catalog,
            rustweaker::engine::package_plan::PackagePreference::Winget,
            rustweaker::engine::package_plan::PackageAction::Uninstall,
        )
        .unwrap();
        let report = package_import_report(result);
        assert_eq!(report["executable"], false);
        assert_eq!(report["catalog_coverage"], "preview_subset_only");
        let warning = report
            .get("upstream_side_effect_warnings")
            .and_then(Value::as_array)
            .and_then(|warnings| warnings.first())
            .and_then(Value::as_str)
            .unwrap();
        assert!(warning.contains("New-Item -Force"));
    }

    #[test]
    fn services_report_accepts_one_to_thirty_two_valid_names() {
        assert_eq!(
            parse(&args(&["--services-report", "EventLog"])),
            Ok(Command::ServicesReport(vec!["EventLog".into()]))
        );
        let mut max_args = vec!["--services-report".to_owned()];
        max_args.extend((0..32).map(|index| format!("Service{index}")));
        assert!(
            matches!(parse(&max_args), Ok(Command::ServicesReport(names)) if names.len() == 32)
        );
    }

    #[test]
    fn services_report_rejects_empty_over_limit_and_invalid_names() {
        assert!(parse(&args(&["--services-report"])).is_err());
        let mut over_limit = vec!["--services-report".to_owned()];
        over_limit.extend((0..33).map(|index| format!("Service{index}")));
        assert!(parse(&over_limit).is_err());
        assert!(parse(&args(&["--services-report", "Bad/Name"])).is_err());
    }

    #[test]
    fn services_report_aggregates_each_query_independently() {
        let names = args(&["First", "Second"]);
        let query_calls = Cell::new(0);
        let config_calls = Cell::new(0);
        let report = aggregate_services_report(
            &names,
            |name| {
                query_calls.set(query_calls.get() + 1);
                if name == "First" {
                    Err("status denied")
                } else {
                    Ok(serde_json::json!({"state": "running"}))
                }
            },
            |name| {
                config_calls.set(config_calls.get() + 1);
                if name == "Second" {
                    Err("config unavailable")
                } else {
                    Ok(serde_json::json!({"start_type": 3}))
                }
            },
        );

        assert_eq!(report.len(), 2);
        assert_eq!(query_calls.get(), 2);
        assert_eq!(config_calls.get(), 2);
        assert_eq!(report[0].name, "First");
        assert!(matches!(report[0].query, ReportOutcome::Error { .. }));
        assert!(matches!(
            report[0].configuration,
            ReportOutcome::Success { .. }
        ));
        assert_eq!(report[1].name, "Second");
        assert!(matches!(report[1].query, ReportOutcome::Success { .. }));
        assert!(matches!(
            report[1].configuration,
            ReportOutcome::Error { .. }
        ));
    }
}
