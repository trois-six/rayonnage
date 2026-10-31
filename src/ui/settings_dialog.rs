//! Preferences: library folders, interface language, file writing, external tools.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use super::widgets::{self, Form};
use super::theme::Theme;
use crate::i18n::Language;
use super::{State, refresh_sidebar, scan};

pub fn open(state: &State) {
    let s = state.settings.borrow().clone();
    let (win, content, buttons) = widgets::dialog(&state.window, t!("Settings"), 720, 620);
    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);
    content.append(&notebook);

    // --- Library folders
    let folders: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(s.library_folders.clone()));
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    let rebuild: Rc<dyn Fn()> = {
        let folders = folders.clone();
        let list = list.clone();
        Rc::new(move || rebuild_folders(&list, &folders))
    };
    rebuild();
    let add_btn = gtk::Button::with_label(t!("Add Folder…"));
    add_btn.connect_clicked(glib::clone!(
        #[weak]
        win,
        #[strong]
        folders,
        #[strong]
        rebuild,
        move |_| {
            let folders = folders.clone();
            let rebuild = rebuild.clone();
            widgets::choose_folder(&win, t!("Book folder to scan"), move |p| {
                if !folders.borrow().contains(&p) {
                    folders.borrow_mut().push(p);
                }
                rebuild();
            });
        }
    ));
    let scan_startup = widgets::check(t!("Scan the folders at startup"), s.scan_on_startup);
    let remove_missing = widgets::check(t!("Remove books whose file has disappeared from the library"), s.remove_missing);
    let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    page.set_margin_start(12);
    page.set_margin_end(12);
    page.set_margin_top(12);
    page.append(&gtk::Label::builder().label(t!("Folders scanned for books (cbz, zip, cbr, rar, cb7, 7z, cbt, tar, pdf, djvu, epub):")).xalign(0.0).wrap(true).build());
    page.append(&widgets::scrolled(&list));
    page.append(&add_btn);
    page.append(&scan_startup);
    page.append(&remove_missing);
    let threads_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let threads = widgets::spin(1.0, 64.0, 1.0, s.scan_threads as f64);
    threads_row.append(&gtk::Label::new(Some(t!("Parallel reads during the scan:"))));
    threads_row.append(&threads);
    threads_row.set_tooltip_text(Some(
        t!("Number of folders and books read at the same time. A high value (16 to 32) greatly speeds up network drives (sshfs, SMB, NFS); 4 is enough for a local disk."),
    ));
    page.append(&threads_row);
    let language_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let languages = Language::ALL.map(Language::label);
    let language_index = Language::ALL.iter().position(|l| *l == s.language).unwrap_or(0);
    let language = widgets::dropdown(&languages, language_index);
    language_row.append(&gtk::Label::new(Some(t!("Interface language:"))));
    language_row.append(&language);
    language_row.append(&gtk::Label::builder().label(t!("(applies at the next start)")).css_classes(["dim-label"]).build());
    page.append(&language_row);
    let theme_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let themes = Theme::ALL.map(Theme::label);
    let theme = widgets::dropdown(&themes, Theme::ALL.iter().position(|t| *t == s.theme).unwrap_or(0));
    theme_row.append(&gtk::Label::new(Some(t!("Appearance:"))));
    theme_row.append(&theme);
    page.append(&theme_row);
    notebook.append_page(&page, Some(&gtk::Label::new(Some(t!("Library")))));

    // --- Files
    let mut form = Form::new();
    form.grid.set_margin_start(12);
    form.grid.set_margin_end(12);
    form.grid.set_margin_top(12);
    let allow_write = widgets::check(t!("Allow writing the information (ComicInfo.xml) into the files"), s.allow_write_info);
    let auto_update = widgets::check(t!("Automatically update the files after each change"), s.auto_update_files);
    form.add_full(&allow_write);
    form.add_full(&auto_update);
    form.add_full(
        &gtk::Label::builder()
            .label(t!("Writable formats: CBZ/ZIP, CB7/7Z, CBT/TAR, and CBR/RAR if the “rar” program is available. PDF and DjVu files keep their information in the library only."))
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    form.add_title(t!("External programs (optional)"));
    let (rar_box, rar) = widgets::path_entry(&win, &s.rar_path, true);
    rar.set_placeholder_text(Some(t!("rar / Rar.exe (WinRAR) — needed to create CBR files")));
    form.add(t!("RAR:"), &rar_box);
    let (pdf_box, pdftoppm) = widgets::path_entry(&win, &s.pdftoppm_path, true);
    pdftoppm.set_placeholder_text(Some(t!("pdftoppm (poppler) — PDF pages without an image")));
    form.add(t!("pdftoppm:"), &pdf_box);
    let (ddjvu_box, ddjvu) = widgets::path_entry(&win, &s.ddjvu_path, true);
    ddjvu.set_placeholder_text(Some(t!("ddjvu (DjVuLibre) — reading DjVu files")));
    form.add(t!("ddjvu:"), &ddjvu_box);
    let (djvused_box, djvused) = widgets::path_entry(&win, &s.djvused_path, true);
    djvused.set_placeholder_text(Some("djvused (DjVuLibre)"));
    form.add(t!("djvused:"), &djvused_box);
    let found = |cfg: &str, names: &[&str]| {
        crate::util::find_program(cfg, names)
            .map(|p| t!("found: {0}", p.display()))
            .unwrap_or_else(|| t!("not found").into())
    };
    form.add_full(
        &gtk::Label::builder()
            .label(t!(
                "Currently — rar: {0}; pdftoppm: {1}; ddjvu: {2}",
                found(&s.rar_path, &["rar", "Rar"]),
                found(&s.pdftoppm_path, &["pdftoppm"]),
                found(&s.ddjvu_path, &["ddjvu"])
            ))
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build(),
    );
    notebook.append_page(&form.grid, Some(&gtk::Label::new(Some(t!("Files")))));

    // --- Custom fields
    let custom = gtk::TextView::new();
    custom.buffer().set_text(&s.custom_fields.join("\n"));
    let cpage = gtk::Box::new(gtk::Orientation::Vertical, 8);
    cpage.set_margin_start(12);
    cpage.set_margin_end(12);
    cpage.set_margin_top(12);
    cpage.append(&gtk::Label::builder().label(t!("Names of the custom values offered in the “Custom” tab (one per line):")).xalign(0.0).wrap(true).build());
    cpage.append(&widgets::scrolled(&custom));
    notebook.append_page(&cpage, Some(&gtk::Label::new(Some(t!("Custom")))));

    // --- Fetching info from bedetheque.com
    let (bd_page, read_bedetheque) = super::scraper_ui::settings_page(s.scraper.clone());
    bd_page.set_margin_start(6);
    bd_page.set_margin_end(6);
    bd_page.set_margin_top(6);
    notebook.append_page(&bd_page, Some(&gtk::Label::new(Some("Bedetheque"))));

    let cancel = widgets::button(t!("Cancel"), false);
    let ok = widgets::button(t!("Save"), true);
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
            let old_folders = st.settings.borrow().library_folders.clone();
            {
                let mut s = st.settings.borrow_mut();
                s.library_folders = folders.borrow().clone();
                s.scan_on_startup = scan_startup.is_active();
                s.remove_missing = remove_missing.is_active();
                s.scan_threads = threads.value() as usize;
                s.scraper = read_bedetheque();
                s.allow_write_info = allow_write.is_active();
                s.auto_update_files = auto_update.is_active();
                s.rar_path = rar.text().trim().to_string();
                s.pdftoppm_path = pdftoppm.text().trim().to_string();
                s.ddjvu_path = ddjvu.text().trim().to_string();
                s.djvused_path = djvused.text().trim().to_string();
                s.language = Language::ALL.get(language.selected() as usize).copied().unwrap_or_default();
                s.theme = Theme::ALL.get(theme.selected() as usize).copied().unwrap_or_default();
                super::theme::apply(s.theme);
                let b = custom.buffer();
                s.custom_fields = b
                    .text(&b.start_iter(), &b.end_iter(), false)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                s.apply_tools();
                if let Err(e) = s.save() {
                    widgets::alert(&st.window, t!("Error"), &format!("{e:#}"));
                }
            }
            refresh_sidebar(&st);
            win.close();
            let new_folders = st.settings.borrow().library_folders.clone();
            if new_folders != old_folders && !new_folders.is_empty() {
                let st2 = st.clone();
                widgets::confirm(
                    &st.window,
                    t!("Folders changed"),
                    t!("Scan the library folders now?"),
                    t!("Scan"),
                    move || scan(&st2),
                );
            }
        }
    ));
    buttons.append(&cancel);
    buttons.append(&ok);
    win.present();
}

fn rebuild_folders(list: &gtk::ListBox, folders: &Rc<RefCell<Vec<PathBuf>>>) {
    while let Some(r) = list.row_at_index(0) {
        list.remove(&r);
    }
    for (i, f) in folders.borrow().iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_margin_start(6);
        row.set_margin_end(6);
        let l = gtk::Label::builder()
            .label(f.display().to_string())
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        let del = gtk::Button::from_icon_name("list-remove-symbolic");
        del.set_tooltip_text(Some(t!("Remove this folder")));
        let folders = folders.clone();
        let list2 = list.clone();
        del.connect_clicked(move |_| {
            folders.borrow_mut().remove(i);
            rebuild_folders(&list2, &folders);
        });
        row.append(&l);
        if !f.is_dir() {
            let warn = gtk::Image::from_icon_name("dialog-warning-symbolic");
            warn.set_tooltip_text(Some(t!("Folder not found")));
            row.append(&warn);
        }
        row.append(&del);
        list.append(&row);
    }
}
