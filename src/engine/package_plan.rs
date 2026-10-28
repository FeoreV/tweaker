//! Read-only package selection and manager planning from the application catalog.
//!
//! This ports the data-only part of `winutil-26.08.19/functions/private/Get-WinUtilSelectedPackages.ps1`.
//! It preserves selected order and package-ID strings, including values containing
//! semicolons. It does not invoke WinGet, Chocolatey, a shell, the network, or any
//! system mutation. A plan is not evidence that an application was installed.
//! Chocolatey planning preserves package IDs as a typed request. The upstream
//! `Install-WinUtilProgramChoco.ps1` interpolates its `string[]` into one
//! `Start-Process` argument string under ambient `$OFS`; this representation is
//! intent data, not equivalent argv. In particular, semicolon-containing IDs
//! remain raw and their Chocolatey interpretation is unproven.
//! WinGet target normalization mirrors the source parsing in
//! `winutil-26.08.19/functions/private/Install-WinUtilProgramWinget.ps1` (SHA-256
//! `CB3C857CF35160E163E16B0585EC7A4C9AC50995C97FE7E07894534D2D7DF936`):
//! `msstore:` is a case-insensitive prefix and exactly eight bytes are stripped.

use crate::data::applications::ApplicationCatalog;
use serde::Serialize;
use std::fmt;

/// Preferred package manager from the upstream install workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackagePreference {
    Choco,
    Winget,
}

impl PackagePreference {
    /// Parse the preference spellings used by the upstream PowerShell switch.
    pub fn parse(value: &str) -> Result<Self, PackagePlanError> {
        if value.eq_ignore_ascii_case("choco") {
            Ok(Self::Choco)
        } else if value.eq_ignore_ascii_case("winget") {
            Ok(Self::Winget)
        } else {
            Err(PackagePlanError::InvalidPreference(value.to_owned()))
        }
    }
}

/// Package manager selected for an application.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    Choco,
    Winget,
}

/// A global package-manager upgrade request. This is data only: it contains no
/// command text or argv and cannot execute the operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UpgradeAllPlan {
    pub operation: UpgradeAllOperation,
    /// Records the upstream bootstrap step as intent metadata only.
    pub bootstrap_intent: UpgradeAllBootstrapIntent,
    /// Explicitly identifies this model as non-executable intent data.
    pub execution: UpgradeAllExecution,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "manager", content = "options")]
pub enum UpgradeAllOperation {
    Chocolatey(ChocolateyUpgradeAllOptions),
    Winget(WingetUpgradeAllOptions),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeAllBootstrapIntent {
    ChocolateyIfSelected,
    WingetIfSelected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeAllExecution {
    DataOnlyNonExecutable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ChocolateyUpgradeAllOptions {
    pub assume_yes: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct WingetUpgradeAllOptions {
    pub powershell_no_exit: bool,
    pub include_unknown: bool,
    pub silent: bool,
    pub accept_source_agreements: bool,
    pub accept_package_agreements: bool,
}

/// Build the global Upgrade All intent. It is independent of catalog
/// selections and does not bootstrap a manager or run a subprocess.
pub fn build_upgrade_all_plan(preference: PackagePreference) -> UpgradeAllPlan {
    let (operation, bootstrap_intent) = match preference {
        PackagePreference::Choco => (
            UpgradeAllOperation::Chocolatey(ChocolateyUpgradeAllOptions { assume_yes: true }),
            UpgradeAllBootstrapIntent::ChocolateyIfSelected,
        ),
        PackagePreference::Winget => (
            UpgradeAllOperation::Winget(WingetUpgradeAllOptions {
                powershell_no_exit: true,
                include_unknown: true,
                silent: true,
                accept_source_agreements: true,
                accept_package_agreements: true,
            }),
            UpgradeAllBootstrapIntent::WingetIfSelected,
        ),
    };
    UpgradeAllPlan {
        operation,
        bootstrap_intent,
        execution: UpgradeAllExecution::DataOnlyNonExecutable,
    }
}

/// Requested package operation. This records intent only; it is never executed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageAction {
    #[default]
    Install,
    Uninstall,
}

/// Why one selected catalog entry did or did not add an ID to the plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionDisposition {
    Added,
    Duplicate,
    SkippedBlank,
    SkippedUnavailable,
}

/// Per-selection explanation, retained in the original selection order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PackageDecision {
    pub catalog_id: String,
    pub preferred_manager: PackagePreference,
    pub selected_manager: PackageManager,
    /// True when Chocolatey preference fell back to WinGet due to a missing,
    /// blank, or `na` Chocolatey ID.
    pub fallback_to_winget: bool,
    /// Raw source value, including `na`/blank sentinels for display only.
    pub source_package_id: String,
    pub disposition: DecisionDisposition,
}

/// Ordered, deduplicated IDs and the decision for each selected catalog item.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PackagePlan {
    pub action: PackageAction,
    /// Upstream Edge uninstall runs `New-Item -Force` at the Microsoft Edge
    /// SystemApps path before invoking the package helper. This reports that
    /// source-level filesystem action as metadata only; it does not claim the
    /// command succeeds or perform the action.
    pub edge_filesystem_action_present: bool,
    pub winget: Vec<String>,
    /// Typed targets derived from `winget`, without changing the raw IDs above.
    pub winget_targets: Vec<WingetTarget>,
    pub choco: Vec<String>,
    /// Data-only install intent; absent when no Chocolatey IDs were selected.
    pub chocolatey_request: Option<ChocolateyRequest>,
    pub decisions: Vec<PackageDecision>,
}

/// Chocolatey action represented by a data-only request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChocolateyAction {
    Install,
    Uninstall,
}

/// Data-only Chocolatey install request. It is not command text or argv and
/// is never executed by this module.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChocolateyRequest {
    pub action: ChocolateyAction,
    /// Ordered, exact, deduplicated raw package IDs; semicolons are preserved.
    pub package_ids: Vec<String>,
    /// Mirrors the upstream helper's `-y` option.
    pub assume_yes: bool,
}

/// WinGet source selected for a planned package ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WingetSource {
    Winget,
    MsStore,
}

/// Normalized, data-only WinGet invocation target. This is not argv and is
/// never executed by this module.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WingetTarget {
    pub raw_id: String,
    pub package_id: String,
    pub source: WingetSource,
}

/// Failure while resolving a package plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackagePlanError {
    InvalidPreference(String),
    UnknownCatalogId(String),
    EmptyMsStoreId(String),
}

impl fmt::Display for PackagePlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPreference(value) => write!(f, "unsupported package preference {value:?}"),
            Self::UnknownCatalogId(id) => write!(f, "unknown application catalog ID {id:?}"),
            Self::EmptyMsStoreId(id) => write!(
                f,
                "Microsoft Store package ID is empty after removing the msstore: prefix from {id:?}"
            ),
        }
    }
}

impl std::error::Error for PackagePlanError {}

/// Build an install plan from ordered application catalog IDs.
///
/// This compatibility wrapper preserves the original install behavior.
pub fn build_package_plan(
    catalog: &ApplicationCatalog,
    selected_ids: &[String],
    preference: PackagePreference,
) -> Result<PackagePlan, PackagePlanError> {
    build_package_plan_for_action(catalog, selected_ids, preference, PackageAction::Install)
}

/// Build a data-only package plan for the requested action.
///
/// Under Chocolatey preference, an absent, whitespace-only, or case-insensitive
/// `na` Chocolatey ID falls back to WinGet, matching the upstream function.
/// Package IDs are added once per manager using exact, case-sensitive string
/// equality, matching the source `ArrayList.Contains` check. Unknown catalog IDs
/// fail the whole call; no partial plan is returned.
pub fn build_package_plan_for_action(
    catalog: &ApplicationCatalog,
    selected_ids: &[String],
    preference: PackagePreference,
    action: PackageAction,
) -> Result<PackagePlan, PackagePlanError> {
    let mut plan = PackagePlan {
        action,
        edge_filesystem_action_present: false,
        ..PackagePlan::default()
    };

    for catalog_id in selected_ids {
        let entry = catalog
            .get(catalog_id)
            .ok_or_else(|| PackagePlanError::UnknownCatalogId(catalog_id.clone()))?;

        let (manager, source_id, fallback) = match preference {
            PackagePreference::Choco => match entry.choco.as_deref() {
                Some(choco_id) if !is_unavailable(choco_id) => {
                    (PackageManager::Choco, choco_id, false)
                }
                _ => (PackageManager::Winget, entry.winget.as_str(), true),
            },
            PackagePreference::Winget => (PackageManager::Winget, entry.winget.as_str(), false),
        };

        let target = match manager {
            PackageManager::Choco => &mut plan.choco,
            PackageManager::Winget => &mut plan.winget,
        };
        let disposition = add_package_id(target, source_id);

        if action == PackageAction::Uninstall
            && manager == PackageManager::Winget
            && source_id.eq_ignore_ascii_case("Microsoft.Edge")
        {
            plan.edge_filesystem_action_present = true;
        }

        plan.decisions.push(PackageDecision {
            catalog_id: catalog_id.clone(),
            preferred_manager: preference,
            selected_manager: manager,
            fallback_to_winget: fallback,
            source_package_id: source_id.to_owned(),
            disposition,
        });
    }

    plan.winget_targets = plan
        .winget
        .iter()
        .map(|raw_id| normalize_winget_target(raw_id))
        .collect::<Result<Vec<_>, _>>()?;
    plan.chocolatey_request = if plan.choco.is_empty() {
        None
    } else {
        Some(ChocolateyRequest {
            action: match action {
                PackageAction::Install => ChocolateyAction::Install,
                PackageAction::Uninstall => ChocolateyAction::Uninstall,
            },
            package_ids: plan.choco.clone(),
            assume_yes: true,
        })
    };

    Ok(plan)
}

fn normalize_winget_target(raw_id: &str) -> Result<WingetTarget, PackagePlanError> {
    const PREFIX: &str = "msstore:";
    if raw_id
        .get(..PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
    {
        let package_id = &raw_id[PREFIX.len()..];
        if package_id.trim().is_empty() {
            return Err(PackagePlanError::EmptyMsStoreId(raw_id.to_owned()));
        }
        Ok(WingetTarget {
            raw_id: raw_id.to_owned(),
            package_id: package_id.to_owned(),
            source: WingetSource::MsStore,
        })
    } else {
        Ok(WingetTarget {
            raw_id: raw_id.to_owned(),
            package_id: raw_id.to_owned(),
            source: WingetSource::Winget,
        })
    }
}

fn is_not_available(value: &str) -> bool {
    value.eq_ignore_ascii_case("na")
}

fn is_unavailable(value: &str) -> bool {
    value.trim().is_empty() || is_not_available(value)
}

/// Mirror upstream `Add-PackageId`: skip blank/`na` values and append unique
/// IDs using exact string equality.
fn add_package_id(target: &mut Vec<String>, package_id: &str) -> DecisionDisposition {
    if package_id.trim().is_empty() {
        DecisionDisposition::SkippedBlank
    } else if is_not_available(package_id) {
        DecisionDisposition::SkippedUnavailable
    } else if target.iter().any(|existing| existing == package_id) {
        DecisionDisposition::Duplicate
    } else {
        target.push(package_id.to_owned());
        DecisionDisposition::Added
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::applications::{decode_applications, embedded_applications};

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn fixture_catalog() -> ApplicationCatalog {
        decode_applications(
            br#"{
                "one": {"category":"A","content":"one","description":"One","link":"https://example.test/one","winget":"Same.Id","choco":"Same.Id","foss":true},
                "two": {"category":"A","content":"two","description":"Two","link":"https://example.test/two","winget":"Other.Id","choco":"Same.Id","foss":true},
                "three": {"category":"A","content":"three","description":"Three","link":"https://example.test/three","winget":"same.id","foss":true},
                "four": {"category":"A","content":"four","description":"Four","link":"https://example.test/four","winget":"SAME.ID","foss":true},
                "blank": {"category":"A","content":"blank","description":"Blank","link":"https://example.test/blank","winget":"Fallback.From.Blank","choco":" \t ","foss":true},
                "sentinel": {"category":"A","content":"sentinel","description":"Sentinel","link":"https://example.test/sentinel","winget":"nA","choco":"NA","foss":true},
                "missing_choco": {"category":"A","content":"missing","description":"Missing","link":"https://example.test/missing","winget":"Fallback.Id","foss":true},
                "store_mixed": {"category":"A","content":"store","description":"Store","link":"https://example.test/store","winget":"mSStoRe:Store.Package.ID","foss":true},
                "store_empty": {"category":"A","content":"store-empty","description":"Store empty","link":"https://example.test/store-empty","winget":"msstore:","foss":true},
                "winget_normal": {"category":"A","content":"normal","description":"Normal","link":"https://example.test/normal","winget":"Normal.Package.ID","foss":true},
                "semi": {"category":"A","content":"semi","description":"Semicolon ID","link":"https://example.test/semi","winget":"Semi.Winget","choco":"git;github-desktop","foss":true}
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn upgrade_all_chocolatey_plan_records_options_and_bootstrap_intent() {
        assert_eq!(
            build_upgrade_all_plan(PackagePreference::Choco),
            UpgradeAllPlan {
                operation: UpgradeAllOperation::Chocolatey(ChocolateyUpgradeAllOptions {
                    assume_yes: true,
                }),
                bootstrap_intent: UpgradeAllBootstrapIntent::ChocolateyIfSelected,
                execution: UpgradeAllExecution::DataOnlyNonExecutable,
            }
        );
    }

    #[test]
    fn upgrade_all_winget_plan_records_all_source_options_and_bootstrap_intent() {
        assert_eq!(
            build_upgrade_all_plan(PackagePreference::Winget),
            UpgradeAllPlan {
                operation: UpgradeAllOperation::Winget(WingetUpgradeAllOptions {
                    powershell_no_exit: true,
                    include_unknown: true,
                    silent: true,
                    accept_source_agreements: true,
                    accept_package_agreements: true,
                }),
                bootstrap_intent: UpgradeAllBootstrapIntent::WingetIfSelected,
                execution: UpgradeAllExecution::DataOnlyNonExecutable,
            }
        );
    }

    #[test]
    fn real_entries_preserve_fallback_and_semicolon_package_ids() {
        let catalog = embedded_applications().unwrap();
        let plan = build_package_plan(
            &catalog,
            &ids(&["7zip", "mpv", "battlenet", "es-de", "githubdesktop"]),
            PackagePreference::Choco,
        )
        .unwrap();

        assert_eq!(plan.choco, ["7zip", "git;github-desktop"]);
        assert_eq!(
            plan.winget,
            [
                "shinchiro.mpv",
                "Blizzard.BattleNet",
                "ES-DE.EmulationStation-DE"
            ]
        );
        assert_eq!(plan.decisions.len(), 5);
        assert!(!plan.decisions[0].fallback_to_winget);
        assert!(plan.decisions[1..4]
            .iter()
            .all(|decision| decision.fallback_to_winget));
        assert_eq!(plan.decisions[4].source_package_id, "git;github-desktop");
        assert!(!plan.decisions[4].fallback_to_winget);
        let request = plan.chocolatey_request.as_ref().unwrap();
        assert_eq!(request.action, ChocolateyAction::Install);
        assert_eq!(request.package_ids, ["7zip", "git;github-desktop"]);
        assert!(request.assume_yes);
    }

    #[test]
    fn real_microsoft_store_entry_keeps_raw_id_and_extracts_store_target() {
        let catalog = embedded_applications().unwrap();
        let plan =
            build_package_plan(&catalog, &ids(&["chatgpt"]), PackagePreference::Choco).unwrap();

        assert_eq!(plan.winget, ["msstore:9NT1R1C2HH7J"]);
        assert_eq!(
            plan.winget_targets,
            [WingetTarget {
                raw_id: "msstore:9NT1R1C2HH7J".into(),
                package_id: "9NT1R1C2HH7J".into(),
                source: WingetSource::MsStore,
            }]
        );
        assert!(plan.decisions[0].fallback_to_winget);
    }

    #[test]
    fn mixed_case_msstore_prefix_is_stripped_exactly_and_normal_ids_stay_unchanged() {
        let catalog = fixture_catalog();
        let plan = build_package_plan(
            &catalog,
            &ids(&["store_mixed", "winget_normal"]),
            PackagePreference::Winget,
        )
        .unwrap();

        assert_eq!(
            plan.winget,
            ["mSStoRe:Store.Package.ID", "Normal.Package.ID"]
        );
        assert_eq!(
            plan.winget_targets,
            [
                WingetTarget {
                    raw_id: "mSStoRe:Store.Package.ID".into(),
                    package_id: "Store.Package.ID".into(),
                    source: WingetSource::MsStore,
                },
                WingetTarget {
                    raw_id: "Normal.Package.ID".into(),
                    package_id: "Normal.Package.ID".into(),
                    source: WingetSource::Winget,
                },
            ]
        );
    }

    #[test]
    fn empty_microsoft_store_suffix_fails_closed() {
        let catalog = fixture_catalog();
        assert_eq!(
            build_package_plan(&catalog, &ids(&["store_empty"]), PackagePreference::Winget,),
            Err(PackagePlanError::EmptyMsStoreId("msstore:".into()))
        );
    }

    #[test]
    fn winget_preference_does_not_use_chocolatey_ids() {
        let catalog = embedded_applications().unwrap();
        let plan = build_package_plan(
            &catalog,
            &ids(&["7zip", "githubdesktop"]),
            PackagePreference::Winget,
        )
        .unwrap();
        assert_eq!(plan.winget, ["7zip.7zip", "GitHub.GitHubDesktop"]);
        assert!(plan.choco.is_empty());
        assert!(plan.chocolatey_request.is_none());
        assert!(plan
            .decisions
            .iter()
            .all(|decision| !decision.fallback_to_winget));
    }

    #[test]
    fn deduplicates_in_order_per_manager_with_exact_string_equality() {
        let catalog = fixture_catalog();
        let plan = build_package_plan(
            &catalog,
            &ids(&["one", "two", "three", "four", "one"]),
            PackagePreference::Choco,
        )
        .unwrap();
        assert_eq!(plan.choco, ["Same.Id"]);
        assert_eq!(plan.winget, ["same.id", "SAME.ID"]);
        assert_eq!(
            plan.decisions
                .iter()
                .map(|decision| decision.disposition)
                .collect::<Vec<_>>(),
            [
                DecisionDisposition::Added,
                DecisionDisposition::Duplicate,
                DecisionDisposition::Added,
                DecisionDisposition::Added,
                DecisionDisposition::Duplicate,
            ]
        );
    }

    #[test]
    fn chocolatey_request_is_ordered_deduplicated_and_preserves_semicolons() {
        let catalog = fixture_catalog();
        let plan = build_package_plan(
            &catalog,
            &ids(&["one", "two", "semi"]),
            PackagePreference::Choco,
        )
        .unwrap();

        let request = plan.chocolatey_request.unwrap();
        assert_eq!(request.action, ChocolateyAction::Install);
        assert_eq!(request.package_ids, ["Same.Id", "git;github-desktop"]);
        assert!(request.assume_yes);
    }

    #[test]
    fn action_aware_planner_records_uninstall_intent_and_edge_action_metadata() {
        let catalog = decode_applications(
            br#"{"edge":{"category":"A","content":"Edge","description":"Edge","link":"https://example.test/edge","winget":"microsoft.edge","foss":true}}"#,
        )
        .unwrap();
        let plan = build_package_plan_for_action(
            &catalog,
            &ids(&["edge"]),
            PackagePreference::Winget,
            PackageAction::Uninstall,
        )
        .unwrap();

        assert_eq!(plan.action, PackageAction::Uninstall);
        assert_eq!(plan.winget, ["microsoft.edge"]);
        assert!(plan.edge_filesystem_action_present);
        assert!(plan.chocolatey_request.is_none());

        let install_plan =
            build_package_plan(&catalog, &ids(&["edge"]), PackagePreference::Winget).unwrap();
        assert_eq!(install_plan.action, PackageAction::Install);
        assert!(!install_plan.edge_filesystem_action_present);
    }

    #[test]
    fn chocolatey_uninstall_request_is_typed_and_keeps_package_order() {
        let catalog = fixture_catalog();
        let plan = build_package_plan_for_action(
            &catalog,
            &ids(&["one", "semi"]),
            PackagePreference::Choco,
            PackageAction::Uninstall,
        )
        .unwrap();

        let request = plan.chocolatey_request.unwrap();
        assert_eq!(request.action, ChocolateyAction::Uninstall);
        assert_eq!(request.package_ids, ["Same.Id", "git;github-desktop"]);
        assert!(request.assume_yes);
    }

    #[test]
    fn empty_plan_keeps_action_without_manager_requests_or_edge_warning() {
        let catalog = fixture_catalog();
        let plan = build_package_plan_for_action(
            &catalog,
            &[],
            PackagePreference::Choco,
            PackageAction::Uninstall,
        )
        .unwrap();

        assert_eq!(plan.action, PackageAction::Uninstall);
        assert!(plan.winget.is_empty());
        assert!(plan.choco.is_empty());
        assert!(plan.chocolatey_request.is_none());
        assert!(!plan.edge_filesystem_action_present);
    }

    #[test]
    fn blank_choco_falls_back_to_winget_and_na_is_skipped() {
        let catalog = fixture_catalog();
        let plan = build_package_plan(
            &catalog,
            &ids(&["blank", "sentinel"]),
            PackagePreference::Choco,
        )
        .unwrap();
        assert!(plan.choco.is_empty());
        assert_eq!(plan.winget, ["Fallback.From.Blank"]);
        assert_eq!(
            plan.decisions
                .iter()
                .map(|decision| decision.disposition)
                .collect::<Vec<_>>(),
            [
                DecisionDisposition::Added,
                DecisionDisposition::SkippedUnavailable,
            ]
        );
        assert!(plan
            .decisions
            .iter()
            .all(|decision| decision.fallback_to_winget));
    }

    #[test]
    fn absent_choco_field_falls_back_to_winget() {
        let catalog = fixture_catalog();
        let plan = build_package_plan(&catalog, &ids(&["missing_choco"]), PackagePreference::Choco)
            .unwrap();
        assert_eq!(plan.winget, ["Fallback.Id"]);
        assert!(plan.choco.is_empty());
        assert!(plan.decisions[0].fallback_to_winget);
    }

    #[test]
    fn add_package_id_skips_blank_and_na_values() {
        let mut target = Vec::new();
        assert_eq!(
            add_package_id(&mut target, " \t "),
            DecisionDisposition::SkippedBlank
        );
        assert_eq!(
            add_package_id(&mut target, "nA"),
            DecisionDisposition::SkippedUnavailable
        );
        assert!(target.is_empty());
    }

    #[test]
    fn unknown_catalog_id_fails_without_returning_a_partial_plan() {
        let catalog = embedded_applications().unwrap();
        assert_eq!(
            build_package_plan(
                &catalog,
                &ids(&["7zip", "not-a-catalog-id"]),
                PackagePreference::Choco,
            ),
            Err(PackagePlanError::UnknownCatalogId(
                "not-a-catalog-id".into()
            ))
        );
    }

    #[test]
    fn parses_only_supported_preferences_case_insensitively() {
        assert_eq!(
            PackagePreference::parse("Choco"),
            Ok(PackagePreference::Choco)
        );
        assert_eq!(
            PackagePreference::parse("wInGeT"),
            Ok(PackagePreference::Winget)
        );
        assert_eq!(
            PackagePreference::parse("auto"),
            Err(PackagePlanError::InvalidPreference("auto".into()))
        );
    }
}
