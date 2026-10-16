/*
 * list.rs
 * Copyright (c) 2025 Posit, PBC
 */

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ListNumberStyle {
    Default,
    Example,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ListNumberDelim {
    Default,
    Period,
    OneParen,
    TwoParens,
}

pub type ListAttributes = (usize, ListNumberStyle, ListNumberDelim);

impl ListNumberStyle {
    /// Every variant, in pandoc's declaration order.
    pub const ALL: [ListNumberStyle; 7] = [
        ListNumberStyle::Default,
        ListNumberStyle::Example,
        ListNumberStyle::Decimal,
        ListNumberStyle::LowerRoman,
        ListNumberStyle::UpperRoman,
        ListNumberStyle::LowerAlpha,
        ListNumberStyle::UpperAlpha,
    ];

    /// Pandoc's constructor name for this style: the `t` tag of the JSON
    /// format, the word in the native format, and the string the Lua API
    /// uses. `Default` is spelled `DefaultStyle`, not `Default`; pandoc
    /// rejects anything else (bd-3h6iasmz).
    pub fn pandoc_name(&self) -> &'static str {
        match self {
            ListNumberStyle::Default => "DefaultStyle",
            ListNumberStyle::Example => "Example",
            ListNumberStyle::Decimal => "Decimal",
            ListNumberStyle::LowerRoman => "LowerRoman",
            ListNumberStyle::UpperRoman => "UpperRoman",
            ListNumberStyle::LowerAlpha => "LowerAlpha",
            ListNumberStyle::UpperAlpha => "UpperAlpha",
        }
    }

    /// The inverse of [`pandoc_name`](Self::pandoc_name); `None` for a
    /// string that is not one of pandoc's constructor names.
    pub fn from_pandoc_name(name: &str) -> Option<ListNumberStyle> {
        Self::ALL.into_iter().find(|s| s.pandoc_name() == name)
    }
}

impl ListNumberDelim {
    /// Every variant, in pandoc's declaration order.
    pub const ALL: [ListNumberDelim; 4] = [
        ListNumberDelim::Default,
        ListNumberDelim::Period,
        ListNumberDelim::OneParen,
        ListNumberDelim::TwoParens,
    ];

    /// Pandoc's constructor name for this delimiter; see
    /// [`ListNumberStyle::pandoc_name`]. `Default` is `DefaultDelim`.
    pub fn pandoc_name(&self) -> &'static str {
        match self {
            ListNumberDelim::Default => "DefaultDelim",
            ListNumberDelim::Period => "Period",
            ListNumberDelim::OneParen => "OneParen",
            ListNumberDelim::TwoParens => "TwoParens",
        }
    }

    /// The inverse of [`pandoc_name`](Self::pandoc_name).
    pub fn from_pandoc_name(name: &str) -> Option<ListNumberDelim> {
        Self::ALL.into_iter().find(|d| d.pandoc_name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pandoc_names_round_trip_and_spell_default_as_pandoc_does() {
        for style in ListNumberStyle::ALL {
            assert_eq!(
                ListNumberStyle::from_pandoc_name(style.pandoc_name()),
                Some(style)
            );
        }
        for delim in ListNumberDelim::ALL {
            assert_eq!(
                ListNumberDelim::from_pandoc_name(delim.pandoc_name()),
                Some(delim)
            );
        }
        assert_eq!(ListNumberStyle::Default.pandoc_name(), "DefaultStyle");
        assert_eq!(ListNumberDelim::Default.pandoc_name(), "DefaultDelim");
        // The Rust variant name is not a pandoc name.
        assert_eq!(ListNumberStyle::from_pandoc_name("Default"), None);
        assert_eq!(ListNumberDelim::from_pandoc_name("Default"), None);
    }
}
