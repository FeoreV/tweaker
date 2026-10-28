//! Pure composition of bounded WinUtil selection JSON with the current
//! application preview catalog and data-only package planner.
//!
//! This supports only the catalog bundled with the preview UI. It does not
//! claim complete upstream application coverage, load files, mutate selection
//! state, invoke a package manager, or execute the resulting intent.

use crate::data::applications::ApplicationCatalog;
use crate::data::selection_import::{
    decode_selection_import, KnownSelectionIds, NoChangesReason, SelectionImportError,
    SelectionImportFormat, SelectionImportResult,
};
use crate::engine::package_plan::{
    build_package_plan_for_action, PackageAction, PackagePlan, PackagePlanError, PackagePreference,
};
use serde::Serialize;
use std::fmt;

/// Provenance limitation for all results from this composition layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogCoverage {
    PreviewSubsetOnly,
}

/// Successful result. Empty/unsupported legacy input is retained as a
/// no-change outcome and deliberately has no package plan.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PackageImportResult {
    Planned {
        format: SelectionImportFormatJson,
        skipped_legacy: Vec<String>,
        selected_catalog_ids: Vec<String>,
        plan: PackagePlan,
        catalog_coverage: CatalogCoverage,
        executable: bool,
    },
    NoChanges {
        reason: NoChangesJson,
        catalog_coverage: CatalogCoverage,
        executable: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionImportFormatJson {
    Modern,
    Legacy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum NoChangesJson {
    EmptyConfiguration,
    NoSupportedLegacySelections { skipped: Vec<String> },
}

/// Composition failures. Errors never carry a partial plan.
#[derive(Debug)]
pub enum PackageImportError {
    Selection(SelectionImportError),
    LegacySelectionsOutsidePreviewCatalog {
        keys: Vec<String>,
    },
    UnsupportedSelectedCategories {
        tweaks: Vec<String>,
        toggles: Vec<String>,
        features: Vec<String>,
        appx: Vec<String>,
    },
    PackagePlan(PackagePlanError),
}

impl fmt::Display for PackageImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selection(SelectionImportError::UnknownModernSelection(key)) => write!(
                f,
                "selection key {key:?} is not in the supplied preview application catalog; upstream support is unverified"
            ),
            Self::Selection(error) => write!(f, "selection import failed: {error:?}"),
            Self::LegacySelectionsOutsidePreviewCatalog { keys } => write!(
                f,
                "legacy selection keys are absent from the supplied preview catalog; upstream support is unverified: {}",
                summarize_keys(keys)
            ),
            Self::UnsupportedSelectedCategories { tweaks, toggles, features, appx } => {
                let keys = tweaks.iter().chain(toggles).chain(features).chain(appx).cloned().collect::<Vec<_>>();
                write!(
                    f,
                    "selection import includes categories not supported by this package planner: {}",
                    summarize_keys(&keys)
                )
            }
            Self::PackagePlan(error) => write!(f, "package planning failed: {error}"),
        }
    }
}

fn summarize_keys(keys: &[String]) -> String {
    let shown = keys.iter().take(10).cloned().collect::<Vec<_>>().join(", ");
    if keys.len() > 10 {
        format!("{shown}, and {} more", keys.len() - 10)
    } else {
        shown
    }
}

impl std::error::Error for PackageImportError {}

/// Decode selection JSON and build a data-only package plan from the supplied
/// preview application catalog. PowerShell checkbox keys are `WPFInstall`
/// followed by the catalog key. Matching is ASCII-case-insensitive, following
/// the existing decoder and catalog lookup behavior.
pub fn plan_package_import(
    bytes: &[u8],
    catalog: &ApplicationCatalog,
    preference: PackagePreference,
    action: PackageAction,
) -> Result<PackageImportResult, PackageImportError> {
    let known_apps: Vec<String> = catalog
        .iter()
        .map(|(id, _)| format!("WPFInstall{id}"))
        .collect();
    let decoded = decode_selection_import(
        bytes,
        &KnownSelectionIds {
            apps: Some(&known_apps),
            // These catalogs are unavailable. Legacy selections can therefore
            // be decoded, but must be rejected below if any are selected.
            ..KnownSelectionIds::default()
        },
    )
    .map_err(PackageImportError::Selection)?;

    let (format, selections, skipped_legacy) = match decoded {
        SelectionImportResult::NoChanges(reason) => {
            if let NoChangesReason::NoSupportedLegacySelections { skipped } = &reason {
                if !skipped.is_empty() {
                    return Err(PackageImportError::LegacySelectionsOutsidePreviewCatalog {
                        keys: skipped.clone(),
                    });
                }
            }
            return Ok(PackageImportResult::NoChanges {
                reason: reason.into(),
                catalog_coverage: CatalogCoverage::PreviewSubsetOnly,
                executable: false,
            });
        }
        SelectionImportResult::Replace {
            format,
            selections,
            skipped_legacy,
        } => {
            if !skipped_legacy.is_empty() {
                return Err(PackageImportError::LegacySelectionsOutsidePreviewCatalog {
                    keys: skipped_legacy,
                });
            }
            (format, selections, skipped_legacy)
        }
    };

    if !selections.tweaks.is_empty()
        || !selections.toggles.is_empty()
        || !selections.features.is_empty()
        || !selections.appx.is_empty()
    {
        return Err(PackageImportError::UnsupportedSelectedCategories {
            tweaks: selections.tweaks,
            toggles: selections.toggles,
            features: selections.features,
            appx: selections.appx,
        });
    }

    let catalog_ids: Vec<String> = selections
        .apps
        .iter()
        .map(|key| key["WPFInstall".len()..].to_owned())
        .collect();
    // Resolve using catalog lookup as the authority and preserve the catalog's
    // canonical identifier spelling for planner decisions.
    let canonical_ids: Vec<String> = catalog_ids
        .iter()
        .map(|id| {
            catalog
                .iter()
                .find(|(catalog_id, _)| catalog_id.eq_ignore_ascii_case(id))
                .map(|(catalog_id, _)| catalog_id.to_owned())
                .ok_or_else(|| PackagePlanError::UnknownCatalogId(id.clone()))
        })
        .collect::<Result<_, _>>()
        .map_err(PackageImportError::PackagePlan)?;

    let plan = build_package_plan_for_action(catalog, &canonical_ids, preference, action)
        .map_err(PackageImportError::PackagePlan)?;
    Ok(PackageImportResult::Planned {
        format: format.into(),
        skipped_legacy,
        selected_catalog_ids: canonical_ids,
        plan,
        catalog_coverage: CatalogCoverage::PreviewSubsetOnly,
        executable: false,
    })
}

impl From<SelectionImportFormat> for SelectionImportFormatJson {
    fn from(value: SelectionImportFormat) -> Self {
        match value {
            SelectionImportFormat::Modern => Self::Modern,
            SelectionImportFormat::Legacy => Self::Legacy,
        }
    }
}

impl From<NoChangesReason> for NoChangesJson {
    fn from(value: NoChangesReason) -> Self {
        match value {
            NoChangesReason::EmptyConfiguration => Self::EmptyConfiguration,
            NoChangesReason::NoSupportedLegacySelections { skipped } => {
                Self::NoSupportedLegacySelections { skipped }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::applications::decode_applications;

    fn catalog() -> ApplicationCatalog {
        decode_applications(br#"{
          "Alpha": {"category":"A","content":"Alpha","description":"Alpha app","link":"https://example.test/alpha","winget":"Alpha.Id","choco":"alpha-choco","foss":true},
          "Beta": {"category":"A","content":"Beta","description":"Beta app","link":"https://example.test/beta","winget":"Beta.Id","foss":true}
        }"#).unwrap()
    }

    fn planned(result: PackageImportResult) -> (Vec<String>, PackagePlan) {
        match result {
            PackageImportResult::Planned {
                selected_catalog_ids,
                plan,
                ..
            } => (selected_catalog_ids, plan),
            other => panic!("expected plan, got {other:?}"),
        }
    }

    #[test]
    fn modern_scalar_key_imports_and_is_explicitly_non_executable_preview_data() {
        let (ids, plan) = planned(
            plan_package_import(
                br#""WPFInstallAlpha""#,
                &catalog(),
                PackagePreference::Winget,
                PackageAction::Install,
            )
            .unwrap(),
        );
        assert_eq!(ids, ["Alpha"]);
        assert_eq!(plan.winget, ["Alpha.Id"]);
        let json = serde_json::to_value(PackageImportResult::Planned {
            format: SelectionImportFormatJson::Modern,
            skipped_legacy: vec![],
            selected_catalog_ids: ids,
            plan,
            catalog_coverage: CatalogCoverage::PreviewSubsetOnly,
            executable: false,
        })
        .unwrap();
        assert_eq!(json["catalog_coverage"], "preview_subset_only");
        assert_eq!(json["executable"], false);
    }

    #[test]
    fn legacy_missing_preview_app_fails_closed_instead_of_partial_plan() {
        let result = plan_package_import(
            br#"{"Install":[],"WPFInstall":["WPFInstallAlpha","WPFInstallRetired"]}"#,
            &catalog(),
            PackagePreference::Choco,
            PackageAction::Install,
        );
        let error = result.unwrap_err();
        assert!(matches!(
            &error,
            PackageImportError::LegacySelectionsOutsidePreviewCatalog { keys }
                if keys.as_slice() == ["WPFInstallRetired"]
        ));
        let message = error.to_string();
        assert!(message.contains("WPFInstallRetired"));
        assert!(message.contains("upstream support is unverified"));
        let result = plan_package_import(
            br#"{"Install":[],"WPFInstall":["WPFInstallRetired"]}"#,
            &catalog(),
            PackagePreference::Choco,
            PackageAction::Install,
        );
        assert!(matches!(
            result,
            Err(PackageImportError::LegacySelectionsOutsidePreviewCatalog { keys })
                if keys == ["WPFInstallRetired"]
        ));
    }

    #[test]
    fn selected_non_app_categories_fail_without_any_partial_plan() {
        for bytes in [
            &br#"["WPFInstallAlpha","WPFTweaksKnown"]"#[..],
            &br#"{"Install":[],"WPFInstall":["WPFInstallAlpha"],"WPFTweaks":["WPFTweaksKnown"]}"#[..],
        ] {
            let error = plan_package_import(
                bytes,
                &catalog(),
                PackagePreference::Winget,
                PackageAction::Install,
            )
            .unwrap_err();
            assert!(matches!(
                error,
                PackageImportError::UnsupportedSelectedCategories { .. }
            ));
        }
        let error = plan_package_import(
            br#"["WPFInstallAlpha","WPFTweaksKnown"]"#,
            &catalog(),
            PackagePreference::Winget,
            PackageAction::Install,
        )
        .unwrap_err();
        assert!(error.to_string().contains("WPFTweaksKnown"));
    }

    #[test]
    fn modern_unknown_is_strict_and_returns_no_plan() {
        assert!(matches!(
            plan_package_import(br#"["WPFInstallAlpha","WPFInstallRetired"]"#, &catalog(), PackagePreference::Choco, PackageAction::Install),
            Err(PackageImportError::Selection(SelectionImportError::UnknownModernSelection(id))) if id == "WPFInstallRetired"
        ));
    }

    #[test]
    fn checkbox_key_and_catalog_lookup_are_case_insensitive() {
        let (ids, _) = planned(
            plan_package_import(
                br#""wpfinstallalpha""#,
                &catalog(),
                PackagePreference::Winget,
                PackageAction::Install,
            )
            .unwrap(),
        );
        assert_eq!(ids, ["Alpha"]);
    }

    #[test]
    fn preference_fallback_and_duplicate_order_are_preserved() {
        let (ids, plan) = planned(
            plan_package_import(
                br#"["WPFInstallBeta","WPFInstallAlpha","WPFInstallBeta"]"#,
                &catalog(),
                PackagePreference::Choco,
                PackageAction::Uninstall,
            )
            .unwrap(),
        );
        assert_eq!(ids, ["Beta", "Alpha", "Beta"]);
        assert_eq!(plan.winget, ["Beta.Id"]);
        assert_eq!(plan.choco, ["alpha-choco"]);
        assert!(plan.decisions[0].fallback_to_winget);
        assert_eq!(
            plan.decisions[2].disposition,
            crate::engine::package_plan::DecisionDisposition::Duplicate
        );
        assert_eq!(plan.action, PackageAction::Uninstall);
    }

    #[test]
    fn byte_decoder_preserves_size_and_utf8_failures() {
        let catalog = catalog();
        assert!(matches!(
            plan_package_import(
                &vec![b' '; crate::data::selection_import::MAX_SELECTION_IMPORT_BYTES + 1],
                &catalog,
                PackagePreference::Winget,
                PackageAction::Install
            ),
            Err(PackageImportError::Selection(
                SelectionImportError::TooLarge
            ))
        ));
        assert!(matches!(
            plan_package_import(
                &[0xff],
                &catalog,
                PackagePreference::Winget,
                PackageAction::Install
            ),
            Err(PackageImportError::Selection(
                SelectionImportError::InvalidJson(_)
            ))
        ));
    }

    #[test]
    fn empty_selection_import_reports_no_changes_without_a_package_plan() {
        assert!(matches!(
            plan_package_import(
                b"[]",
                &catalog(),
                PackagePreference::Winget,
                PackageAction::Install,
            ),
            Ok(PackageImportResult::NoChanges {
                reason: NoChangesJson::EmptyConfiguration,
                executable: false,
                ..
            })
        ));
    }
}
