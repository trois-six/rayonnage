//! Message journal: what the application is doing, live, with the errors.
//! Shown in its own window (Ctrl+J or the button at the bottom right) and
//! mirrored to a log file in the cache folder.

use std::cell::{Cell, RefCell};
use std::io::Write;

use gtk::glib;
use gtk::prelude::*;

use super::widgets;

const MAX_LINES: i32 = 20_000;

pub struct Journal {
    buffer: gtk::TextBuffer,
    pub spinner: gtk::Spinner,
    pub button: gtk::Button,
    label: gtk::Label,
    unseen_errors: Cell<u32>,
    window: RefCell<Option<glib::WeakRef<gtk::Window>>>,
    file: RefCell<Option<std::fs::File>>,
}

pub fn log_file_path() -> std::path::PathBuf {
    crate::library::cache_dir().join("journal.log")
}

impl Journal {
    pub fn new() -> Self {
        let buffer = gtk::TextBuffer::new(None);
        buffer.create_tag(Some("time"), &[("foreground", &"#888888")]);
        buffer.create_tag(Some("error"), &[("foreground", &"#e5534b"), ("weight", &700)]);
        buffer.create_tag(Some("title"), &[("weight", &700)]);
        let spinner = gtk::Spinner::new();
        spinner.set_visible(false);
        let label = gtk::Label::new(Some(t!("Log")));
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.append(&spinner);
        content.append(&label);
        let button = gtk::Button::builder()
            .child(&content)
            .tooltip_text(t!("Message Log (Ctrl+J)"))
            .action_name("win.journal")
            .build();
        button.add_css_class("flat");
        let file = std::fs::File::create(log_file_path()).ok();
        Journal {
            buffer,
            spinner,
            button,
            label,
            unseen_errors: Cell::new(0),
            window: RefCell::new(None),
            file: RefCell::new(file),
        }
    }

    pub fn info(&self, msg: &str) {
        self.append(msg, None);
    }

    /// A step that starts or ends an operation (shown in bold).
    pub fn title(&self, msg: &str) {
        self.append(msg, Some("title"));
    }

    pub fn error(&self, msg: &str) {
        self.append(msg, Some("error"));
        if !self.is_window_visible() {
            self.unseen_errors.set(self.unseen_errors.get() + 1);
            self.update_label();
        }
    }

    fn append(&self, msg: &str, tag: Option<&str>) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        let mut end = self.buffer.end_iter();
        self.buffer.insert_with_tags_by_name(&mut end, &format!("{time}  "), &["time"]);
        let mut end = self.buffer.end_iter();
        match tag {
            Some(t) => self.buffer.insert_with_tags_by_name(&mut end, &format!("{msg}\n"), &[t]),
            None => self.buffer.insert(&mut end, &format!("{msg}\n")),
        }
        if self.buffer.line_count() > MAX_LINES {
            let mut start = self.buffer.start_iter();
            let mut cut = self.buffer.iter_at_line(MAX_LINES / 10).unwrap_or(self.buffer.end_iter());
            self.buffer.delete(&mut start, &mut cut);
        }
        if let Some(f) = self.file.borrow_mut().as_mut() {
            if tag == Some("error") {
                let _ = writeln!(f, "{time}  {} {msg}", t!("ERROR"));
            } else {
                let _ = writeln!(f, "{time}  {msg}");
            }
        }
    }

    /// Shows the activity spinner while an operation runs.
    pub fn set_active(&self, active: bool) {
        self.spinner.set_visible(active);
        if active {
            self.spinner.start();
        } else {
            self.spinner.stop();
        }
    }

    fn update_label(&self) {
        let n = self.unseen_errors.get();
        if n == 0 {
            self.label.set_text(t!("Log"));
            self.label.remove_css_class("error");
        } else {
            self.label.set_text(&t!("Log — {0} error(s)", n));
            self.label.add_css_class("error");
        }
    }

    fn is_window_visible(&self) -> bool {
        self.window
            .borrow()
            .as_ref()
            .and_then(|w| w.upgrade())
            .is_some_and(|w| w.is_visible())
    }

    pub fn open(&self, parent: &impl IsA<gtk::Window>) {
        self.unseen_errors.set(0);
        self.update_label();
        if let Some(w) = self.window.borrow().as_ref().and_then(|w| w.upgrade()) {
            w.present();
            return;
        }
        let win = gtk::Window::builder()
            .title(t!("Message Log"))
            .transient_for(parent)
            .default_width(900)
            .default_height(500)
            .hide_on_close(true)
            .build();
        let view = gtk::TextView::builder()
            .buffer(&self.buffer)
            .editable(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(8)
            .right_margin(8)
            .top_margin(6)
            .build();
        let scroll = gtk::ScrolledWindow::builder().child(&view).vexpand(true).build();
        // Follow the end of the journal, unless the user scrolled up.
        let end_mark = self.buffer.create_mark(None, &self.buffer.end_iter(), false);
        let follow = std::rc::Rc::new(Cell::new(true));
        let adj = scroll.vadjustment();
        adj.connect_value_changed(glib::clone!(
            #[strong]
            follow,
            move |a| follow.set(a.value() + a.page_size() >= a.upper() - 30.0)
        ));
        self.buffer.connect_changed(glib::clone!(
            #[weak]
            view,
            #[strong]
            follow,
            #[strong]
            end_mark,
            move |_| {
                if follow.get() {
                    view.scroll_mark_onscreen(&end_mark);
                }
            }
        ));
        view.scroll_mark_onscreen(&end_mark);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.append(&scroll);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        bar.set_margin_bottom(8);
        let path = gtk::Label::builder()
            .label(t!("Mirrored to {0}", log_file_path().display()))
            .xalign(0.0)
            .hexpand(true)
            .selectable(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["dim-label"])
            .build();
        let clear = widgets::button(t!("Clear"), false);
        let save = widgets::button(t!("Save…"), false);
        let close = widgets::button(t!("Close"), true);
        let buffer = self.buffer.clone();
        clear.connect_clicked(move |_| buffer.set_text(""));
        let buffer = self.buffer.clone();
        save.connect_clicked(glib::clone!(
            #[weak]
            win,
            move |_| {
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false).to_string();
                let fd = gtk::FileDialog::builder().title(t!("Save Log")).initial_name("journal.txt").build();
                fd.save(Some(&win), gtk::gio::Cancellable::NONE, move |r| {
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
        bar.append(&path);
        bar.append(&clear);
        bar.append(&save);
        bar.append(&close);
        root.append(&bar);
        win.set_child(Some(&root));
        self.window.replace(Some(win.downgrade()));
        win.present();
    }
}
