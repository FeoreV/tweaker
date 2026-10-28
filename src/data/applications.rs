//! Read-only, typed view of the bundled application reference catalog.
//!
//! Package identifiers are metadata only. This module has no installer,
//! process, network, or system-mutation behavior.

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Maximum number of bytes accepted by the application catalog decoder.
pub const MAX_APPLICATION_CATALOG_BYTES: usize = 1024 * 1024;

/// The preview catalog bundled with the application.
pub const EMBEDDED_APPLICATION_CATALOG: &[u8] =
    include_bytes!("../../ui/src/data/applications.json");

/// A catalog indexed by its stable, source-provided identifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCatalog {
    entries: BTreeMap<String, ApplicationEntry>,
}

impl ApplicationCatalog {
    /// Return the number of catalog entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether this catalog has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Find an entry by its catalog identifier, matching the upstream
    /// PowerShell hashtable's case-insensitive key lookup.
    pub fn get(&self, id: &str) -> Option<&ApplicationEntry> {
        self.entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(id))
            .map(|(_, entry)| entry)
    }

    /// Iterate over entries in identifier order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ApplicationEntry)> {
        self.entries.iter().map(|(id, entry)| (id.as_str(), entry))
    }
}

/// Descriptive fields and package IDs from the reference catalog.
///
/// Values are preserved as supplied. In particular, package IDs such as
/// `"na"` and empty Chocolatey IDs are not interpreted or executed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationEntry {
    pub category: String,
    pub content: String,
    pub description: String,
    pub link: String,
    pub winget: String,
    #[serde(default, deserialize_with = "deserialize_choco")]
    pub choco: Option<String>,
    pub foss: bool,
}

/// Catalog decoding and structural validation failures.
#[derive(Debug)]
pub enum ApplicationCatalogError {
    TooLarge,
    InvalidJson(serde_json::Error),
    InvalidIdentifier(String),
    DuplicateIdentifier(String),
    EmptyRequiredField { id: String, field: &'static str },
}

impl fmt::Display for ApplicationCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "application catalog exceeds the size limit"),
            Self::InvalidJson(error) => write!(f, "invalid application catalog JSON: {error}"),
            Self::InvalidIdentifier(id) => {
                write!(
                    f,
                    "application catalog contains an empty identifier: {id:?}"
                )
            }
            Self::DuplicateIdentifier(id) => {
                write!(
                    f,
                    "application catalog contains duplicate identifier {id:?}"
                )
            }
            Self::EmptyRequiredField { id, field } => {
                write!(f, "application {id:?} has an empty required field {field}")
            }
        }
    }
}

impl std::error::Error for ApplicationCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
            _ => None,
        }
    }
}

/// Decode and validate the keyed JSON application catalog.
///
/// Unknown entry fields, incorrect field types, IDs duplicated without regard
/// to ASCII case, empty IDs, empty required strings, and documents over the
/// byte limit are rejected.
/// This function only parses data; it does not resolve or run package IDs.
pub fn decode_applications(bytes: &[u8]) -> Result<ApplicationCatalog, ApplicationCatalogError> {
    if bytes.len() > MAX_APPLICATION_CATALOG_BYTES {
        return Err(ApplicationCatalogError::TooLarge);
    }

    let parsed: RawApplicationMap =
        serde_json::from_slice(bytes).map_err(ApplicationCatalogError::InvalidJson)?;
    let mut entries: BTreeMap<String, ApplicationEntry> = BTreeMap::new();

    for (id, entry) in parsed.0 {
        if id.trim().is_empty() || id.contains('\0') {
            return Err(ApplicationCatalogError::InvalidIdentifier(id));
        }
        if entries
            .keys()
            .any(|existing| existing.eq_ignore_ascii_case(&id))
        {
            return Err(ApplicationCatalogError::DuplicateIdentifier(id));
        }
        validate_required_fields(&id, &entry)?;
        entries.insert(id, entry);
    }

    Ok(ApplicationCatalog { entries })
}

/// Decode the catalog embedded at compile time from the preview source file.
pub fn embedded_applications() -> Result<ApplicationCatalog, ApplicationCatalogError> {
    decode_applications(EMBEDDED_APPLICATION_CATALOG)
}

fn validate_required_fields(
    id: &str,
    entry: &ApplicationEntry,
) -> Result<(), ApplicationCatalogError> {
    for (field, value) in [
        ("category", entry.category.as_str()),
        ("content", entry.content.as_str()),
        ("description", entry.description.as_str()),
        ("link", entry.link.as_str()),
        ("winget", entry.winget.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ApplicationCatalogError::EmptyRequiredField {
                id: id.to_owned(),
                field,
            });
        }
    }
    Ok(())
}

/// Treat an absent `choco` field as `None`, but reject explicit JSON null or
/// non-string values so a present field always has the documented string type.
fn deserialize_choco<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

struct RawApplicationMap(Vec<(String, ApplicationEntry)>);

impl<'de> Deserialize<'de> for RawApplicationMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawApplicationMapVisitor;

        impl<'de> Visitor<'de> for RawApplicationMapVisitor {
            type Value = RawApplicationMap;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object mapping application IDs to application entries")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(entry) = map.next_entry::<String, ApplicationEntry>()? {
                    entries.push(entry);
                }
                Ok(RawApplicationMap(entries))
            }
        }

        deserializer.deserialize_map(RawApplicationMapVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_entry(id: &str, extra_fields: &str) -> String {
        format!(
            r#"{{"{id}":{{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","winget":"Vendor.Example"{extra_fields},"foss":false}}}}"#
        )
    }

    #[test]
    fn embedded_catalog_decodes_and_preserves_optional_package_metadata() {
        let catalog = embedded_applications().unwrap();
        assert_eq!(catalog.len(), 232);
        assert!(!catalog.is_empty());

        let seven_zip = catalog.get("7zip").unwrap();
        assert!(std::ptr::eq(seven_zip, catalog.get("7ZIP").unwrap()));
        assert_eq!(seven_zip.content, "7-Zip");
        assert_eq!(seven_zip.winget, "7zip.7zip");
        assert_eq!(seven_zip.choco.as_deref(), Some("7zip"));
        assert!(seven_zip.foss);

        let missing_choco = catalog.get("mpv").unwrap();
        assert_eq!(missing_choco.choco, None);

        let not_listed = catalog.get("battlenet").unwrap();
        assert_eq!(not_listed.choco.as_deref(), Some("na"));

        let empty_package = catalog.get("es-de").unwrap();
        assert_eq!(empty_package.choco.as_deref(), Some(""));

        assert_eq!(catalog.iter().count(), catalog.len());
    }

    #[test]
    fn rejects_duplicate_ids_instead_of_overwriting() {
        let entry = r#"{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","winget":"Vendor.Example","foss":false}"#;
        let bytes = format!(r#"{{"duplicate":{entry},"duplicate":{entry}}}"#);
        assert!(matches!(
            decode_applications(bytes.as_bytes()),
            Err(ApplicationCatalogError::DuplicateIdentifier(id)) if id == "duplicate"
        ));
    }

    #[test]
    fn rejects_ascii_case_insensitive_duplicate_ids() {
        let entry = r#"{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","winget":"Vendor.Example","foss":false}"#;
        let bytes = format!(r#"{{"foo":{entry},"FOO":{entry}}}"#);
        assert!(matches!(
            decode_applications(bytes.as_bytes()),
            Err(ApplicationCatalogError::DuplicateIdentifier(id)) if id == "FOO"
        ));
    }

    #[test]
    fn rejects_unknown_fields_and_wrong_types() {
        let unknown_field = r#"{"example":{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","winget":"Vendor.Example","foss":false,"extra":true}}"#;
        let wrong_winget_type = r#"{"example":{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","winget":42,"foss":false}}"#;
        let null_choco = r#"{"example":{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","winget":"Vendor.Example","choco":null,"foss":false}}"#;
        let wrong_foss_type = r#"{"example":{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","winget":"Vendor.Example","foss":"false"}}"#;

        assert!(matches!(
            decode_applications(unknown_field.as_bytes()),
            Err(ApplicationCatalogError::InvalidJson(_))
        ));
        assert!(matches!(
            decode_applications(wrong_winget_type.as_bytes()),
            Err(ApplicationCatalogError::InvalidJson(_))
        ));
        assert!(matches!(
            decode_applications(null_choco.as_bytes()),
            Err(ApplicationCatalogError::InvalidJson(_))
        ));
        assert!(matches!(
            decode_applications(wrong_foss_type.as_bytes()),
            Err(ApplicationCatalogError::InvalidJson(_))
        ));
    }

    #[test]
    fn rejects_oversized_catalogs() {
        let bytes = vec![b' '; MAX_APPLICATION_CATALOG_BYTES + 1];
        assert!(matches!(
            decode_applications(&bytes),
            Err(ApplicationCatalogError::TooLarge)
        ));
    }

    #[test]
    fn rejects_missing_or_empty_required_fields_and_empty_ids() {
        let missing = r#"{"example":{"category":"Utilities","content":"Example","description":"Example app","link":"https://example.invalid/","foss":false}}"#;
        assert!(matches!(
            decode_applications(missing.as_bytes()),
            Err(ApplicationCatalogError::InvalidJson(_))
        ));

        let empty_content =
            valid_entry("example", "").replace("Example\",\"description", " \",\"description");
        assert!(matches!(
            decode_applications(empty_content.as_bytes()),
            Err(ApplicationCatalogError::EmptyRequiredField {
                field: "content",
                ..
            })
        ));

        let empty_id = valid_entry("   ", "");
        assert!(matches!(
            decode_applications(empty_id.as_bytes()),
            Err(ApplicationCatalogError::InvalidIdentifier(_))
        ));
    }
}
