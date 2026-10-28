//! Pure filtering for application catalog entries.
//!
//! This only selects iterator items; it does not change UI visibility or
//! category expansion state. IDs are catalog keys, equivalent to the suffix
//! after the `WPFInstall` prefix used by the PowerShell UI tags. Searches also
//! check the complete tag because the upstream UI searches its full `Tag`.
//!
//! Matching folds ASCII letters only. Non-ASCII Unicode characters must match
//! their exact UTF-8 spelling; this intentionally does not claim full
//! PowerShell Unicode case-insensitive comparison parity.

use crate::data::applications::ApplicationEntry;

/// Filter `(catalog_id, entry)` pairs by optional text and category criteria.
///
/// Blank search text disables text matching, and blank category values are
/// ignored. Active category and text criteria are combined with AND. Text is
/// a literal, ASCII-case-insensitive substring of `content`, `description`,
/// or the catalog ID. The input iterator's order is preserved.
pub fn filter_applications<'a, I, Id>(
    entries: I,
    search: &str,
    categories: &[&str],
) -> impl Iterator<Item = (Id, &'a ApplicationEntry)>
where
    I: IntoIterator<Item = (Id, &'a ApplicationEntry)>,
    Id: AsRef<str>,
{
    let search = (!search.trim().is_empty()).then(|| search.to_owned());
    let categories: Vec<String> = categories
        .iter()
        .copied()
        .filter(|category| !category.trim().is_empty())
        .map(str::to_owned)
        .collect();

    entries.into_iter().filter(move |(id, entry)| {
        let category_matches = categories.is_empty()
            || categories
                .iter()
                .any(|category| entry.category.eq_ignore_ascii_case(category));
        let text_matches = search.as_deref().is_none_or(|needle| {
            contains_ascii_case_insensitive(&entry.content, needle)
                || contains_ascii_case_insensitive(&entry.description, needle)
                || contains_ascii_case_insensitive(id.as_ref(), needle)
                || contains_ascii_case_insensitive(&format!("WPFInstall{}", id.as_ref()), needle)
        });

        category_matches && text_matches
    })
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::filter_applications;
    use crate::data::applications::ApplicationEntry;

    fn entry(category: &str, content: &str, description: &str) -> ApplicationEntry {
        ApplicationEntry {
            category: category.to_owned(),
            content: content.to_owned(),
            description: description.to_owned(),
            link: String::from("https://example.invalid/"),
            winget: String::from("Vendor.Example"),
            choco: None,
            foss: false,
        }
    }

    fn sample() -> Vec<(String, ApplicationEntry)> {
        vec![
            (
                "browser-key".into(),
                entry("Browsers", "First", "Private web"),
            ),
            ("Literal*[?]".into(), entry("Tools", "Second", "Utilities")),
            (
                "PowerToys".into(),
                entry("Tools", "Third", "Microsoft utility"),
            ),
        ]
    }

    #[test]
    fn whitespace_search_and_categories_disable_their_filters() {
        let entries = sample();
        let pairs = entries.iter().map(|(id, value)| (id.as_str(), value));

        assert_eq!(
            filter_applications(pairs, " \t\n", &[" ", "\t"])
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["browser-key", "Literal*[?]", "PowerToys"]
        );
    }

    #[test]
    fn active_text_and_category_filters_are_combined_with_and() {
        let entries = sample();
        let pairs = entries.iter().map(|(id, value)| (id.as_str(), value));

        assert_eq!(
            filter_applications(pairs, "private", &["browsers"])
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["browser-key"]
        );
    }

    #[test]
    fn matching_is_ascii_case_insensitive_and_literal_for_wildcard_chars() {
        let entries = sample();
        let pairs = entries.iter().map(|(id, value)| (id.as_str(), value));
        assert_eq!(
            filter_applications(pairs, "literal*[?]", &[])
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["Literal*[?]"]
        );

        let entries = sample();
        let pairs = entries.iter().map(|(id, value)| (id.as_str(), value));
        assert_eq!(
            filter_applications(pairs, "mIcRoSoFt", &[])
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["PowerToys"]
        );
    }

    #[test]
    fn searches_catalog_id_without_wpf_install_prefix() {
        let entries = sample();
        let pairs = entries.iter().map(|(id, value)| (id.as_str(), value));
        assert_eq!(
            filter_applications(pairs, "browser", &[])
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["browser-key"]
        );

        let entries = sample();
        let pairs = entries.iter().map(|(id, value)| (id.as_str(), value));
        assert_eq!(
            filter_applications(pairs, "WPFInstallbrowser-key", &[])
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["browser-key"]
        );
    }

    #[test]
    fn preserves_input_iterator_order() {
        let entries = sample();
        let pairs = entries.iter().rev().map(|(id, value)| (id.as_str(), value));
        assert_eq!(
            filter_applications(pairs, "", &[])
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            ["PowerToys", "Literal*[?]", "browser-key"]
        );
    }

    #[test]
    fn unicode_matching_requires_exact_character_spelling() {
        let entries = [("cafe", entry("Tools", "café", ""))];
        let pairs = entries.iter().map(|(id, value)| (*id, value));
        assert_eq!(filter_applications(pairs, "CAFÉ", &[]).count(), 0);
        let pairs = entries.iter().map(|(id, value)| (*id, value));
        assert_eq!(filter_applications(pairs, "café", &[]).count(), 1);
    }
}
