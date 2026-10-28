//! Pure formatter port of `Get-WinUtilPackageLogSummary.ps1`.
//!
//! The source uses PowerShell's case-insensitive string comparisons. This
//! module uses Rust's ASCII-insensitive comparison for `na` and `Choco`; that
//! matches these ASCII tokens, while PowerShell's broader Unicode casing rules
//! may differ for non-ASCII input. Field values are never trimmed.

/// Package fields consumed by the upstream summary formatter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackageLogEntry<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub winget: Option<&'a str>,
    pub choco: Option<&'a str>,
}

/// Format each package in order, preserving duplicates and raw field text.
pub fn summarize_package_logs(packages: &[PackageLogEntry<'_>], preference: &str) -> Vec<String> {
    packages
        .iter()
        .map(|package| {
            let name = [
                package.name,
                package.description,
                package.winget,
                package.choco,
            ]
            .into_iter()
            .flatten()
            .find(|value| is_usable(value))
            .unwrap_or("Unknown package");

            if preference.eq_ignore_ascii_case("Choco") && package.choco.is_some_and(is_usable) {
                format!("{name} (choco: {})", package.choco.unwrap_or_default())
            } else if package.winget.is_some_and(is_usable) {
                format!("{name} (winget: {})", package.winget.unwrap_or_default())
            } else {
                format!("{name} (no package id)")
            }
        })
        .collect()
}

fn is_usable(value: &str) -> bool {
    !value.trim().is_empty() && !value.eq_ignore_ascii_case("na")
}

#[cfg(test)]
mod tests {
    use super::{summarize_package_logs, PackageLogEntry};

    #[test]
    fn chooses_first_usable_name_field_and_preferred_id() {
        let entries = [PackageLogEntry {
            name: Some("  "),
            description: Some("Description"),
            winget: Some("vendor.id"),
            choco: Some("choco-id"),
        }];

        assert_eq!(
            summarize_package_logs(&entries, "Choco"),
            ["Description (choco: choco-id)"]
        );
    }

    #[test]
    fn preference_falls_back_to_winget_when_choco_missing_or_na() {
        let entries = [
            PackageLogEntry {
                name: Some("One"),
                winget: Some("winget-one"),
                ..PackageLogEntry::default()
            },
            PackageLogEntry {
                name: Some("Two"),
                winget: Some("winget-two"),
                choco: Some("NA"),
                ..PackageLogEntry::default()
            },
        ];

        assert_eq!(
            summarize_package_logs(&entries, "cHoCo"),
            ["One (winget: winget-one)", "Two (winget: winget-two)"]
        );
    }

    #[test]
    fn ascii_case_insensitive_na_is_skipped_but_padded_na_is_usable() {
        let entries = [
            PackageLogEntry {
                name: Some("nA"),
                description: Some("Fallback"),
                winget: Some("na"),
                ..PackageLogEntry::default()
            },
            PackageLogEntry {
                name: Some(" na "),
                winget: Some(" na "),
                ..PackageLogEntry::default()
            },
        ];

        assert_eq!(
            summarize_package_logs(&entries, "winget"),
            ["Fallback (no package id)", " na  (winget:  na )"]
        );
    }

    #[test]
    fn output_preserves_whitespace_in_name_and_package_id() {
        let entries = [PackageLogEntry {
            name: Some("  Raw Name  "),
            winget: Some("  raw.id  "),
            ..PackageLogEntry::default()
        }];

        assert_eq!(
            summarize_package_logs(&entries, "other"),
            ["  Raw Name   (winget:   raw.id  )"]
        );
    }

    #[test]
    fn unknown_name_and_no_ids_fall_back_cleanly() {
        let entries = [PackageLogEntry {
            name: Some(" \t "),
            description: None,
            winget: Some("NA"),
            choco: Some(" "),
        }];

        assert_eq!(
            summarize_package_logs(&entries, "Choco"),
            ["Unknown package (no package id)"]
        );
    }

    #[test]
    fn empty_input_is_empty_and_order_and_duplicates_are_preserved() {
        assert!(summarize_package_logs(&[], "Choco").is_empty());
        let entry = PackageLogEntry {
            name: Some("Repeat"),
            winget: Some("id"),
            ..PackageLogEntry::default()
        };
        assert_eq!(
            summarize_package_logs(&[entry, entry], "Winget"),
            ["Repeat (winget: id)", "Repeat (winget: id)"]
        );
    }
}
