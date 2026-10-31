//! Application settings, persisted as JSON in the user configuration folder.

use std::path::PathBuf;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::bedetheque::ScraperSettings;
use crate::convert::ExportSettings;
use crate::library::{project_dirs, write_atomic};
use crate::model::Field;
use crate::organizer::OrganizerSettings;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnSetting {
    pub field: Field,
    pub visible: bool,
    pub width: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Folders scanned for books.
    pub library_folders: Vec<PathBuf>,
    pub scan_on_startup: bool,
    /// Interface language (applied at the next start).
    pub language: crate::i18n::Language,
    /// Light / dark appearance.
    pub theme: crate::ui::theme::Theme,
    /// Remove books whose file disappeared when scanning.
    pub remove_missing: bool,
    /// Folders listed and books read at the same time during a scan (useful
    /// on network drives, where each read waits for the network).
    pub scan_threads: usize,
    /// "Allow writing of Book info into files".
    pub allow_write_info: bool,
    /// "Book files are updated automatically" after each edit.
    pub auto_update_files: bool,
    pub rar_path: String,
    pub ddjvu_path: String,
    pub djvused_path: String,
    pub pdftoppm_path: String,
    /// Column order, visibility and width of the book list.
    #[serde(deserialize_with = "skip_unreadable")]
    pub columns: Vec<ColumnSetting>,
    #[serde(deserialize_with = "none_if_unreadable")]
    pub sort_field: Option<Field>,
    pub sort_descending: bool,
    pub show_info_panel: bool,
    pub window_width: i32,
    pub window_height: i32,
    /// Last custom export settings and user presets.
    pub export: ExportSettings,
    pub export_presets: Vec<ExportSettings>,
    /// Ask before a conversion replaces or deletes the original files.
    pub confirm_convert: bool,
    /// Last options of "Split Packs into Albums".
    pub split: crate::packs::SplitOptions,
    pub scraper: ScraperSettings,
    pub organizer: OrganizerSettings,
    /// User list of custom value names (Custom tab).
    pub custom_fields: Vec<String>,
}

/// A list whose entries this version cannot read (a column added by a newer
/// version) loses them instead of making the whole file unreadable.
fn skip_unreadable<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let values = Vec::<serde_json::Value>::deserialize(d)?;
    Ok(values.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

fn none_if_unreadable<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(serde_json::Value::deserialize(d)?).ok())
}

/// Columns visible by default (close to ComicRack CE's default layout).
const DEFAULT_VISIBLE: &[Field] = &[
    Field::Modified,
    Field::Series,
    Field::Number,
    Field::Volume,
    Field::Title,
    Field::Released,
    Field::Writer,
    Field::Penciller,
    Field::Publisher,
    Field::PageCount,
    Field::Rating,
    Field::Added,
    Field::FileFormat,
    Field::FileName,
];

pub fn default_columns() -> Vec<ColumnSetting> {
    let mut cols: Vec<ColumnSetting> = DEFAULT_VISIBLE
        .iter()
        .map(|f| ColumnSetting { field: *f, visible: true, width: default_width(*f) })
        .collect();
    for f in Field::ALL {
        if !DEFAULT_VISIBLE.contains(f) {
            cols.push(ColumnSetting { field: *f, visible: false, width: default_width(*f) });
        }
    }
    cols
}

fn default_width(f: Field) -> i32 {
    match f {
        Field::Modified => 30,
        Field::Number | Field::AlternateNumber => 100,
        Field::Volume | Field::Count | Field::Year | Field::Month | Field::Day
        | Field::PageCount | Field::AlternateCount => 70,
        Field::Series | Field::Title | Field::FileName | Field::Caption | Field::AlternateSeries => 220,
        Field::FilePath | Field::FileDirectory | Field::Summary | Field::Notes => 300,
        _ => 130,
    }
}

impl Default for ColumnSetting {
    fn default() -> Self {
        ColumnSetting { field: Field::Series, visible: true, width: 120 }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            library_folders: Vec::new(),
            scan_on_startup: false,
            language: crate::i18n::Language::Auto,
            theme: crate::ui::theme::Theme::System,
            remove_missing: true,
            scan_threads: 16,
            allow_write_info: true,
            auto_update_files: false,
            rar_path: String::new(),
            ddjvu_path: String::new(),
            djvused_path: String::new(),
            pdftoppm_path: String::new(),
            columns: default_columns(),
            sort_field: None,
            sort_descending: false,
            show_info_panel: true,
            window_width: 1280,
            window_height: 800,
            export: ExportSettings::default(),
            export_presets: Vec::new(),
            confirm_convert: true,
            split: crate::packs::SplitOptions::default(),
            scraper: ScraperSettings::default(),
            organizer: OrganizerSettings::default(),
            custom_fields: Vec::new(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        project_dirs().config_dir().join("settings.json")
    }

    /// The settings, with a message for the user when the file could not
    /// be read: it is then kept aside, as it would be replaced at the next
    /// save.
    pub fn load() -> (Settings, Option<String>) {
        let path = Self::path();
        let (mut s, problem) = match std::fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(e) => (Settings::default(), Some(t!("The settings could not be read ({0}): the default settings are used.", e))),
            Ok(data) => match serde_json::from_slice(&data) {
                Ok(s) => (s, None),
                Err(e) => {
                    let kept = path.with_extension("json.unreadable");
                    let _ = std::fs::copy(&path, &kept);
                    let m = t!("The settings could not be read ({0}). The file was kept as {1}; the default settings are used.", e, kept.display());
                    (Settings::default(), Some(m))
                }
            },
        };
        // Columns added in newer versions are appended (hidden).
        for f in Field::ALL {
            if !s.columns.iter().any(|c| c.field == *f) {
                s.columns.push(ColumnSetting { field: *f, visible: false, width: default_width(*f) });
            }
        }
        s.apply_tools();
        (s, problem)
    }

    /// Only the language, read before the interface is built.
    pub fn load_language() -> crate::i18n::Language {
        #[derive(Deserialize, Default)]
        struct L {
            #[serde(default)]
            language: crate::i18n::Language,
        }
        std::fs::read(Self::path())
            .ok()
            .and_then(|d| serde_json::from_slice::<L>(&d).ok())
            .unwrap_or_default()
            .language
    }

    pub fn save(&self) -> Result<()> {
        write_atomic(&Self::path(), &serde_json::to_vec_pretty(self)?)
    }

    pub fn apply_tools(&self) {
        crate::archive::set_tools(crate::archive::Tools {
            rar: self.rar_path.clone(),
            ddjvu: self.ddjvu_path.clone(),
            djvused: self.djvused_path.clone(),
            pdftoppm: self.pdftoppm_path.clone(),
        });
    }

    pub fn undo_path() -> PathBuf {
        crate::library::data_dir().join("organizer-undo.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_unknown_to_this_version_are_skipped() {
        let json = r#"{"library_folders": ["/b"], "columns": [{"field": "Series", "visible": true, "width": 9}, {"field": "FromTheFuture"}], "sort_field": "FromTheFuture"}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.library_folders, [PathBuf::from("/b")]);
        assert_eq!(s.columns.len(), 1);
        assert_eq!(s.sort_field, None);
    }
}
