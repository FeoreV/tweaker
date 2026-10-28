//! Pure in-memory selection state operations.

use super::selection_import::{KnownSelectionIds, SelectionCategories, SelectionImportResult};

/// A preset selection scope. `Merge` appends without clearing; `All` clears all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresetCategory {
    Apps,
    Tweaks,
    Toggles,
    Features,
    Appx,
    /// Append every validated preset family without clearing current selections.
    Merge,
    All,
}

/// Current checkbox selection. Ordering and duplicate IDs are retained.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionState {
    pub apps: Vec<String>,
    pub tweaks: Vec<String>,
    pub toggles: Vec<String>,
    pub features: Vec<String>,
    pub appx: Vec<String>,
}

impl From<SelectionCategories> for SelectionState {
    fn from(value: SelectionCategories) -> Self {
        Self {
            apps: value.apps,
            tweaks: value.tweaks,
            toggles: value.toggles,
            features: value.features,
            appx: value.appx,
        }
    }
}

impl SelectionState {
    /// Apply a decoded import atomically. NoChanges leaves every list intact.
    pub fn apply_import(&mut self, result: &SelectionImportResult) -> bool {
        if let SelectionImportResult::Replace { selections, .. } = result {
            *self = selections.clone().into();
            true
        } else {
            false
        }
    }

    /// Validate every preset ID before changing state. The requested category
    /// is cleared first, then all preset families are appended, matching the
    /// source's filtered preset operation. `Merge` appends without clearing;
    /// `All` clears every category. Errors leave the state unchanged.
    pub fn apply_preset(
        &mut self,
        ids: &[String],
        scope: PresetCategory,
        known: &KnownSelectionIds<'_>,
    ) -> Result<(), PresetError> {
        let mut categorized = SelectionCategories::default();
        for id in ids {
            let category = classify(id).ok_or_else(|| PresetError::UnsupportedId(id.clone()))?;
            let catalog = catalog_for(category, known);
            let Some(catalog) = catalog else {
                return Err(PresetError::CatalogUnavailable {
                    id: id.clone(),
                    category: category.into(),
                });
            };
            if !catalog
                .iter()
                .any(|known_id| known_id.eq_ignore_ascii_case(id))
            {
                return Err(PresetError::UnknownId {
                    id: id.clone(),
                    category: category.into(),
                });
            }
            push(&mut categorized, category, id.clone());
        }

        match scope {
            PresetCategory::All => *self = SelectionState::default(),
            PresetCategory::Merge => {}
            _ => clear_scope(self, scope),
        }
        // PowerShell feeds the entire preset into Update-WinUtilSelections,
        // which appends every category after the selected scope is cleared.
        append_all(self, categorized);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresetError {
    UnsupportedId(String),
    UnknownId {
        id: String,
        category: PresetCategory,
    },
    CatalogUnavailable {
        id: String,
        category: PresetCategory,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Category {
    Apps,
    Tweaks,
    Toggles,
    Features,
    Appx,
}

impl From<Category> for PresetCategory {
    fn from(value: Category) -> Self {
        match value {
            Category::Apps => Self::Apps,
            Category::Tweaks => Self::Tweaks,
            Category::Toggles => Self::Toggles,
            Category::Features => Self::Features,
            Category::Appx => Self::Appx,
        }
    }
}

fn classify(id: &str) -> Option<Category> {
    [
        ("WPFInstall", Category::Apps),
        ("WPFTweaks", Category::Tweaks),
        ("WPFToggle", Category::Toggles),
        ("WPFFeature", Category::Features),
        ("WPFAppx", Category::Appx),
    ]
    .iter()
    .find_map(|(prefix, category)| {
        id.get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
            .map(|_| *category)
    })
}

fn catalog_for<'a>(category: Category, known: &'a KnownSelectionIds<'_>) -> Option<&'a [String]> {
    match category {
        Category::Apps => known.apps,
        Category::Tweaks => known.tweaks,
        Category::Toggles => known.toggles,
        Category::Features => known.features,
        Category::Appx => known.appx,
    }
}

fn push(out: &mut SelectionCategories, category: Category, id: String) {
    match category {
        Category::Apps => out.apps.push(id),
        Category::Tweaks => out.tweaks.push(id),
        Category::Toggles => out.toggles.push(id),
        Category::Features => out.features.push(id),
        Category::Appx => out.appx.push(id),
    }
}

fn clear_scope(state: &mut SelectionState, scope: PresetCategory) {
    match scope {
        PresetCategory::Apps => state.apps.clear(),
        PresetCategory::Tweaks => state.tweaks.clear(),
        PresetCategory::Toggles => state.toggles.clear(),
        PresetCategory::Features => state.features.clear(),
        PresetCategory::Appx => state.appx.clear(),
        PresetCategory::Merge => {}
        PresetCategory::All => unreachable!(),
    }
}

fn append_all(state: &mut SelectionState, values: SelectionCategories) {
    state.apps.extend(values.apps);
    state.tweaks.extend(values.tweaks);
    state.toggles.extend(values.toggles);
    state.features.extend(values.features);
    state.appx.extend(values.appx);
}

#[cfg(test)]
mod tests {
    use super::super::selection_import::{NoChangesReason, SelectionImportFormat};
    use super::*;

    type TestCatalogs = [Vec<String>; 5];

    fn catalogs() -> TestCatalogs {
        [
            vec!["WPFInstallA".into(), "WPFInstallB".into()],
            vec!["WPFTweaksA".into(), "WPFTweaksB".into()],
            vec!["WPFToggleA".into(), "WPFToggleB".into()],
            vec!["WPFFeatureA".into(), "WPFFeatureB".into()],
            vec!["WPFAppxA".into(), "WPFAppxB".into()],
        ]
    }
    fn known(c: &TestCatalogs) -> KnownSelectionIds<'_> {
        KnownSelectionIds {
            apps: Some(&c[0]),
            tweaks: Some(&c[1]),
            toggles: Some(&c[2]),
            features: Some(&c[3]),
            appx: Some(&c[4]),
        }
    }
    fn sid(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn scoped_replace_preserves_other_categories() {
        let c = catalogs();
        let mut state = SelectionState {
            apps: sid(&["WPFInstallA"]),
            tweaks: sid(&["old"]),
            ..Default::default()
        };
        state
            .apply_preset(&sid(&["WPFTweaksA"]), PresetCategory::Tweaks, &known(&c))
            .unwrap();
        assert_eq!(state.apps, ["WPFInstallA"]);
        assert_eq!(state.tweaks, ["WPFTweaksA"]);
    }

    #[test]
    fn scoped_replace_appends_preset_ids_from_other_categories() {
        let c = catalogs();
        let mut state = SelectionState {
            apps: sid(&["WPFInstallA"]),
            tweaks: sid(&["old"]),
            ..Default::default()
        };
        state
            .apply_preset(
                &sid(&["WPFTweaksA", "WPFInstallA"]),
                PresetCategory::Tweaks,
                &known(&c),
            )
            .unwrap();
        assert_eq!(state.apps, ["WPFInstallA", "WPFInstallA"]);
        assert_eq!(state.tweaks, ["WPFTweaksA"]);
    }

    #[test]
    fn merge_scope_keeps_existing_items_and_appends_all_preset_families() {
        let catalogs = catalogs();
        let mut state = SelectionState {
            apps: sid(&["WPFInstallA"]),
            tweaks: sid(&["WPFTweaksA"]),
            toggles: sid(&["WPFToggleA"]),
            features: sid(&["WPFFeatureA"]),
            appx: sid(&["WPFAppxA"]),
        };
        let preset = sid(&[
            "WPFInstallB",
            "WPFTweaksB",
            "WPFToggleB",
            "WPFFeatureB",
            "WPFAppxB",
        ]);

        state
            .apply_preset(&preset, PresetCategory::Merge, &known(&catalogs))
            .unwrap();

        assert_eq!(state.apps, ["WPFInstallA", "WPFInstallB"]);
        assert_eq!(state.tweaks, ["WPFTweaksA", "WPFTweaksB"]);
        assert_eq!(state.toggles, ["WPFToggleA", "WPFToggleB"]);
        assert_eq!(state.features, ["WPFFeatureA", "WPFFeatureB"]);
        assert_eq!(state.appx, ["WPFAppxA", "WPFAppxB"]);
    }

    #[test]
    fn all_replace_clears_all_five_categories() {
        let c = catalogs();
        let mut state = SelectionState {
            apps: sid(&["old"]),
            tweaks: sid(&["old"]),
            toggles: sid(&["old"]),
            features: sid(&["old"]),
            appx: sid(&["old"]),
        };
        state
            .apply_preset(
                &sid(&[
                    "WPFInstallA",
                    "WPFTweaksA",
                    "WPFToggleA",
                    "WPFFeatureA",
                    "WPFAppxA",
                ]),
                PresetCategory::All,
                &known(&c),
            )
            .unwrap();
        assert_eq!(
            state,
            SelectionState {
                apps: sid(&["WPFInstallA"]),
                tweaks: sid(&["WPFTweaksA"]),
                toggles: sid(&["WPFToggleA"]),
                features: sid(&["WPFFeatureA"]),
                appx: sid(&["WPFAppxA"])
            }
        );
    }

    #[test]
    fn mixed_valid_invalid_is_atomic() {
        let c = catalogs();
        let mut state = SelectionState {
            apps: sid(&["before"]),
            ..Default::default()
        };
        let before = state.clone();
        assert!(matches!(
            state.apply_preset(
                &sid(&["WPFInstallA", "WPFInstallMissing"]),
                PresetCategory::All,
                &known(&c)
            ),
            Err(PresetError::UnknownId { .. })
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn unavailable_catalog_is_explicit_and_atomic() {
        let unavailable = KnownSelectionIds::default();
        let mut state = SelectionState {
            apps: sid(&["before"]),
            ..Default::default()
        };
        let before = state.clone();
        assert!(matches!(
            state.apply_preset(&sid(&["WPFInstallA"]), PresetCategory::All, &unavailable),
            Err(PresetError::CatalogUnavailable { .. })
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn duplicates_and_input_order_are_preserved() {
        let c = catalogs();
        let mut state = SelectionState::default();
        state
            .apply_preset(
                &sid(&["WPFTweaksA", "WPFInstallA", "WPFTweaksA"]),
                PresetCategory::All,
                &known(&c),
            )
            .unwrap();
        assert_eq!(state.tweaks, ["WPFTweaksA", "WPFTweaksA"]);
        assert_eq!(state.apps, ["WPFInstallA"]);
    }

    #[test]
    fn import_replace_is_all_categories_and_no_changes_is_noop() {
        let mut state = SelectionState {
            apps: sid(&["old"]),
            ..Default::default()
        };
        let result = SelectionImportResult::Replace {
            format: SelectionImportFormat::Modern,
            selections: SelectionCategories {
                tweaks: sid(&["WPFTweaksA"]),
                ..Default::default()
            },
            skipped_legacy: vec![],
        };
        assert!(state.apply_import(&result));
        assert!(state.apps.is_empty());
        assert_eq!(state.tweaks, ["WPFTweaksA"]);
        let before = state.clone();
        let result = SelectionImportResult::NoChanges(NoChangesReason::EmptyConfiguration);
        assert!(!state.apply_import(&result));
        assert_eq!(state, before);
    }
}
