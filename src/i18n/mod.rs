//! User interface translations.
//!
//! Strings are written in English in the code and wrapped in `t!`:
//!
//! ```ignore
//! label.set_text(t!("Scan finished"));
//! let msg = t!("{0} book(s) added", count); // positional placeholders
//! ```
//!
//! The French catalogs (`fr_*.rs`) map each English string to its
//! translation; a missing entry falls back to the English text. The language
//! is chosen once at startup (setting, or the system locale).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

mod fr_bedetheque;
mod fr_core;
mod fr_dialogs;
mod fr_organizer;
mod fr_ui;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// Language of the operating system.
    #[default]
    Auto,
    English,
    French,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Auto, Language::English, Language::French];

    /// Name shown in the settings (each language in its own language).
    pub fn label(self) -> &'static str {
        match self {
            Language::Auto => crate::t!("System language"),
            Language::English => "English",
            Language::French => "Français",
        }
    }
}

static CATALOG: OnceLock<Option<HashMap<&'static str, &'static str>>> = OnceLock::new();

/// Language of the system locale (French when it starts with "fr").
fn system_is_french() -> bool {
    for var in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
            && v != "C"
            && v != "POSIX"
        {
            return v.to_lowercase().starts_with("fr");
        }
    }
    sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("fr"))
}

/// Selects the language; to be called once, before any `t!`.
pub fn init(lang: Language) {
    let french = match lang {
        Language::Auto => system_is_french(),
        Language::English => false,
        Language::French => true,
    };
    let _ = CATALOG.set(french.then(|| {
        [
            fr_core::ENTRIES,
            fr_ui::ENTRIES,
            fr_dialogs::ENTRIES,
            fr_bedetheque::ENTRIES,
            fr_organizer::ENTRIES,
        ]
        .iter()
        .flat_map(|e| e.iter().copied())
        .collect()
    }));
}

/// Whether the interface is in French.
pub fn is_french() -> bool {
    matches!(CATALOG.get(), Some(Some(_)))
}

/// Translation of an English interface string.
pub fn tr(english: &'static str) -> &'static str {
    match CATALOG.get() {
        Some(Some(map)) => map.get(english).copied().unwrap_or(english),
        _ => english,
    }
}

/// Replaces the `{0}`, `{1}`… placeholders of a translated string.
pub fn format(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}').and_then(|end| after[..end].parse::<usize>().ok().map(|i| (i, end))) {
            Some((i, end)) if i < args.len() => {
                out.push_str(&args[i].to_string());
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Translates an English interface string: `t!("Text")` gives a
/// `&'static str`, `t!("Text {0}", value)` a `String`.
#[macro_export]
macro_rules! t {
    ($s:literal) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::format($crate::i18n::tr($s), &[$(&$arg as &dyn ::std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        assert_eq!(format("{1} sur {0}", &[&3, &"x"]), "x sur 3");
        assert_eq!(format("{a} {0}", &[&1]), "{a} 1");
    }

    /// Every catalog entry must keep the placeholders of its English key.
    #[test]
    fn catalogs_keep_placeholders() {
        let all = [
            fr_core::ENTRIES,
            fr_ui::ENTRIES,
            fr_dialogs::ENTRIES,
            fr_bedetheque::ENTRIES,
            fr_organizer::ENTRIES,
        ];
        let re = regex::Regex::new(r"\{\d+\}").unwrap();
        for (en, fr) in all.iter().flat_map(|e| e.iter()) {
            let mut a: Vec<_> = re.find_iter(en).map(|m| m.as_str()).collect();
            let mut b: Vec<_> = re.find_iter(fr).map(|m| m.as_str()).collect();
            a.sort();
            b.sort();
            assert_eq!(a, b, "placeholders differ: {en:?} / {fr:?}");
        }
    }
}
