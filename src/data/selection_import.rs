//! Bounded decoder for WinUtil selection imports. It only returns proposed
//! selections; callers decide whether and how to replace their own state.

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

pub const MAX_SELECTION_IMPORT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionCategories {
    pub apps: Vec<String>,
    pub tweaks: Vec<String>,
    pub toggles: Vec<String>,
    pub features: Vec<String>,
    pub appx: Vec<String>,
}

/// Each `Some` is a known-ID catalog for that family. `None` means the
/// catalog was unavailable, so IDs in that family cannot be judged unknown.
#[derive(Clone, Debug, Default)]
pub struct KnownSelectionIds<'a> {
    pub apps: Option<&'a [String]>,
    pub tweaks: Option<&'a [String]>,
    pub toggles: Option<&'a [String]>,
    pub features: Option<&'a [String]>,
    pub appx: Option<&'a [String]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionImportFormat {
    Modern,
    Legacy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoChangesReason {
    EmptyConfiguration,
    NoSupportedLegacySelections { skipped: Vec<String> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectionImportResult {
    Replace {
        format: SelectionImportFormat,
        selections: SelectionCategories,
        skipped_legacy: Vec<String>,
    },
    NoChanges(NoChangesReason),
}

#[derive(Debug)]
pub enum SelectionImportError {
    TooLarge,
    InvalidJson(serde_json::Error),
    InvalidModernShape(&'static str),
    UnknownModernSelection(String),
}

/// Small JSON representation that retains object member order for legacy
/// property traversal, without enabling serde_json's optional map dependency.
#[derive(Debug)]
enum JsonValue {
    Null,
    Bool,
    Number,
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = JsonValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonValue::Null)
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonValue::Null)
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(JsonValue::Bool)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(JsonValue::Number)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(JsonValue::Number)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(JsonValue::Number)
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(JsonValue::String(value.to_owned()))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(JsonValue::String(value))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(JsonValue::Array(values))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    values.push(entry);
                }
                Ok(JsonValue::Object(values))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}

#[derive(Clone, Copy)]
enum Family {
    Apps,
    Tweaks,
    Toggles,
    Features,
    Appx,
}

/// Decode a modern flat string array or a legacy WinUtil object. Identifier
/// matching and prefix recognition are ASCII-case-insensitive, matching the
/// common PowerShell cases without claiming full Unicode case-fold parity.
pub fn decode_selection_import(
    bytes: &[u8],
    known: &KnownSelectionIds<'_>,
) -> Result<SelectionImportResult, SelectionImportError> {
    if bytes.len() > MAX_SELECTION_IMPORT_BYTES {
        return Err(SelectionImportError::TooLarge);
    }
    let value: JsonValue =
        serde_json::from_slice(bytes).map_err(SelectionImportError::InvalidJson)?;
    if is_legacy_object(&value) {
        decode_legacy(&value, known)
    } else {
        decode_modern(&value, known)
    }
}

fn is_legacy_object(value: &JsonValue) -> bool {
    if let JsonValue::Object(obj) = value {
        obj.iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("Install"))
            && obj
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case("WPFInstall"))
    } else {
        false
    }
}

fn decode_modern(
    value: &JsonValue,
    known: &KnownSelectionIds<'_>,
) -> Result<SelectionImportResult, SelectionImportError> {
    let singleton;
    let values = match value {
        JsonValue::Array(values) => values.as_slice(),
        JsonValue::String(value) if !value.is_empty() => {
            singleton = vec![JsonValue::String(value.clone())];
            singleton.as_slice()
        }
        JsonValue::String(_) => {
            return Ok(SelectionImportResult::NoChanges(
                NoChangesReason::EmptyConfiguration,
            ))
        }
        _ => {
            return Err(SelectionImportError::InvalidModernShape(
                "expected a flat array of strings or a scalar string",
            ))
        }
    };
    if values.is_empty() {
        return Ok(SelectionImportResult::NoChanges(
            NoChangesReason::EmptyConfiguration,
        ));
    }
    let mut result = SelectionCategories::default();
    for value in values {
        let JsonValue::String(id) = value else {
            return Err(SelectionImportError::InvalidModernShape(
                "every array item must be a string",
            ));
        };
        let family = classify(id)
            .ok_or_else(|| SelectionImportError::UnknownModernSelection(id.to_owned()))?;
        if is_unknown(id, family, known) {
            return Err(SelectionImportError::UnknownModernSelection(id.to_owned()));
        }
        push(&mut result, family, id.to_owned());
    }
    Ok(SelectionImportResult::Replace {
        format: SelectionImportFormat::Modern,
        selections: result,
        skipped_legacy: Vec::new(),
    })
}

fn decode_legacy(
    value: &JsonValue,
    known: &KnownSelectionIds<'_>,
) -> Result<SelectionImportResult, SelectionImportError> {
    let JsonValue::Object(obj) = value else {
        unreachable!("legacy detection requires object")
    };
    let mut flattened = Vec::new();
    for (name, property) in obj {
        if !is_legacy_selection_property(name) {
            continue;
        }
        match property {
            JsonValue::Array(values) => {
                for item in values {
                    collect_legacy_string(item, &mut flattened);
                }
            }
            other => collect_legacy_string(other, &mut flattened),
        }
    }
    if flattened.is_empty() {
        return Ok(SelectionImportResult::NoChanges(
            NoChangesReason::EmptyConfiguration,
        ));
    }
    let mut result = SelectionCategories::default();
    let mut skipped = Vec::new();
    for id in flattened {
        let Some(family) = classify(&id) else {
            skipped.push(id);
            continue;
        };
        if is_unknown(&id, family, known) {
            skipped.push(id);
        } else {
            push(&mut result, family, id);
        }
    }
    if result_count(&result) == 0 {
        return Ok(SelectionImportResult::NoChanges(
            NoChangesReason::NoSupportedLegacySelections { skipped },
        ));
    }
    Ok(SelectionImportResult::Replace {
        format: SelectionImportFormat::Legacy,
        selections: result,
        skipped_legacy: skipped,
    })
}

fn collect_legacy_string(value: &JsonValue, out: &mut Vec<String>) {
    if let JsonValue::String(value) = value {
        if !value.trim().is_empty() {
            out.push(value.to_owned());
        }
    }
}

fn is_legacy_selection_property(name: &str) -> bool {
    [
        "WPFInstall",
        "WPFTweaks",
        "WPFToggle",
        "WPFFeature",
        "WPFAppx",
    ]
    .iter()
    .any(|prefix| {
        name.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    })
}

fn classify(id: &str) -> Option<Family> {
    [
        ("WPFInstall", Family::Apps),
        ("WPFTweaks", Family::Tweaks),
        ("WPFToggle", Family::Toggles),
        ("WPFFeature", Family::Features),
        ("WPFAppx", Family::Appx),
    ]
    .iter()
    .find_map(|(prefix, family)| {
        id.get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
            .map(|_| *family)
    })
}

fn is_unknown(id: &str, family: Family, known: &KnownSelectionIds<'_>) -> bool {
    let catalog = match family {
        Family::Apps => known.apps,
        Family::Tweaks => known.tweaks,
        Family::Toggles => known.toggles,
        Family::Features => known.features,
        Family::Appx => known.appx,
    };
    catalog.is_some_and(|ids| !ids.iter().any(|known_id| known_id.eq_ignore_ascii_case(id)))
}

fn push(result: &mut SelectionCategories, family: Family, id: String) {
    match family {
        Family::Apps => result.apps.push(id),
        Family::Tweaks => result.tweaks.push(id),
        Family::Toggles => result.toggles.push(id),
        Family::Features => result.features.push(id),
        Family::Appx => result.appx.push(id),
    }
}

fn result_count(result: &SelectionCategories) -> usize {
    result.apps.len()
        + result.tweaks.len()
        + result.toggles.len()
        + result.features.len()
        + result.appx.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(ids: &[String]) -> KnownSelectionIds<'_> {
        KnownSelectionIds {
            apps: Some(ids),
            tweaks: Some(ids),
            toggles: Some(ids),
            features: Some(ids),
            appx: Some(ids),
        }
    }

    fn ids() -> Vec<String> {
        [
            "WPFInstallGit",
            "WPFTweaksOne",
            "WPFToggleDarkMode",
            "WPFFeatureSandbox",
            "WPFAppxXbox",
        ]
        .iter()
        .map(|id| (*id).to_owned())
        .collect()
    }

    #[test]
    fn modern_routes_categories_and_preserves_order_and_duplicates() {
        let input = br#"["WPFInstallGit","WPFFeatureSandbox","WPFInstallGit","WPFToggleDarkMode"]"#;
        let ids = ids();
        let SelectionImportResult::Replace { selections, .. } =
            decode_selection_import(input, &known(&ids)).unwrap()
        else {
            panic!()
        };
        assert_eq!(selections.apps, ["WPFInstallGit", "WPFInstallGit"]);
        assert_eq!(selections.features, ["WPFFeatureSandbox"]);
        assert_eq!(selections.toggles, ["WPFToggleDarkMode"]);
    }

    #[test]
    fn modern_scalar_string_matches_powershell_singleton_binding() {
        let known_ids = ["WPFInstallGit".to_owned()];
        let result = decode_selection_import(
            br#""WPFInstallGit""#,
            &KnownSelectionIds {
                apps: Some(&known_ids),
                ..KnownSelectionIds::default()
            },
        )
        .unwrap();
        let SelectionImportResult::Replace {
            selections, format, ..
        } = result
        else {
            panic!()
        };
        assert_eq!(format, SelectionImportFormat::Modern);
        assert_eq!(selections.apps, ["WPFInstallGit"]);
    }

    #[test]
    fn modern_unknown_aborts_whole_import() {
        let ids = ids();
        assert!(
            matches!(decode_selection_import(br#"["WPFInstallGit","WPFInstallOld"]"#, &known(&ids)), Err(SelectionImportError::UnknownModernSelection(id)) if id == "WPFInstallOld")
        );
    }

    #[test]
    fn legacy_detects_groups_flattens_filters_and_skips_retired_ids() {
        let input = br#"{"Install":{"ignored":"metadata"},"WPFInstall":["WPFInstallGit","WPFInstallOld",null,4,"  "],"WPFTweaks":"WPFTweaksOne","ExportVersion":"ignored"}"#;
        let ids = ids();
        let SelectionImportResult::Replace {
            format: SelectionImportFormat::Legacy,
            selections,
            skipped_legacy,
        } = decode_selection_import(input, &known(&ids)).unwrap()
        else {
            panic!()
        };
        assert_eq!(selections.apps, ["WPFInstallGit"]);
        assert_eq!(selections.tweaks, ["WPFTweaksOne"]);
        assert_eq!(skipped_legacy, ["WPFInstallOld"]);
    }

    #[test]
    fn legacy_keeps_json_property_iteration_order() {
        let known_ids = ["WPFInstallZulu".to_owned(), "WPFInstallAlpha".to_owned()];
        let input = br#"{"Install":[],"WPFInstallZulu":["WPFInstallZulu"],"WPFInstall":["WPFInstallAlpha"]}"#;
        let SelectionImportResult::Replace { selections, .. } = decode_selection_import(
            input,
            &KnownSelectionIds {
                apps: Some(&known_ids),
                ..KnownSelectionIds::default()
            },
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(selections.apps, ["WPFInstallZulu", "WPFInstallAlpha"]);
    }

    #[test]
    fn legacy_all_unknown_means_no_changes() {
        let input = br#"{"Install":[],"WPFInstall":["WPFInstallOld"]}"#;
        let ids = ids();
        assert_eq!(
            decode_selection_import(input, &known(&ids)).unwrap(),
            SelectionImportResult::NoChanges(NoChangesReason::NoSupportedLegacySelections {
                skipped: vec!["WPFInstallOld".into()]
            })
        );
    }

    #[test]
    fn malformed_and_bounded_inputs_are_rejected() {
        let ids = ids();
        let known = known(&ids);
        assert!(matches!(
            decode_selection_import(b"\xff", &known),
            Err(SelectionImportError::InvalidJson(_))
        ));
        assert!(matches!(
            decode_selection_import(b"{}", &known),
            Err(SelectionImportError::InvalidModernShape(_))
        ));
        assert!(matches!(
            decode_selection_import(br#"[3,"WPFInstallGit"]"#, &known),
            Err(SelectionImportError::InvalidModernShape(_))
        ));
        assert!(matches!(
            decode_selection_import(b"[]", &known),
            Ok(SelectionImportResult::NoChanges(
                NoChangesReason::EmptyConfiguration
            ))
        ));
        assert!(matches!(
            decode_selection_import(&vec![b' '; MAX_SELECTION_IMPORT_BYTES + 1], &known),
            Err(SelectionImportError::TooLarge)
        ));
    }

    #[test]
    fn unavailable_catalog_does_not_mark_ids_unknown() {
        let unavailable = KnownSelectionIds {
            apps: None,
            tweaks: None,
            toggles: None,
            features: None,
            appx: None,
        };
        let SelectionImportResult::Replace { selections, .. } =
            decode_selection_import(br#"["WPFInstallFuture"]"#, &unavailable).unwrap()
        else {
            panic!()
        };
        assert_eq!(selections.apps, ["WPFInstallFuture"]);
    }

    #[test]
    fn matching_is_ascii_case_insensitive() {
        let ids = ids();
        let SelectionImportResult::Replace { selections, .. } =
            decode_selection_import(br#"["wpfinstallgit"]"#, &known(&ids)).unwrap()
        else {
            panic!()
        };
        assert_eq!(selections.apps, ["wpfinstallgit"]);
    }
}
