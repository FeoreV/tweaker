//! Bounded, text-only matching of Chocolatey list output against catalog metadata.
//!
//! This module does not run Chocolatey or establish that a package is installed.
//! It accepts caller-provided bytes (up to 1 MiB), rejects invalid UTF-8, and
//! returns catalog IDs whose final semicolon-delimited `choco` segment appears
//! as a whole first token on a matching list-output line. Catalog IDs are
//! returned in `ApplicationCatalog`'s stable identifier order.

use crate::data::applications::ApplicationCatalog;
use std::collections::HashSet;
use std::fmt;

/// Maximum accepted size of caller-provided Chocolatey list text.
pub const MAX_CHOCOLATEY_LIST_BYTES: usize = 1024 * 1024;

/// Failure to safely interpret supplied Chocolatey list text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChocolateyListError {
    TooLarge,
    InvalidUtf8,
}

impl fmt::Display for ChocolateyListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "Chocolatey list text exceeds the byte limit"),
            Self::InvalidUtf8 => write!(f, "Chocolatey list text is not valid UTF-8"),
        }
    }
}

impl std::error::Error for ChocolateyListError {}

/// Maximum accepted size of caller-provided WinGet list text.
pub const MAX_WINGET_LIST_BYTES: usize = 1024 * 1024;

/// Failure to safely interpret supplied WinGet list text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WingetListError {
    TooLarge,
    InvalidUtf8,
}

impl fmt::Display for WingetListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "WinGet list text exceeds the byte limit"),
            Self::InvalidUtf8 => write!(f, "WinGet list text is not valid UTF-8"),
        }
    }
}

impl std::error::Error for WingetListError {}

/// Match catalog WinGet IDs against caller-provided list text.
///
/// This reproduces the upstream line regex's literal ID and whitespace
/// boundaries without running WinGet. Catalog IDs are the last `;` segment;
/// a leading `msstore:` is removed case-insensitively before trimming, then
/// empty and `na` values are ignored. Comparison is ASCII case-insensitive.
/// Non-ASCII IDs are deliberately skipped because .NET's Unicode
/// case-insensitive regex folding is not equivalent to Rust's ASCII folding.
/// This is metadata matching only and does not establish installed state.
pub fn match_winget_list_bytes(
    catalog: &ApplicationCatalog,
    list_text: &[u8],
) -> Result<Vec<String>, WingetListError> {
    if list_text.len() > MAX_WINGET_LIST_BYTES {
        return Err(WingetListError::TooLarge);
    }
    let text = std::str::from_utf8(list_text).map_err(|_| WingetListError::InvalidUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    // Build the set of IDs admitted by the source regex in one text pass.
    let mut candidates = HashSet::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut runs = Vec::new();
        let mut run_start = None;
        for (index, ch) in line.char_indices() {
            if is_horizontal_whitespace(ch) {
                run_start.get_or_insert(index);
            } else if let Some(start) = run_start.take() {
                runs.push((start, index));
            }
        }
        if let Some(start) = run_start {
            runs.push((start, line.len()));
        }

        for (index, &(start, end)) in runs.iter().enumerate() {
            if end - start < 2 {
                continue;
            }
            let id_start = end;
            let id_end = runs
                .get(index + 1)
                .map_or(line.len(), |(next_start, _)| *next_start);
            let id = &line[id_start..id_end];
            let right_boundary_ok = runs
                .get(index + 1)
                .is_some_and(|(next_start, next_end)| next_end - next_start >= 2)
                || index + 1 == runs.len();
            if right_boundary_ok && !id.is_empty() && id.is_ascii() {
                candidates.insert(id.to_ascii_lowercase());
            }
        }
    }

    Ok(catalog
        .iter()
        .filter_map(|(catalog_id, entry)| {
            let raw = entry.winget.rsplit(';').next()?;
            let raw = strip_msstore_prefix(raw);
            let id = raw.trim();
            if id.is_empty() || id.eq_ignore_ascii_case("na") || !id.is_ascii() {
                return None;
            }
            candidates
                .contains(&id.to_ascii_lowercase())
                .then(|| catalog_id.to_owned())
        })
        .collect())
}

fn strip_msstore_prefix(value: &str) -> &str {
    value
        .get(..8)
        .filter(|prefix| prefix.eq_ignore_ascii_case("msstore:"))
        .map_or(value, |_| &value[8..])
}

fn is_horizontal_whitespace(ch: char) -> bool {
    ch != '\r' && ch != '\n' && ch.is_whitespace()
}

/// Match catalog package candidates against text supplied by the caller.
///
/// Each input line contributes its first non-whitespace token only when the
/// line itself starts with a non-whitespace character, matching the anchored
/// `^\\S+` pattern. Candidate IDs are the trimmed final `;` segment; missing,
/// empty, and case-insensitive `na` candidates are ignored. Comparison uses
/// ASCII case-insensitivity, as Chocolatey package IDs in the catalog are
/// ASCII. The result is metadata matching only, not an installed-state claim.
pub fn match_chocolatey_list_bytes(
    catalog: &ApplicationCatalog,
    list_text: &[u8],
) -> Result<Vec<String>, ChocolateyListError> {
    if list_text.len() > MAX_CHOCOLATEY_LIST_BYTES {
        return Err(ChocolateyListError::TooLarge);
    }
    let text = std::str::from_utf8(list_text).map_err(|_| ChocolateyListError::InvalidUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    let tokens: HashSet<String> = text
        .lines()
        .filter_map(|line| {
            let first = line.chars().next()?;
            if first.is_whitespace() {
                return None;
            }
            let end = line
                .char_indices()
                .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
                .unwrap_or(line.len());
            Some(line[..end].to_ascii_lowercase())
        })
        .collect();

    Ok(catalog
        .iter()
        .filter_map(|(catalog_id, entry)| {
            let raw = entry.choco.as_deref()?;
            let candidate = raw.rsplit(';').next()?.trim();
            if candidate.is_empty() || candidate.eq_ignore_ascii_case("na") {
                return None;
            }
            tokens
                .contains(&candidate.to_ascii_lowercase())
                .then(|| catalog_id.to_owned())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::applications::decode_applications;

    fn catalog(entries: &[(&str, Option<&str>)]) -> ApplicationCatalog {
        let mut json = String::from("{");
        for (index, (id, choco)) in entries.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            let choco = choco
                .map(|value| format!(",\"choco\":{}", serde_json::to_string(value).unwrap()))
                .unwrap_or_default();
            json.push_str(&format!(
                "{}:{{\"category\":\"Utilities\",\"content\":\"{id}\",\"description\":\"{id}\",\"link\":\"https://example.test\",\"winget\":\"{id}\"{choco},\"foss\":true}}",
                serde_json::to_string(id).unwrap()
            ));
        }
        json.push('}');
        decode_applications(json.as_bytes()).unwrap()
    }

    #[test]
    fn last_semicolon_segment_is_trimmed_and_matched() {
        let catalog = catalog(&[("app", Some("first; middle ; final-id  "))]);
        assert_eq!(
            match_chocolatey_list_bytes(&catalog, b"final-id 1.2.3\n").unwrap(),
            ["app"]
        );
    }

    #[test]
    fn skips_missing_empty_and_na_candidates_case_insensitively() {
        let catalog = catalog(&[
            ("missing", None),
            ("empty", Some("   ")),
            ("semi_empty", Some("name;  ")),
            ("na", Some("nA")),
            ("semi_na", Some("first; NA ")),
        ]);
        assert!(match_chocolatey_list_bytes(&catalog, b"name\nna\n")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn anchored_pattern_ignores_lines_with_leading_whitespace() {
        let catalog = catalog(&[("app", Some("pkg"))]);
        assert!(match_chocolatey_list_bytes(&catalog, b"  pkg 1.0\n\tpkg\n")
            .unwrap()
            .is_empty());
        assert_eq!(
            match_chocolatey_list_bytes(&catalog, b"pkg 1.0\n").unwrap(),
            ["app"]
        );
    }

    #[test]
    fn matches_exact_first_token_not_substrings_or_later_tokens() {
        let catalog = catalog(&[("app", Some("pkg"))]);
        for text in [b"pkg-extra 1.0\n".as_slice(), b"other pkg\n".as_slice()] {
            assert!(match_chocolatey_list_bytes(&catalog, text)
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn package_id_comparison_is_ascii_case_insensitive() {
        let catalog = catalog(&[("app", Some("Vendor.Pkg"))]);
        assert_eq!(
            match_chocolatey_list_bytes(&catalog, b"vendor.pkg 4.0\n").unwrap(),
            ["app"]
        );
    }

    #[test]
    fn strips_a_leading_utf8_bom_before_matching() {
        let catalog = catalog(&[("app", Some("pkg"))]);
        assert_eq!(
            match_chocolatey_list_bytes(&catalog, "\u{feff}pkg 1.0\n".as_bytes()).unwrap(),
            ["app"]
        );
    }

    #[test]
    fn duplicate_tokens_do_not_duplicate_results_and_output_is_sorted() {
        let catalog = catalog(&[("z-app", Some("pkg")), ("a-app", Some("PKG"))]);
        assert_eq!(
            match_chocolatey_list_bytes(&catalog, b"pkg 1\nPKG 2\npkg 3\n").unwrap(),
            ["a-app", "z-app"]
        );
    }

    #[test]
    fn many_repeated_tokens_are_indexed_without_changing_matches() {
        let catalog = catalog(&[("app", Some("pkg")), ("other", Some("missing"))]);
        let input = "pkg 1.0\n".repeat(20_000);
        assert_eq!(
            match_chocolatey_list_bytes(&catalog, input.as_bytes()).unwrap(),
            ["app"]
        );
    }

    #[test]
    fn empty_input_and_no_entries_return_empty_results() {
        assert!(
            match_chocolatey_list_bytes(&catalog(&[("app", Some("pkg"))]), b"")
                .unwrap()
                .is_empty()
        );
        assert!(match_chocolatey_list_bytes(&catalog(&[]), b"pkg\n")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_invalid_utf8_and_oversized_input() {
        assert_eq!(
            match_chocolatey_list_bytes(&catalog(&[]), &[0xff]),
            Err(ChocolateyListError::InvalidUtf8)
        );
        assert_eq!(
            match_chocolatey_list_bytes(&catalog(&[]), &vec![b'x'; MAX_CHOCOLATEY_LIST_BYTES + 1]),
            Err(ChocolateyListError::TooLarge)
        );
    }

    #[test]
    fn winget_strips_store_prefix_before_trimming() {
        // Use a separately decoded entry whose WinGet ID carries the prefix.
        let catalog = catalog_with_winget(&[("app", "msstore: Store.Id")]);
        assert_eq!(
            match_winget_list_bytes(&catalog, b"Name  Store.Id\n").unwrap(),
            ["app"]
        );
        let catalog = catalog_with_winget(&[("app", "msstore:Store.Id")]);
        assert!(
            match_winget_list_bytes(&catalog, b"Name  msstore:Store.Id\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn winget_skips_empty_and_na_candidates() {
        let catalog = catalog_with_winget(&[("empty", "na; "), ("na", "NA"), ("semi", "a; na")]);
        assert!(match_winget_list_bytes(&catalog, b"Name  NA\n")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn winget_requires_literal_id_and_two_whitespace_boundaries() {
        let catalog = catalog_with_winget(&[("literal", "Vendor.Pkg+Test(1)")]);
        assert_eq!(
            match_winget_list_bytes(&catalog, b"Name  Vendor.Pkg+Test(1)\n").unwrap(),
            ["literal"]
        );
        for text in [
            b"Name Vendor.Pkg+Test(1)\n".as_slice(),
            b"Name  Vendor.Pkg+Test(1) x\n".as_slice(),
            b"Name  Vendor.Pkg+Test(1) x\n".as_slice(),
            b"Name  Vendor.Pkg+Test(1)-extra\n".as_slice(),
            b"Name  XVendor.Pkg+Test(1)\n".as_slice(),
        ] {
            assert!(match_winget_list_bytes(&catalog, text).unwrap().is_empty());
        }
        assert_eq!(
            match_winget_list_bytes(
                &catalog,
                "Name\u{2003}\u{2003}Vendor.Pkg+Test(1)\r\n".as_bytes()
            )
            .unwrap(),
            ["literal"]
        );
    }

    #[test]
    fn winget_results_are_unique_and_follow_catalog_order() {
        let catalog = catalog_with_winget(&[("z", "Pkg"), ("a", "pkg")]);
        assert_eq!(
            match_winget_list_bytes(&catalog, b"Name  Pkg\nName  pkg\n").unwrap(),
            ["a", "z"]
        );
    }

    #[test]
    fn winget_large_multicolumn_input_matches_without_rechecking_catalog_per_run() {
        let owned_ids: Vec<String> = (0..96)
            .map(|index| format!("Vendor.Pkg{index:03}"))
            .collect();
        // Catalog identifiers must be unique, so rebuild entry IDs independently.
        let entries: Vec<(String, &str)> = owned_ids
            .iter()
            .enumerate()
            .map(|(index, id)| (format!("app-{index:03}"), id.as_str()))
            .collect();
        let entry_refs: Vec<(&str, &str)> = entries
            .iter()
            .map(|(app, winget)| (app.as_str(), *winget))
            .collect();
        let catalog = catalog_with_winget(&entry_refs);

        let mut input = String::new();
        for row in 0..128 {
            input.push_str("Name  ");
            for column in 0..32 {
                if column != 0 {
                    input.push_str("  ");
                }
                let index = (row * 32 + column) % owned_ids.len();
                input.push_str(&owned_ids[index]);
            }
            input.push_str("  Version\r\n");
        }
        assert_eq!(
            match_winget_list_bytes(&catalog, input.as_bytes())
                .unwrap()
                .len(),
            owned_ids.len()
        );
    }

    #[test]
    fn winget_handles_bom_and_rejects_bad_or_oversized_input() {
        let catalog = catalog_with_winget(&[("app", "Pkg")]);
        assert_eq!(
            match_winget_list_bytes(&catalog, "\u{feff}Name  Pkg\r\n".as_bytes()).unwrap(),
            ["app"]
        );
        assert_eq!(
            match_winget_list_bytes(&catalog, &[0xff]),
            Err(WingetListError::InvalidUtf8)
        );
        assert_eq!(
            match_winget_list_bytes(&catalog, &vec![b'x'; MAX_WINGET_LIST_BYTES + 1]),
            Err(WingetListError::TooLarge)
        );
    }

    fn catalog_with_winget(entries: &[(&str, &str)]) -> ApplicationCatalog {
        let mut json = String::from("{");
        for (index, (id, winget)) in entries.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{}:{{\"category\":\"Utilities\",\"content\":\"{id}\",\"description\":\"{id}\",\"link\":\"https://example.test\",\"winget\":{},\"foss\":true}}",
                serde_json::to_string(id).unwrap(), serde_json::to_string(winget).unwrap()
            ));
        }
        json.push('}');
        decode_applications(json.as_bytes()).unwrap()
    }
}
