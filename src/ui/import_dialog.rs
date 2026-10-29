//! "Import ComicRack Settings…": Bedetheque, file organization profiles,
//! preferences and export settings of a ComicRack installation.

use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gio, glib};

use super::State;
use super::widgets::{self, Form};
use crate::comicrack_import::{ComicRackConfig, ImportChoices, apply, read};

pub fn open(state: &State) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(t!("ComicRack configuration (zip, App.Config, losettingsx.dat, Config.xml)")));
    for p in ["*.zip", "*.config", "*.Config", "*.dat", "*.xml"] {
        filter.add_pattern(p);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let fd = gtk::FileDialog::builder()
        .title(t!("Import ComicRack Settings"))
        .modal(true)
        .filters(&filters)
        .build();
    let st = state.clone();
    fd.open(Some(&state.window), gio::Cancellable::NONE, move |r| {
        let Some(path) = r.ok().and_then(|f| f.path()) else { return };
        // A single file stands for its folder (the other files are next to it).
        let source = if crate::util::extension_lower(&path) == "zip" {
            path
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or(path)
        };
        match read(&source) {
            Ok(cfg) => show(&st, cfg, &source),
            Err(e) => widgets::alert(&st.window, t!("Import ComicRack Settings"), &format!("{e:#}")),
        }
    });
}

/// Local folder proposed for a Windows root: the common parent of the
/// library folders.
fn guess_local_root(state: &State) -> String {
    let folders = state.settings.borrow().library_folders.clone();
    let mut common: Option<PathBuf> = folders.first().and_then(|f| f.parent().map(Path::to_path_buf));
    for f in &folders {
        while let Some(c) = &common {
            if f.starts_with(c) {
                break;
            }
            common = c.parent().map(Path::to_path_buf);
        }
    }
    common.map(|c| c.display().to_string()).unwrap_or_default()
}

fn show(state: &State, cfg: ComicRackConfig, source: &Path) {
    let (win, content, buttons) = widgets::dialog(&state.window, t!("Import ComicRack Settings"), 620, 420);
    content.append(
        &gtk::Label::builder()
            .label(t!("Settings found in {0}:", source.display()))
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    let mut form = Form::new();
    let scraper = widgets::check(t!("Bedetheque settings (fields and options)"), cfg.scraper.is_some());
    scraper.set_sensitive(cfg.scraper.is_some());
    form.add_full(&scraper);
    let n = cfg.organizer.as_ref().map(|o| o.profiles.len()).unwrap_or(0);
    let names = cfg
        .organizer
        .as_ref()
        .map(|o| o.profiles.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let organizer = widgets::check(&t!("File organization profiles: {0} ({1})", n, names), n > 0);
    organizer.set_sensitive(n > 0);
    form.add_full(&organizer);
    let has_prefs = cfg.scan_on_startup.is_some() || cfg.remove_missing.is_some();
    let prefs = widgets::check(t!("Preferences: scan at startup, removal of missing files, convert confirmation"), has_prefs);
    prefs.set_sensitive(has_prefs);
    form.add_full(&prefs);
    let has_export = cfg.export.is_some() || !cfg.export_presets.is_empty();
    let export = widgets::check(t!("Export settings and presets"), has_export);
    export.set_sensitive(has_export);
    form.add_full(&export);

    // Windows paths of the profiles → local folders.
    let mut mappings = Vec::new();
    if !cfg.windows_roots.is_empty() {
        form.add_title(t!("Windows paths"));
        form.add_full(
            &gtk::Label::builder()
                .label(t!("Local folder corresponding to each Windows drive or share used by the profiles:"))
                .xalign(0.0)
                .wrap(true)
                .build(),
        );
        let guess = guess_local_root(state);
        for root in &cfg.windows_roots {
            let (b, e) = widgets::path_entry(&win, &guess, false);
            form.add(root, &b);
            mappings.push((root.clone(), e));
        }
    }
    content.append(&widgets::scrolled(&form.grid));

    let cancel = widgets::button(t!("Cancel"), false);
    let ok = widgets::button(t!("Import"), true);
    cancel.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    let st = state.clone();
    ok.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            let choices = ImportChoices {
                scraper: scraper.is_active(),
                organizer: organizer.is_active(),
                preferences: prefs.is_active(),
                export: export.is_active(),
            };
            let map: Vec<(String, String)> = mappings.iter().map(|(w, e)| (w.clone(), e.text().trim().to_string())).collect();
            let report = {
                let mut s = st.settings.borrow_mut();
                let report = apply(&cfg, &choices, &map, &mut s);
                if let Err(e) = s.save() {
                    st.journal.error(&format!("{e:#}"));
                }
                report
            };
            for line in &report {
                st.journal.info(line);
            }
            win.close();
            widgets::alert(&st.window, t!("Import ComicRack Settings"), &report.join("\n"));
        }
    ));
    buttons.append(&cancel);
    buttons.append(&ok);
    win.present();
}
