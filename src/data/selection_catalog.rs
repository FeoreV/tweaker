//! Bounded, read-only catalog of recognized selection identifiers and presets.
//!
//! The catalog is ID-selection metadata only. It does not authorize or perform
//! system changes, package installation, removal, or tweak application.

use super::selection_import::{KnownSelectionIds, SelectionCategories};
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

pub const MAX_SELECTION_CATALOG_BYTES: usize = 1024 * 1024;
pub const SELECTION_CATALOG_SCHEMA_VERSION: u32 = 1;
const EMBEDDED_SELECTION_CATALOG_BYTES: &[u8] =
    include_bytes!("../../config/selection-catalog.v1.json");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionCatalog {
    ids: SelectionCategories,
    named_presets: Vec<(String, Vec<String>)>,
}

impl SelectionCatalog {
    /// Return the validated stable identifiers grouped by selection family.
    pub fn ids(&self) -> &SelectionCategories {
        &self.ids
    }

    /// Return catalogs suitable for validating imported selections.
    pub fn known_ids(&self) -> KnownSelectionIds<'_> {
        KnownSelectionIds {
            apps: Some(&self.ids.apps),
            tweaks: Some(&self.ids.tweaks),
            toggles: Some(&self.ids.toggles),
            features: Some(&self.ids.features),
            appx: Some(&self.ids.appx),
        }
    }

    /// Find a named preset using ASCII-case-insensitive name matching.
    pub fn preset(&self, name: &str) -> Option<&[String]> {
        self.named_presets
            .iter()
            .find(|(preset_name, _)| preset_name.eq_ignore_ascii_case(name))
            .map(|(_, ids)| ids.as_slice())
    }

    /// Iterate over source-ordered named presets.
    pub fn preset_names(&self) -> impl Iterator<Item = &str> {
        self.named_presets
            .iter()
            .map(|(preset_name, _)| preset_name.as_str())
    }
}

#[derive(Debug)]
pub enum SelectionCatalogError {
    TooLarge,
    InvalidJson(serde_json::Error),
    UnsupportedSchemaVersion(u32),
    InvalidSourceManifest,
    InvalidSourceHash(String),
    InvalidIdentifier { family: &'static str, id: String },
    DuplicateIdentifier { family: &'static str, id: String },
    EmptyPresetName,
    DuplicatePresetName(String),
    EmptyPreset(String),
    UnknownPresetIdentifier { preset: String, id: String },
    AmbiguousPresetIdentifier { preset: String, id: String },
}

impl fmt::Display for SelectionCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => formatter.write_str("selection catalog exceeds byte limit"),
            Self::InvalidJson(error) => {
                write!(formatter, "invalid selection catalog JSON: {error}")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    formatter,
                    "unsupported selection catalog schema version {version}"
                )
            }
            Self::InvalidSourceManifest => {
                formatter.write_str("selection catalog source manifest is incomplete or duplicated")
            }
            Self::InvalidSourceHash(name) => write!(formatter, "invalid SHA-256 for source {name}"),
            Self::InvalidIdentifier { family, id } => {
                write!(formatter, "invalid {family} selection identifier {id:?}")
            }
            Self::DuplicateIdentifier { family, id } => {
                write!(formatter, "duplicate {family} selection identifier {id:?}")
            }
            Self::EmptyPresetName => formatter.write_str("preset name must not be empty"),
            Self::DuplicatePresetName(name) => write!(formatter, "duplicate preset name {name:?}"),
            Self::EmptyPreset(name) => write!(formatter, "preset {name:?} must contain an ID"),
            Self::UnknownPresetIdentifier { preset, id } => {
                write!(formatter, "preset {preset:?} contains unknown ID {id:?}")
            }
            Self::AmbiguousPresetIdentifier { preset, id } => {
                write!(formatter, "preset {preset:?} contains ambiguous ID {id:?}")
            }
        }
    }
}

impl std::error::Error for SelectionCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
            _ => None,
        }
    }
}

struct RawSelectionCatalog {
    schema_version: u32,
    _provenance: RawProvenance,
    source_sha256: Vec<(String, String)>,
    categories: RawCategories,
    named_presets: Vec<(String, Vec<String>)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvenance {
    #[serde(rename = "source")]
    _source: String,
    #[serde(rename = "notice")]
    _notice: String,
    #[serde(rename = "parsing")]
    _parsing: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCategories {
    apps: Vec<String>,
    tweaks: Vec<String>,
    toggles: Vec<String>,
    features: Vec<String>,
    appx: Vec<String>,
}

/// Decode and validate selection metadata without enabling any system action.
pub fn decode_selection_catalog(bytes: &[u8]) -> Result<SelectionCatalog, SelectionCatalogError> {
    if bytes.len() > MAX_SELECTION_CATALOG_BYTES {
        return Err(SelectionCatalogError::TooLarge);
    }
    let raw: RawSelectionCatalog =
        serde_json::from_slice(bytes).map_err(SelectionCatalogError::InvalidJson)?;
    if raw.schema_version != SELECTION_CATALOG_SCHEMA_VERSION {
        return Err(SelectionCatalogError::UnsupportedSchemaVersion(
            raw.schema_version,
        ));
    }

    let expected_sources = [
        "applications.json",
        "tweaks.json",
        "feature.json",
        "appx.json",
        "preset.json",
    ];
    if raw.source_sha256.len() != expected_sources.len()
        || expected_sources
            .iter()
            .any(|expected| !raw.source_sha256.iter().any(|(name, _)| name == expected))
    {
        return Err(SelectionCatalogError::InvalidSourceManifest);
    }
    for (name, hash) in &raw.source_sha256 {
        if !expected_sources.contains(&name.as_str())
            || hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(SelectionCatalogError::InvalidSourceHash(name.clone()));
        }
    }

    let ids = SelectionCategories {
        apps: raw.categories.apps,
        tweaks: raw.categories.tweaks,
        toggles: raw.categories.toggles,
        features: raw.categories.features,
        appx: raw.categories.appx,
    };
    validate_family("apps", "WPFInstall", &ids.apps)?;
    validate_family("tweaks", "WPFTweaks", &ids.tweaks)?;
    validate_family("toggles", "WPFToggle", &ids.toggles)?;
    validate_family("features", "WPFFeature", &ids.features)?;
    validate_family("appx", "WPFAppx", &ids.appx)?;

    let mut preset_names = HashSet::with_capacity(raw.named_presets.len());
    for (name, preset_ids) in &raw.named_presets {
        if name.trim().is_empty() || name.contains('\0') {
            return Err(SelectionCatalogError::EmptyPresetName);
        }
        if !preset_names.insert(name.to_ascii_lowercase()) {
            return Err(SelectionCatalogError::DuplicatePresetName(name.clone()));
        }
        if preset_ids.is_empty() {
            return Err(SelectionCatalogError::EmptyPreset(name.clone()));
        }
        for id in preset_ids {
            validate_identifier(id, "preset")?;
            let matches = family_match_count(id, &ids);
            if matches == 0 {
                return Err(SelectionCatalogError::UnknownPresetIdentifier {
                    preset: name.clone(),
                    id: id.clone(),
                });
            }
            if matches != 1 {
                return Err(SelectionCatalogError::AmbiguousPresetIdentifier {
                    preset: name.clone(),
                    id: id.clone(),
                });
            }
        }
    }

    Ok(SelectionCatalog {
        ids,
        named_presets: raw.named_presets,
    })
}

/// Decode the compile-time bundled selection catalog.
pub fn embedded_selection_catalog() -> Result<SelectionCatalog, SelectionCatalogError> {
    decode_selection_catalog(EMBEDDED_SELECTION_CATALOG_BYTES)
}

fn validate_family(
    family: &'static str,
    prefix: &str,
    ids: &[String],
) -> Result<(), SelectionCatalogError> {
    let mut seen = HashSet::with_capacity(ids.len());
    for id in ids {
        validate_identifier(id, family)?;
        if !id
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        {
            return Err(SelectionCatalogError::InvalidIdentifier {
                family,
                id: id.clone(),
            });
        }
        if !seen.insert(id.to_ascii_lowercase()) {
            return Err(SelectionCatalogError::DuplicateIdentifier {
                family,
                id: id.clone(),
            });
        }
    }
    Ok(())
}

fn validate_identifier(id: &str, family: &'static str) -> Result<(), SelectionCatalogError> {
    if id.trim().is_empty() || id.contains('\0') {
        return Err(SelectionCatalogError::InvalidIdentifier {
            family,
            id: id.to_owned(),
        });
    }
    Ok(())
}

fn family_match_count(id: &str, ids: &SelectionCategories) -> usize {
    [
        &ids.apps,
        &ids.tweaks,
        &ids.toggles,
        &ids.features,
        &ids.appx,
    ]
    .iter()
    .filter(|family| family.iter().any(|known| known.eq_ignore_ascii_case(id)))
    .count()
}

/// Parse a JSON object without losing member order or silently overwriting duplicate keys.
struct OrderedMap<T>(Vec<(String, T)>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OrderedMap<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OrderedMapVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for OrderedMapVisitor<T> {
            type Value = OrderedMap<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an ordered JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(OrderedMap(entries))
            }
        }
        deserializer.deserialize_map(OrderedMapVisitor(std::marker::PhantomData))
    }
}

impl<'de> Deserialize<'de> for RawSelectionCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            schema_version: u32,
            provenance: RawProvenance,
            source_sha256: OrderedMap<String>,
            categories: RawCategories,
            named_presets: OrderedMap<Vec<String>>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            schema_version: raw.schema_version,
            _provenance: raw.provenance,
            source_sha256: raw.source_sha256.0,
            categories: raw.categories,
            named_presets: raw.named_presets.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_json() -> serde_json::Value {
        serde_json::from_slice(EMBEDDED_SELECTION_CATALOG_BYTES).unwrap()
    }

    fn encode(value: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }

    #[test]
    fn embedded_catalog_has_expected_counts_and_presets() {
        let catalog = embedded_selection_catalog().unwrap();
        assert_eq!(catalog.ids().apps.len(), 232);
        assert_eq!(catalog.ids().tweaks.len(), 38);
        assert_eq!(catalog.ids().toggles.len(), 23);
        assert_eq!(catalog.ids().features.len(), 10);
        assert_eq!(catalog.ids().appx.len(), 33);
        for (name, count) in [
            ("Standard", 12),
            ("Minimal", 4),
            ("Advanced", 17),
            ("AppxDefault", 18),
        ] {
            assert_eq!(catalog.preset(name).unwrap().len(), count);
        }
        assert_eq!(catalog.preset("sTaNdArD"), catalog.preset("Standard"));
    }

    #[test]
    fn rejects_malformed_oversized_and_unknown_schema() {
        assert!(matches!(
            decode_selection_catalog(b"{"),
            Err(SelectionCatalogError::InvalidJson(_))
        ));
        assert!(matches!(
            decode_selection_catalog(&vec![b' '; MAX_SELECTION_CATALOG_BYTES + 1]),
            Err(SelectionCatalogError::TooLarge)
        ));
        let mut value = raw_json();
        value["schema_version"] = 9.into();
        assert!(matches!(
            decode_selection_catalog(&encode(&value)),
            Err(SelectionCatalogError::UnsupportedSchemaVersion(9))
        ));
    }

    #[test]
    fn rejects_unknown_and_ambiguous_preset_ids() {
        let mut unknown = raw_json();
        unknown["named_presets"]["Standard"][0] = "WPFTweaksMissing".into();
        assert!(matches!(
            decode_selection_catalog(&encode(&unknown)),
            Err(SelectionCatalogError::UnknownPresetIdentifier { .. })
        ));

        let mut ambiguous = raw_json();
        let app_id = ambiguous["categories"]["apps"][0].clone();
        ambiguous["categories"]["toggles"]
            .as_array_mut()
            .unwrap()
            .push(app_id);
        assert!(matches!(
            decode_selection_catalog(&encode(&ambiguous)),
            Err(SelectionCatalogError::AmbiguousPresetIdentifier { .. })
                | Err(SelectionCatalogError::InvalidIdentifier { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_ids_and_unknown_fields() {
        let mut duplicate = raw_json();
        let first = duplicate["categories"]["apps"][0].clone();
        duplicate["categories"]["apps"]
            .as_array_mut()
            .unwrap()
            .push(first);
        assert!(matches!(
            decode_selection_catalog(&encode(&duplicate)),
            Err(SelectionCatalogError::DuplicateIdentifier { .. })
        ));

        let mut unknown = raw_json();
        unknown["unexpected"] = true.into();
        assert!(matches!(
            decode_selection_catalog(&encode(&unknown)),
            Err(SelectionCatalogError::InvalidJson(_))
        ));
    }
}
