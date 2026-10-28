//! Pure WinUtil selection JSON exporter.
//!
//! Mirrors `Invoke-WPFImpex.ps1`: concatenate selected apps, tweaks, toggles,
//! features, then AppX IDs, and serialize the resulting strings as JSON.

use super::selection_import::SelectionCategories;

pub const MAX_SELECTION_EXPORT_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub enum SelectionExportError {
    EmptySelection,
    TooLarge,
    Serialization(String),
}

/// Serialize selections in WinUtil export order without changing their order
/// within each category or removing duplicates. A single selected ID is
/// encoded as a JSON string, matching PowerShell's pipeline serialization.
pub fn encode_selection_export(
    selections: &SelectionCategories,
) -> Result<String, SelectionExportError> {
    let count = selections.apps.len()
        + selections.tweaks.len()
        + selections.toggles.len()
        + selections.features.len()
        + selections.appx.len();
    if count == 0 {
        return Err(SelectionExportError::EmptySelection);
    }

    let mut all = Vec::with_capacity(count);
    all.extend(selections.apps.iter());
    all.extend(selections.tweaks.iter());
    all.extend(selections.toggles.iter());
    all.extend(selections.features.iter());
    all.extend(selections.appx.iter());

    let json = if all.len() == 1 {
        serde_json::to_string(all[0])
    } else {
        serde_json::to_string(&all)
    }
    .map_err(|error| SelectionExportError::Serialization(error.to_string()))?;

    if json.len() > MAX_SELECTION_EXPORT_BYTES {
        return Err(SelectionExportError::TooLarge);
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenates_categories_in_export_order_and_keeps_duplicates() {
        let selections = SelectionCategories {
            apps: vec!["app-a".into(), "app-a".into()],
            tweaks: vec!["tweak".into()],
            toggles: vec!["toggle".into()],
            features: vec!["feature".into()],
            appx: vec!["appx".into()],
        };
        assert_eq!(
            encode_selection_export(&selections).unwrap(),
            r#"["app-a","app-a","tweak","toggle","feature","appx"]"#
        );
    }

    #[test]
    fn emits_scalar_for_one_id_and_array_for_multiple_ids() {
        let one = SelectionCategories {
            tweaks: vec!["WPFTweaksOne".into()],
            ..SelectionCategories::default()
        };
        assert_eq!(encode_selection_export(&one).unwrap(), r#""WPFTweaksOne""#);

        let two = SelectionCategories {
            apps: vec!["first".into()],
            appx: vec!["second".into()],
            ..SelectionCategories::default()
        };
        assert_eq!(
            encode_selection_export(&two).unwrap(),
            r#"["first","second"]"#
        );
    }

    #[test]
    fn escapes_json_and_roundtrips_unicode() {
        let original = "quote\" slash\\ line\n ☃ 日本語";
        let selections = SelectionCategories {
            appx: vec![original.into()],
            ..SelectionCategories::default()
        };
        let encoded = encode_selection_export(&selections).unwrap();
        assert_eq!(serde_json::from_str::<String>(&encoded).unwrap(), original);
    }

    #[test]
    fn rejects_empty_selection() {
        assert!(matches!(
            encode_selection_export(&SelectionCategories::default()),
            Err(SelectionExportError::EmptySelection)
        ));
    }

    #[test]
    fn rejects_output_over_one_mib() {
        let selections = SelectionCategories {
            apps: vec!["x".repeat(MAX_SELECTION_EXPORT_BYTES)],
            ..SelectionCategories::default()
        };
        assert!(matches!(
            encode_selection_export(&selections),
            Err(SelectionExportError::TooLarge)
        ));
    }
}
