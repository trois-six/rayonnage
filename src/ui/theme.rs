//! Light / dark appearance.

use std::sync::OnceLock;

use gtk::glib;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    /// Appearance of the system.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Theme::System => t!("System"),
            Theme::Light => t!("Light"),
            Theme::Dark => t!("Dark"),
        }
    }
}

/// GTK theme name and dark preference before any change, to go back to the
/// system appearance.
static INITIAL: OnceLock<(Option<String>, bool)> = OnceLock::new();

/// Applies the appearance (immediately, to every window).
///
/// Desktop themes often exist in a single variant (a dark-only theme ignores
/// the light preference), so the forced light and dark modes use GTK's
/// built-in theme; "System" keeps the desktop theme.
pub fn apply(theme: Theme) {
    let Some(settings) = gtk::Settings::default() else { return };
    let (initial_name, initial_dark) = INITIAL
        .get_or_init(|| {
            (
                settings.gtk_theme_name().map(|n| n.to_string()),
                settings.is_gtk_application_prefer_dark_theme(),
            )
        })
        .clone();
    let has_color_scheme = settings.find_property("gtk-interface-color-scheme").is_some();
    match theme {
        Theme::System => {
            settings.set_gtk_theme_name(initial_name.as_deref());
            if !has_color_scheme {
                settings.set_gtk_application_prefer_dark_theme(initial_dark);
            }
        }
        Theme::Light | Theme::Dark => {
            settings.set_gtk_theme_name(Some("Default"));
            if !has_color_scheme {
                settings.set_gtk_application_prefer_dark_theme(theme == Theme::Dark);
            }
        }
    }
    set_color_scheme(&settings, theme);
}

/// GTK ≥ 4.20 chooses the variant with "gtk-interface-color-scheme" (the dark
/// preference above is deprecated there).
fn set_color_scheme(settings: &gtk::Settings, theme: Theme) {
    if settings.find_property("gtk-interface-color-scheme").is_none() {
        return;
    }
    let Some(ty) = glib::Type::from_name("GtkInterfaceColorScheme") else { return };
    let Some(class) = glib::EnumClass::with_type(ty) else { return };
    let nick = match theme {
        Theme::System => "default",
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    if let Some(value) = class.to_value_by_nick(nick) {
        settings.set_property_from_value("gtk-interface-color-scheme", &value);
    }
}
