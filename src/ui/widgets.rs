//! Small reusable dialogs and form helpers.

use gtk::prelude::*;
use gtk::{gio, glib};

pub fn alert(parent: &impl IsA<gtk::Window>, title: &str, detail: &str) {
    gtk::AlertDialog::builder()
        .message(title)
        .detail(detail)
        .modal(true)
        .build()
        .show(Some(parent));
}

/// Asks for confirmation; `on_ok` runs only when the user accepts.
pub fn confirm(
    parent: &impl IsA<gtk::Window>,
    title: &str,
    detail: &str,
    ok_label: &str,
    on_ok: impl FnOnce() + 'static,
) {
    let d = gtk::AlertDialog::builder()
        .message(title)
        .detail(detail)
        .modal(true)
        .buttons([t!("Cancel"), ok_label])
        .cancel_button(0)
        .default_button(1)
        .build();
    d.choose(Some(parent), gio::Cancellable::NONE, move |r| {
        if r == Ok(1) {
            on_ok();
        }
    });
}

/// A modal window with a content area and a button row; returns (window, content, buttons).
pub fn dialog(parent: &impl IsA<gtk::Window>, title: &str, width: i32, height: i32) -> (gtk::Window, gtk::Box, gtk::Box) {
    let win = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .default_width(width)
        .default_height(height)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_vexpand(true);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    root.append(&content);
    root.append(&buttons);
    win.set_child(Some(&root));
    let esc = gtk::ShortcutController::new();
    esc.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(|w, _| {
            if let Some(w) = w.downcast_ref::<gtk::Window>() {
                w.close();
            }
            glib::Propagation::Stop
        })),
    ));
    win.add_controller(esc);
    (win, content, buttons)
}

pub fn button(label: &str, suggested: bool) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    if suggested {
        b.add_css_class("suggested-action");
    }
    b
}

/// Shows a read-only text (logs, reports) with a "Save…" button.
pub fn show_log(parent: &impl IsA<gtk::Window>, title: &str, text: &str) {
    let (win, content, buttons) = dialog(parent, title, 760, 480);
    let view = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    view.buffer().set_text(text);
    content.append(&gtk::ScrolledWindow::builder().child(&view).vexpand(true).build());
    let save = button(t!("Save…"), false);
    let close = button(t!("Close"), true);
    let text = text.to_string();
    save.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            let text = text.clone();
            let fd = gtk::FileDialog::builder().title(t!("Save")).initial_name(t!("report.txt")).build();
            fd.save(Some(&win), gio::Cancellable::NONE, move |r| {
                if let Ok(f) = r
                    && let Some(p) = f.path()
                {
                    let _ = std::fs::write(p, &text);
                }
            });
        }
    ));
    close.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    buttons.append(&save);
    buttons.append(&close);
    win.present();
}

/// Lets the user pick a folder.
pub fn choose_folder(parent: &impl IsA<gtk::Window>, title: &str, on_ok: impl FnOnce(std::path::PathBuf) + 'static) {
    let fd = gtk::FileDialog::builder().title(title).modal(true).build();
    fd.select_folder(Some(parent), gio::Cancellable::NONE, move |r| {
        if let Ok(f) = r
            && let Some(p) = f.path()
        {
            on_ok(p);
        }
    });
}

/// Lets the user pick a file.
pub fn choose_file(parent: &impl IsA<gtk::Window>, title: &str, on_ok: impl FnOnce(std::path::PathBuf) + 'static) {
    let fd = gtk::FileDialog::builder().title(title).modal(true).build();
    fd.open(Some(parent), gio::Cancellable::NONE, move |r| {
        if let Ok(f) = r
            && let Some(p) = f.path()
        {
            on_ok(p);
        }
    });
}

/// A grid of "label: widget" rows.
pub struct Form {
    pub grid: gtk::Grid,
    row: i32,
}

impl Form {
    pub fn new() -> Self {
        let grid = gtk::Grid::builder().row_spacing(6).column_spacing(12).build();
        Form { grid, row: 0 }
    }

    pub fn add(&mut self, label: &str, widget: &impl IsA<gtk::Widget>) {
        let l = gtk::Label::builder().label(label).xalign(1.0).build();
        widget.set_hexpand(true);
        self.grid.attach(&l, 0, self.row, 1, 1);
        self.grid.attach(widget, 1, self.row, 1, 1);
        self.row += 1;
    }

    pub fn add_full(&mut self, widget: &impl IsA<gtk::Widget>) {
        self.grid.attach(widget, 0, self.row, 2, 1);
        self.row += 1;
    }

    pub fn add_title(&mut self, title: &str) {
        let l = gtk::Label::builder().xalign(0.0).margin_top(8).build();
        l.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(title)));
        self.add_full(&l);
    }
}

pub fn entry(text: &str) -> gtk::Entry {
    gtk::Entry::builder().text(text).build()
}

pub fn check(label: &str, active: bool) -> gtk::CheckButton {
    gtk::CheckButton::builder().label(label).active(active).build()
}

pub fn spin(min: f64, max: f64, step: f64, value: f64) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(min, max, step);
    s.set_value(value);
    s
}

pub fn dropdown(items: &[&str], selected: usize) -> gtk::DropDown {
    let d = gtk::DropDown::from_strings(items);
    d.set_selected(selected as u32);
    d
}

/// An entry with a "…" button choosing a folder (or a file when `file` is true).
pub fn path_entry(parent: &gtk::Window, text: &str, file: bool) -> (gtk::Box, gtk::Entry) {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let e = entry(text);
    e.set_hexpand(true);
    let btn = gtk::Button::from_icon_name("document-open-symbolic");
    btn.connect_clicked(glib::clone!(
        #[weak]
        e,
        #[weak]
        parent,
        move |_| {
            let set = move |p: std::path::PathBuf| e.set_text(&p.to_string_lossy());
            if file {
                choose_file(&parent, t!("Choose a program"), set);
            } else {
                choose_folder(&parent, t!("Choose a folder"), set);
            }
        }
    ));
    b.append(&e);
    b.append(&btn);
    (b, e)
}

pub fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
}
