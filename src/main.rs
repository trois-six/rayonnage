//! Rayonnage: a ComicRack CE-like library manager for comics, BD and mangas.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod archive;
mod autonumber;

mod bedetheque;
mod comicinfo;
mod comicrack_import;
mod convert;
mod epub;
mod library;
mod model;
mod organizer;
mod packs;
mod settings;
mod ui;
mod util;

use gtk::prelude::*;

fn main() -> gtk::glib::ExitCode {
    // GTK renderer on Linux, unless the user chose one with GSK_RENDERER.
    // ("ngl", the OpenGL renderer, closes faster than Vulkan with some
    // NVIDIA drivers.)
    #[cfg(target_os = "linux")]
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: no other thread runs yet.
        unsafe { std::env::set_var("GSK_RENDERER", "vulkan") };
    }
    library::migrate_old_dirs();
    i18n::init(settings::Settings::load_language());
    let app = gtk::Application::builder().application_id(ui::APP_ID).build();
    // Screenshot runs (development) must not hand over to a running instance.
    #[cfg(debug_assertions)]
    if std::env::var_os("BO_SHOT").is_some() {
        app.set_flags(gtk::gio::ApplicationFlags::NON_UNIQUE);
    }
    app.connect_activate(ui::build_ui);
    app.run()
}
