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
    /// Only the errors are shown.
    errors_only: Cell<bool>,
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
        buffer.create_tag(Some("hidden"), &[("invisible", &true)]);
        buffer.create_tag(Some("match"), &[("background", &"#f5d76e"), ("foreground", &"#1e1e1e")]);
        buffer.create_tag(Some("current-match"), &[("background", &"#f0883e"), ("foreground", &"#1e1e1e")]);
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
            errors_only: Cell::new(false),
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
        let line_start = self.buffer.end_iter().offset();
        let mut end = self.buffer.end_iter();
        self.buffer.insert_with_tags_by_name(&mut end, &format!("{time}  "), &["time"]);
        let mut end = self.buffer.end_iter();
        match tag {
            Some(t) => self.buffer.insert_with_tags_by_name(&mut end, &format!("{msg}\n"), &[t]),
            None => self.buffer.insert(&mut end, &format!("{msg}\n")),
        }
        if self.errors_only.get() && tag != Some("error") {
            let start = self.buffer.iter_at_offset(line_start);
            self.buffer.apply_tag_by_name("hidden", &start, &self.buffer.end_iter());
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

    /// Shows only the lines that are errors, or every line.
    pub fn set_errors_only(&self, on: bool) {
        self.errors_only.set(on);
        let b = &self.buffer;
        b.remove_tag_by_name("hidden", &b.start_iter(), &b.end_iter());
        if !on {
            return;
        }
        let error = b.tag_table().lookup("error");
        for line in 0..b.line_count() {
            let Some(start) = b.iter_at_line(line) else { continue };
            let mut end = start;
            if !end.forward_line() {
                end = b.end_iter();
            }
            let mut probe = start;
            let is_error = error.as_ref().is_some_and(|t| {
                start.has_tag(t) || (probe.forward_to_tag_toggle(Some(t)) && probe < end)
            });
            if !is_error {
                b.apply_tag_by_name("hidden", &start, &end);
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
        root.append(&self.search_bar(&win, &view, &follow));
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
        let errors_only = widgets::check(t!("Errors only"), self.errors_only.get());
        errors_only.connect_toggled(|c| {
            if let Some(st) = c.root().and_then(|r| r.downcast::<gtk::Window>().ok()).and_then(|w| w.transient_for()) {
                let _ = st.activate_action("win.journal-errors-only", Some(&c.is_active().to_variant()));
            }
        });
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
        bar.append(&errors_only);
        bar.append(&clear);
        bar.append(&save);
        bar.append(&close);
        root.append(&bar);
        win.set_child(Some(&root));
        self.window.replace(Some(win.downgrade()));
        win.present();
    }

    /// Search field: highlights every match, Enter / Shift+Enter (or the
    /// arrows, F3 / Shift+F3) go to the next / previous one.
    fn search_bar(&self, win: &gtk::Window, view: &gtk::TextView, follow: &std::rc::Rc<Cell<bool>>) -> gtk::Box {
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_start(8);
        bar.set_margin_end(8);
        bar.set_margin_top(8);
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(t!("Search the log (Ctrl+F)"))
            .hexpand(true)
            .build();
        let count = gtk::Label::builder().css_classes(["dim-label"]).width_chars(14).xalign(1.0).build();
        let prev = gtk::Button::from_icon_name("go-up-symbolic");
        prev.set_tooltip_text(Some(t!("Previous match (Shift+Enter)")));
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        next.set_tooltip_text(Some(t!("Next match (Enter)")));
        bar.append(&entry);
        bar.append(&count);
        bar.append(&prev);
        bar.append(&next);

        let search = std::rc::Rc::new(Search {
            buffer: self.buffer.clone(),
            view: view.clone(),
            entry: entry.clone(),
            count,
            follow: follow.clone(),
            matches: RefCell::new(Vec::new()),
            current: Cell::new(None),
            pending: Cell::new(false),
        });
        entry.connect_search_changed(glib::clone!(
            #[strong]
            search,
            move |_| {
                search.refresh();
                search.go(1);
            }
        ));
        entry.connect_activate(glib::clone!(
            #[strong]
            search,
            move |_| search.go(1)
        ));
        entry.connect_next_match(glib::clone!(
            #[strong]
            search,
            move |_| search.go(1)
        ));
        entry.connect_previous_match(glib::clone!(
            #[strong]
            search,
            move |_| search.go(-1)
        ));
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[strong]
            search,
            move |_, key, _, mods| {
                let shift = mods.contains(gtk::gdk::ModifierType::SHIFT_MASK);
                match key {
                    gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter if shift => search.go(-1),
                    gtk::gdk::Key::Up => search.go(-1),
                    gtk::gdk::Key::Down => search.go(1),
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            }
        ));
        entry.add_controller(keys);
        prev.connect_clicked(glib::clone!(
            #[strong]
            search,
            move |_| search.go(-1)
        ));
        next.connect_clicked(glib::clone!(
            #[strong]
            search,
            move |_| search.go(1)
        ));
        // New lines: the matches are recomputed shortly after.
        self.buffer.connect_changed(glib::clone!(
            #[strong]
            search,
            move |_| search.schedule_refresh()
        ));
        // Ctrl+F / F3 / Shift+F3 in the log window.
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.set_scope(gtk::ShortcutScope::Managed);
        for (trigger, dir) in [("<Control>f", 0), ("F3", 1), ("<Shift>F3", -1)] {
            let search = search.clone();
            shortcuts.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(trigger),
                Some(gtk::CallbackAction::new(move |_, _| {
                    if dir == 0 {
                        search.entry.grab_focus();
                    } else {
                        search.go(dir);
                    }
                    glib::Propagation::Stop
                })),
            ));
        }
        win.add_controller(shortcuts);
        entry.set_key_capture_widget(Some(win));
        bar
    }
}

/// State of the log search.
struct Search {
    buffer: gtk::TextBuffer,
    view: gtk::TextView,
    entry: gtk::SearchEntry,
    count: gtk::Label,
    follow: std::rc::Rc<Cell<bool>>,
    /// Character offsets of the matches.
    matches: RefCell<Vec<(i32, i32)>>,
    current: Cell<Option<usize>>,
    pending: Cell<bool>,
}

impl Search {
    /// Finds and highlights every match of the searched text.
    fn refresh(&self) {
        let b = &self.buffer;
        b.remove_tag_by_name("match", &b.start_iter(), &b.end_iter());
        b.remove_tag_by_name("current-match", &b.start_iter(), &b.end_iter());
        let needle = self.entry.text().to_string();
        let mut found = Vec::new();
        if !needle.is_empty() {
            let flags = gtk::TextSearchFlags::CASE_INSENSITIVE | gtk::TextSearchFlags::VISIBLE_ONLY;
            let mut from = b.start_iter();
            while let Some((start, end)) = from.forward_search(&needle, flags, None) {
                b.apply_tag_by_name("match", &start, &end);
                found.push((start.offset(), end.offset()));
                from = end;
                if found.len() >= 10_000 {
                    break;
                }
            }
        }
        let keep = self.current.get().filter(|i| *i < found.len());
        self.matches.replace(found);
        self.current.set(keep);
        self.show_current(false);
    }

    fn schedule_refresh(self: &std::rc::Rc<Self>) {
        if self.entry.text().is_empty() || self.pending.replace(true) {
            return;
        }
        let me = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            me.pending.set(false);
            me.refresh();
        });
    }

    /// Goes to the next (`dir` = 1) or previous (-1) match.
    fn go(&self, dir: i32) {
        let n = self.matches.borrow().len();
        if n == 0 {
            self.show_current(false);
            return;
        }
        let next = match self.current.get() {
            None if dir > 0 => 0,
            None => n - 1,
            Some(i) => ((i as i64 + dir as i64).rem_euclid(n as i64)) as usize,
        };
        self.current.set(Some(next));
        self.show_current(true);
    }

    fn show_current(&self, scroll: bool) {
        let b = &self.buffer;
        b.remove_tag_by_name("current-match", &b.start_iter(), &b.end_iter());
        let matches = self.matches.borrow();
        let n = matches.len();
        if self.entry.text().is_empty() {
            self.count.set_text("");
            return;
        }
        if n == 0 {
            self.count.set_text(t!("No match"));
            return;
        }
        let Some(i) = self.current.get() else {
            self.count.set_text(&t!("{0} match(es)", n));
            return;
        };
        self.count.set_text(&format!("{} / {n}", i + 1));
        let (s, e) = matches[i];
        let (start, end) = (b.iter_at_offset(s), b.iter_at_offset(e));
        b.apply_tag_by_name("current-match", &start, &end);
        if scroll {
            // Stop following the end of the log while browsing the matches.
            self.follow.set(false);
            let mut it = start;
            self.view.scroll_to_iter(&mut it, 0.1, true, 0.0, 0.4);
        }
    }
}
