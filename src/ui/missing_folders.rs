//! Library folders not found (moved, drive or share not mounted): asks where
//! they are now, then moves the folders and the paths of their books.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use super::widgets;
use super::{State, fill_store, refresh_sidebar, update_status};
use crate::library::moved_like;

/// Runs `then` at once when every library folder exists; otherwise after
/// asking where the missing ones are.
pub fn check(state: &State, then: impl FnOnce(&State) + 'static) {
    let missing: Vec<PathBuf> = state
        .settings
        .borrow()
        .library_folders
        .iter()
        .filter(|f| !f.is_dir())
        .cloned()
        .collect();
    if missing.is_empty() {
        then(state);
        return;
    }
    for f in &missing {
        state.journal.info(&t!("{0}: folder not found", f.display()));
    }

    let (win, content, buttons) = widgets::dialog(&state.window, t!("Library folders not found"), 680, 360);
    content.append(
        &gtk::Label::builder()
            .label(t!("These library folders cannot be found: they may have moved, or their drive or network share is not mounted. Locate each one to update the folder and the paths of its books. Folders left as they are keep their books and are skipped by the scan."))
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    let targets: Rc<RefCell<Vec<Option<PathBuf>>>> = Rc::new(RefCell::new(vec![None; missing.len()]));
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let apply = widgets::button(t!("Update"), true);
    apply.set_sensitive(false);
    let mut target_labels = Vec::new();
    let mut locate_buttons = Vec::new();
    for f in &missing {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_margin_start(6);
        row.set_margin_end(6);
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.append(
            &gtk::Label::builder()
                .label(f.display().to_string())
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .build(),
        );
        let target = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["dim-label"])
            .build();
        text.append(&target);
        let locate = gtk::Button::with_label(t!("Locate…"));
        locate.set_valign(gtk::Align::Center);
        row.append(&text);
        row.append(&locate);
        list.append(&row);
        target_labels.push(target);
        locate_buttons.push(locate);
    }
    let show: Rc<dyn Fn()> = {
        let (targets, apply) = (targets.clone(), apply.clone());
        Rc::new(move || {
            let targets = targets.borrow();
            for (label, target) in target_labels.iter().zip(targets.iter()) {
                match target {
                    Some(p) => label.set_label(&t!("→ {0}", p.display())),
                    None => label.set_label(t!("not found")),
                }
            }
            apply.set_sensitive(targets.iter().any(Option::is_some));
        })
    };
    show();
    let missing = Rc::new(missing);
    for (i, locate) in locate_buttons.iter().enumerate() {
        let (missing, targets, show) = (missing.clone(), targets.clone(), show.clone());
        locate.connect_clicked(glib::clone!(
            #[weak]
            win,
            move |_| {
                let (missing, targets, show) = (missing.clone(), targets.clone(), show.clone());
                widgets::choose_folder(&win, &t!("New location of {0}", missing[i].display()), move |p| {
                    let mut t = targets.borrow_mut();
                    // The other folders probably moved the same way.
                    for (j, other) in missing.iter().enumerate() {
                        if t[j].is_none()
                            && let Some(guess) = moved_like(&missing[i], &p, other)
                            && guess.is_dir()
                        {
                            t[j] = Some(guess);
                        }
                    }
                    t[i] = Some(p);
                    drop(t);
                    show();
                });
            }
        ));
    }
    content.append(&widgets::scrolled(&list));

    // Closing the window, whatever the button, goes on with `then`.
    let then = RefCell::new(Some(then));
    let st = state.clone();
    win.connect_close_request(move |_| {
        if let Some(then) = then.take() {
            then(&st);
        }
        glib::Propagation::Proceed
    });
    let skip = widgets::button(t!("Not Now"), false);
    skip.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    let st = state.clone();
    apply.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            let moves: Vec<(PathBuf, PathBuf)> = missing
                .iter()
                .zip(targets.borrow().iter())
                .filter_map(|(from, to)| Some((from.clone(), to.clone()?)))
                .collect();
            relocate(&st, &moves);
            win.close();
        }
    ));
    buttons.append(&skip);
    buttons.append(&apply);
    win.present();
}

/// Replaces the folders in the settings, and moves their books in the library.
fn relocate(state: &State, moves: &[(PathBuf, PathBuf)]) {
    let mut books = 0;
    for (from, to) in moves {
        let n = state.library.borrow_mut().relocate(from, to);
        books += n;
        state.journal.info(&t!("Folder moved: {0} → {1} ({2} book(s))", from.display(), to.display(), n));
        let mut s = state.settings.borrow_mut();
        if s.library_folders.contains(to) {
            s.library_folders.retain(|f| f != from);
        } else if let Some(f) = s.library_folders.iter_mut().find(|f| *f == from) {
            *f = to.clone();
        }
    }
    if let Err(e) = state.settings.borrow().save() {
        widgets::alert(&state.window, t!("Error"), &format!("{e:#}"));
    }
    state.save_library();
    fill_store(state);
    refresh_sidebar(state);
    update_status(state);
    state.set_status(&t!("{0} folder(s) and {1} book(s) moved to their new location", moves.len(), books));
}
