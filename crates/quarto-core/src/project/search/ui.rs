/*
 * project/search/ui.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Where the search box renders (bd-eip0naf4, bd-6cme).
 */

//! Placement decisions shared by the navbar, sidebar, and secondary-nav
//! transforms — Quarto 1's `navbarEjsData` / `sidebarEjsData` rule:
//!
//! ```text
//! search: explicit !== undefined ? explicit
//!       : searchOpts && searchOpts.location === <this surface> ? searchOpts.type
//!       : false
//! ```
//!
//! An explicit `navbar.search` / `sidebar.search` always wins; unset
//! follows `website.search.location`. For the sidebar an explicit
//! `true` renders the inline textbox (Quarto 1 compares against the
//! string `"overlay"`, so the boolean never matches it).

use quarto_pandoc_types::ConfigValue;

use super::config::{SearchLocation, SearchType, resolve_website_search};

/// Does the navbar render the search box?
pub fn navbar_search_enabled(meta: &ConfigValue, explicit: Option<bool>) -> bool {
    let mut scratch = Vec::new();
    let Some(opts) = resolve_website_search(meta, &mut scratch) else {
        return false;
    };
    explicit.unwrap_or(opts.location == SearchLocation::Navbar)
}

/// Does the sidebar render the search box, and as what?
pub fn sidebar_search_kind(meta: &ConfigValue, explicit: Option<bool>) -> Option<SearchType> {
    let mut scratch = Vec::new();
    let opts = resolve_website_search(meta, &mut scratch)?;
    match explicit {
        Some(false) => None,
        Some(true) => Some(if opts.location == SearchLocation::Sidebar {
            opts.kind
        } else {
            SearchType::Textbox
        }),
        None => (opts.location == SearchLocation::Sidebar).then_some(opts.kind),
    }
}

/// The 11 `search-*` language terms the UI reads, with Quarto 1's
/// English defaults for a metadata table that lacks them.
pub const SEARCH_TERMS: &[(&str, &str)] = &[
    ("search-no-results-text", "No results"),
    ("search-matching-documents-text", "matching documents"),
    ("search-copy-link-title", "Copy link to search"),
    ("search-hide-matches-text", "Hide additional matches"),
    ("search-more-match-text", "more match in this document"),
    ("search-more-matches-text", "more matches in this document"),
    ("search-clear-button-title", "Clear"),
    ("search-text-placeholder", ""),
    ("search-detached-cancel-button-title", "Cancel"),
    ("search-submit-button-title", "Submit"),
    ("search-label", "Search"),
];

/// The localized `search-label` term (the box's `title` / the
/// secondary-nav button's `aria-label`).
pub fn search_label(meta: &ConfigValue) -> String {
    crate::language::LanguageTerms::from_meta(meta)
        .and_then(|t| t.get("search-label").map(str::to_owned))
        .unwrap_or_else(|| "Search".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(yaml: &str) -> ConfigValue {
        let parsed = quarto_yaml::parse(yaml).expect("fixture yaml parses");
        let (config, _) = crate::cell_options::options_to_config(parsed);
        config
    }

    #[test]
    fn navbar_follows_location_unless_explicit() {
        let with_navbar = meta("website:\n  navbar:\n    left: [a.qmd]\n");
        assert!(navbar_search_enabled(&with_navbar, None));
        assert!(!navbar_search_enabled(&with_navbar, Some(false)));
        let sidebar_loc =
            meta("website:\n  navbar:\n    left: [a.qmd]\n  search:\n    location: sidebar\n");
        assert!(!navbar_search_enabled(&sidebar_loc, None));
        assert!(navbar_search_enabled(&sidebar_loc, Some(true)));
        assert!(!navbar_search_enabled(
            &meta("website:\n  search: false\n"),
            Some(true)
        ));
    }

    #[test]
    fn sidebar_follows_location_and_explicit_true_is_textbox() {
        let no_navbar = meta("website:\n  title: x\n");
        assert_eq!(
            sidebar_search_kind(&no_navbar, None),
            Some(SearchType::Textbox)
        );
        assert_eq!(sidebar_search_kind(&no_navbar, Some(false)), None);
        let overlay_in_sidebar =
            meta("website:\n  search:\n    location: sidebar\n    type: overlay\n");
        assert_eq!(
            sidebar_search_kind(&overlay_in_sidebar, None),
            Some(SearchType::Overlay)
        );
        assert_eq!(
            sidebar_search_kind(&overlay_in_sidebar, Some(true)),
            Some(SearchType::Overlay)
        );
        let navbar_site = meta("website:\n  navbar:\n    left: [a.qmd]\n");
        assert_eq!(sidebar_search_kind(&navbar_site, None), None);
        assert_eq!(
            sidebar_search_kind(&navbar_site, Some(true)),
            Some(SearchType::Textbox)
        );
        assert_eq!(
            sidebar_search_kind(&meta("website:\n  search: false\n"), Some(true)),
            None
        );
    }

    #[test]
    fn search_label_defaults_to_english() {
        assert_eq!(search_label(&meta("title: x\n")), "Search");
    }
}
