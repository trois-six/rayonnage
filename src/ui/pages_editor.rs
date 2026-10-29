//! "Pages" tab of the Info dialog, as ComicRack CE's pages view: thumbnails
//! and details of every page, reordering by drag and drop, and a context
//! menu to set the page type, rotation, position and bookmark, move pages to
//! the top / bottom, mark them as deleted, reset or sort the order, select
//! and copy pages.
//!
//! The order is the order of the `<Page>` entries of ComicInfo.xml (their
//! `Image` attribute keeps the index of the image in the file); it is used
//! by conversions, which also drop deleted pages and apply rotations.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use super::widgets;
use crate::model::{Book, PAGE_POSITIONS, PAGE_TYPES, PageInfo, ROTATIONS, page_sequence};
use crate::util::natural_cmp;

const THUMB: u32 = 240;

#[derive(Clone, Default)]
struct Row {
    info: PageInfo,
    /// Name of the image in the file.
    name: String,
    size: u64,
    width: u32,
    height: u32,
    thumb: Option<gdk::Texture>,
}

/// Widgets of a displayed row, updated when the thumbnails arrive.
struct RowWidgets {
    picture: gtk::Image,
    details: gtk::Label,
    name: gtk::Label,
}

/// Action of the context menu / toolbar.
type EditorAction = dyn Fn(&Rc<PagesEditor>);

pub struct PagesEditor {
    pub widget: gtk::Box,
    path: PathBuf,
    rows: RefCell<Vec<Row>>,
    list: gtk::ListBox,
    widgets: RefCell<HashMap<i32, RowWidgets>>,
    preview: gtk::Picture,
    preview_label: gtk::Label,
    changed: Cell<bool>,
    /// Page currently dragged (image index).
    drag_source: Cell<Option<i32>>,
}

pub fn type_label(t: &str) -> &'static str {
    super::edit_dialog::page_type_label(t)
}

fn rotation_label(r: &str) -> &'static str {
    match r {
        "Rotate90" => t!("90°"),
        "Rotate180" => t!("180°"),
        "Rotate270" => t!("270°"),
        _ => "",
    }
}

fn position_label(p: &str) -> &'static str {
    match p {
        "Near" => t!("Near"),
        "Far" => t!("Far"),
        _ => t!("Default"),
    }
}

impl PagesEditor {
    pub fn new(book: &Book) -> Rc<Self> {
        let count = book.info.page_count.max(0) as usize;
        let rows = page_sequence(&book.info, count)
            .into_iter()
            .map(|info| Row { info, ..Default::default() })
            .collect();

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        widget.set_margin_start(12);
        widget.set_margin_end(12);
        widget.set_margin_top(12);
        widget.set_margin_bottom(6);
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Multiple).build();
        list.add_css_class("boxed-list");
        let preview = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .width_request(220)
            .vexpand(true)
            .build();
        let preview_label = gtk::Label::builder().css_classes(["dim-label"]).build();

        let ed = Rc::new(PagesEditor {
            widget,
            path: book.file_path.clone(),
            rows: RefCell::new(rows),
            list,
            widgets: RefCell::new(HashMap::new()),
            preview,
            preview_label,
            changed: Cell::new(false),
            drag_source: Cell::new(None),
        });
        ed.build();
        let first = ed.rows.borrow().first().map(|r| r.info.image);
        ed.rebuild(&first.into_iter().collect());
        ed.load_thumbnails();
        ed
    }

    /// The new page list, when the user changed anything.
    pub fn pages(&self) -> Option<Vec<PageInfo>> {
        self.changed.get().then(|| self.rows.borrow().iter().map(|r| r.info.clone()).collect())
    }

    fn build(self: &Rc<Self>) {
        let hint = gtk::Label::builder()
            .label(t!("Change the page order with drag & drop or use the context menu to change page types:"))
            .xalign(0.0)
            .wrap(true)
            .build();
        self.widget.append(&hint);

        // Toolbar.
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let reset_menu = gio::Menu::new();
        reset_menu.append(Some(t!("Reset original Order")), Some("pages.reset"));
        reset_menu.append(Some(t!("Order by Name")), Some("pages.order-name"));
        reset_menu.append(Some(t!("Order by Name (numeric)")), Some("pages.order-numeric"));
        let reset = gtk::MenuButton::builder().label(t!("Reset")).menu_model(&reset_menu).build();
        bar.append(&reset);
        for (label, action) in [
            (t!("Move to Top"), "pages.top"),
            (t!("Move to Bottom"), "pages.bottom"),
            (t!("Mark as Deleted"), "pages.delete"),
        ] {
            let b = gtk::Button::with_label(label);
            b.set_action_name(Some(action));
            bar.append(&b);
        }
        self.widget.append(&bar);

        // List and preview.
        let scroll = gtk::ScrolledWindow::builder()
            .child(&self.list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .vexpand(true)
            .build();
        let side = gtk::Box::new(gtk::Orientation::Vertical, 4);
        side.append(&self.preview);
        side.append(&self.preview_label);
        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&scroll)
            .end_child(&side)
            .resize_end_child(false)
            .shrink_end_child(false)
            .vexpand(true)
            .build();
        self.widget.append(&paned);

        let weak = Rc::downgrade(self);
        self.list.connect_selected_rows_changed(move |_| {
            if let Some(ed) = weak.upgrade() {
                ed.update_preview();
            }
        });

        self.install_actions();
        self.install_context_menu();
        self.install_drop_target();

        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(ed) = weak.upgrade() else { return glib::Propagation::Proceed };
            match key {
                gdk::Key::Delete | gdk::Key::KP_Delete => {
                    ed.toggle_deleted();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.list.add_controller(keys);
    }

    fn install_actions(self: &Rc<Self>) {
        let group = gio::SimpleActionGroup::new();
        let add = |name: &str, f: Box<EditorAction>| {
            let a = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(self);
            a.connect_activate(move |_, _| {
                if let Some(ed) = weak.upgrade() {
                    f(&ed);
                }
            });
            group.add_action(&a);
        };
        add("top", Box::new(|ed| ed.move_to(true)));
        add("bottom", Box::new(|ed| ed.move_to(false)));
        add("delete", Box::new(|ed| ed.toggle_deleted()));
        add("reset", Box::new(|ed| ed.reorder(|a, b| a.info.image.cmp(&b.info.image))));
        add("order-name", Box::new(|ed| ed.reorder(|a, b| a.name.cmp(&b.name))));
        add("order-numeric", Box::new(|ed| ed.reorder(|a, b| natural_cmp(&a.name, &b.name))));
        add("bookmark", Box::new(|ed| ed.ask_bookmark()));
        add("remove-bookmark", Box::new(|ed| ed.edit_selected(|r| r.info.bookmark.clear())));
        add("select-all", Box::new(|ed| ed.list.select_all()));
        add(
            "invert",
            Box::new(|ed| {
                let mut i = 0;
                while let Some(row) = ed.list.row_at_index(i) {
                    if row.is_selected() {
                        ed.list.unselect_row(&row);
                    } else {
                        ed.list.select_row(Some(&row));
                    }
                    i += 1;
                }
            }),
        );
        add("copy", Box::new(|ed| ed.copy_page()));
        for (name, apply) in [
            ("type", (|r: &mut Row, v: &str| r.info.page_type = v.to_string()) as fn(&mut Row, &str)),
            ("rotation", |r, v| r.info.rotation = if v == "None" { String::new() } else { v.to_string() }),
            ("position", |r, v| r.info.position = if v == "Default" { String::new() } else { v.to_string() }),
        ] {
            let a = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
            let weak = Rc::downgrade(self);
            a.connect_activate(move |_, p| {
                if let (Some(ed), Some(v)) = (weak.upgrade(), p.and_then(|p| p.str().map(str::to_string))) {
                    ed.edit_selected(|r| apply(r, &v));
                }
            });
            group.add_action(&a);
        }
        self.widget.insert_action_group("pages", Some(&group));
    }

    fn install_context_menu(self: &Rc<Self>) {
        let menu = gio::Menu::new();
        let types = gio::Menu::new();
        for t in PAGE_TYPES {
            types.append(Some(type_label(t)), Some(&format!("pages.type::{t}")));
        }
        menu.append_submenu(Some(t!("Page Type")), &types);
        let rotations = gio::Menu::new();
        for r in ROTATIONS {
            let label = if *r == "None" { t!("No rotation") } else { rotation_label(r) };
            rotations.append(Some(label), Some(&format!("pages.rotation::{r}")));
        }
        menu.append_submenu(Some(t!("Page Rotation")), &rotations);
        let positions = gio::Menu::new();
        for p in PAGE_POSITIONS {
            positions.append(Some(position_label(p)), Some(&format!("pages.position::{p}")));
        }
        menu.append_submenu(Some(t!("Page Position")), &positions);
        let bookmarks = gio::Menu::new();
        bookmarks.append(Some(t!("Set Bookmark…")), Some("pages.bookmark"));
        bookmarks.append(Some(t!("Remove Bookmark")), Some("pages.remove-bookmark"));
        menu.append_section(None, &bookmarks);
        let moves = gio::Menu::new();
        moves.append(Some(t!("Move to Top")), Some("pages.top"));
        moves.append(Some(t!("Move to Bottom")), Some("pages.bottom"));
        moves.append(Some(t!("Reset original Order")), Some("pages.reset"));
        menu.append_section(None, &moves);
        let select = gio::Menu::new();
        select.append(Some(t!("Select All")), Some("pages.select-all"));
        select.append(Some(t!("Invert Selection")), Some("pages.invert"));
        menu.append_section(None, &select);
        let other = gio::Menu::new();
        other.append(Some(t!("Copy Page")), Some("pages.copy"));
        other.append(Some(t!("Mark as Deleted")), Some("pages.delete"));
        menu.append_section(None, &other);

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(&self.list);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        let click = gtk::GestureClick::builder().button(gdk::BUTTON_SECONDARY).build();
        let list = self.list.clone();
        click.connect_pressed(move |_, _, x, y| {
            // Right click on an unselected page selects it alone.
            if let Some(row) = list.row_at_y(y as i32)
                && !row.is_selected()
            {
                list.unselect_all();
                list.select_row(Some(&row));
            }
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        self.list.add_controller(click);
    }

    /// Dropping pages on a row moves them before it (or after it, in its
    /// lower half).
    fn install_drop_target(self: &Rc<Self>) {
        let target = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        let weak = Rc::downgrade(self);
        target.connect_drop(move |_, _, _, y| {
            let Some(ed) = weak.upgrade() else { return false };
            let Some(source) = ed.drag_source.take() else { return false };
            let len = ed.rows.borrow().len() as i32;
            let mut position = match ed.list.row_at_y(y as i32) {
                Some(row) => {
                    let top = row
                        .compute_point(&ed.list, &gtk::graphene::Point::new(0.0, 0.0))
                        .map(|p| p.y() as f64)
                        .unwrap_or(0.0);
                    let lower = y - top > row.height() as f64 / 2.0;
                    row.index() + lower as i32
                }
                None => len,
            };
            position = position.clamp(0, len);
            let selected = ed.selected_images();
            let moved = if selected.contains(&source) { selected } else { HashSet::from([source]) };
            ed.move_pages(position as usize, &moved);
            true
        });
        self.list.add_controller(target);
    }

    fn selected_images(&self) -> HashSet<i32> {
        self.list
            .selected_rows()
            .iter()
            .filter_map(|r| self.rows.borrow().get(r.index() as usize).map(|row| row.info.image))
            .collect()
    }

    /// Rebuilds the list, keeping the given pages selected.
    fn rebuild(self: &Rc<Self>, selected: &HashSet<i32>) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let mut widgets = HashMap::new();
        for (pos, r) in self.rows.borrow().iter().enumerate() {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            line.set_margin_start(6);
            line.set_margin_end(6);
            line.set_margin_top(3);
            line.set_margin_bottom(3);
            // An image at a fixed size (a picture would take the thumbnail's size).
            let picture = gtk::Image::builder().pixel_size(72).build();
            if let Some(t) = &r.thumb {
                picture.set_paintable(Some(t));
            }
            let number = gtk::Label::builder().label((pos + 1).to_string()).width_chars(4).xalign(1.0).build();
            let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
            texts.set_valign(gtk::Align::Center);
            texts.set_hexpand(true);
            let mut title = type_label(if r.info.page_type.is_empty() { "Story" } else { &r.info.page_type }).to_string();
            if !r.info.bookmark.is_empty() {
                title.push_str(&format!(" — 🔖 {}", r.info.bookmark));
            }
            let title = gtk::Label::builder().label(title).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
            title.add_css_class("heading");
            let details = gtk::Label::builder().label(details_text(r)).xalign(0.0).css_classes(["dim-label"]).build();
            let name = gtk::Label::builder()
                .label(&r.name)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .css_classes(["dim-label"])
                .build();
            texts.append(&title);
            texts.append(&details);
            texts.append(&name);
            line.append(&number);
            line.append(&picture);
            line.append(&texts);
            let row = gtk::ListBoxRow::builder().child(&line).build();
            if r.info.page_type == "Deleted" {
                row.set_opacity(0.45);
            }
            self.attach_drag_source(&row, r.info.image);
            self.list.append(&row);
            if selected.contains(&r.info.image) {
                self.list.select_row(Some(&row));
            }
            widgets.insert(r.info.image, RowWidgets { picture, details, name });
        }
        self.widgets.replace(widgets);
        self.update_preview();
    }

    fn attach_drag_source(self: &Rc<Self>, row: &gtk::ListBoxRow, image: i32) {
        let drag = gtk::DragSource::builder().actions(gdk::DragAction::MOVE).build();
        let weak = Rc::downgrade(self);
        drag.connect_prepare(move |_, _, _| {
            let ed = weak.upgrade()?;
            ed.drag_source.set(Some(image));
            Some(gdk::ContentProvider::for_value(&image.to_string().to_value()))
        });
        let weak = Rc::downgrade(self);
        drag.connect_drag_begin(move |src, _| {
            if let Some(ed) = weak.upgrade()
                && let Some(t) = ed.rows.borrow().iter().find(|r| r.info.image == image).and_then(|r| r.thumb.clone())
            {
                src.set_icon(Some(&t), 0, 0);
            }
        });
        row.add_controller(drag);
    }

    fn update_preview(&self) {
        let rows = self.rows.borrow();
        let first = self.list.selected_rows().first().map(|r| r.index() as usize);
        match first.and_then(|i| rows.get(i).map(|r| (i, r))) {
            Some((i, r)) => {
                self.preview.set_paintable(r.thumb.as_ref());
                self.preview_label.set_text(&t!("Page {0}", i + 1));
            }
            None => {
                self.preview.set_paintable(None::<&gdk::Paintable>);
                self.preview_label.set_text("");
            }
        }
    }

    fn mark_changed(self: &Rc<Self>) {
        self.changed.set(true);
        let selected = self.selected_images();
        self.rebuild(&selected);
    }

    fn edit_selected(self: &Rc<Self>, f: impl Fn(&mut Row)) {
        let selected = self.selected_images();
        if selected.is_empty() {
            return;
        }
        for r in self.rows.borrow_mut().iter_mut().filter(|r| selected.contains(&r.info.image)) {
            f(r);
        }
        self.mark_changed();
    }

    /// "Mark as Deleted" toggles between deleted and story pages.
    fn toggle_deleted(self: &Rc<Self>) {
        self.edit_selected(|r| {
            r.info.page_type = if r.info.page_type == "Deleted" { "Story".into() } else { "Deleted".into() };
        });
    }

    fn reorder(self: &Rc<Self>, cmp: impl Fn(&Row, &Row) -> std::cmp::Ordering) {
        self.rows.borrow_mut().sort_by(cmp);
        self.mark_changed();
    }

    /// Moves `pages` (image indexes, kept in their current order) so that
    /// they start at display position `position`.
    fn move_pages(self: &Rc<Self>, position: usize, pages: &HashSet<i32>) {
        if pages.is_empty() {
            return;
        }
        {
            let mut rows = self.rows.borrow_mut();
            let before = rows.iter().take(position).filter(|r| pages.contains(&r.info.image)).count();
            let (moved, mut rest): (Vec<Row>, Vec<Row>) = rows.drain(..).partition(|r| pages.contains(&r.info.image));
            let at = (position - before).min(rest.len());
            rest.splice(at..at, moved);
            *rows = rest;
        }
        self.mark_changed();
    }

    /// Move to Top / Bottom. Moving front covers to the end gives the cover
    /// to the next page, as ComicRack does.
    fn move_to(self: &Rc<Self>, top: bool) {
        let selected = self.selected_images();
        if selected.is_empty() {
            return;
        }
        if top {
            self.move_pages(0, &selected);
            return;
        }
        {
            let mut rows = self.rows.borrow_mut();
            let all_covers = rows.iter().filter(|r| selected.contains(&r.info.image)).all(|r| r.info.page_type == "FrontCover");
            if all_covers {
                for r in rows.iter_mut().filter(|r| selected.contains(&r.info.image)) {
                    r.info.page_type = "Other".into();
                }
                let n = selected.len();
                if let Some(r) = rows.get_mut(n) {
                    r.info.page_type = "FrontCover".into();
                }
            }
        }
        let len = self.rows.borrow().len();
        self.move_pages(len, &selected);
    }

    fn ask_bookmark(self: &Rc<Self>) {
        let selected = self.selected_images();
        let Some(image) = self.rows.borrow().iter().find(|r| selected.contains(&r.info.image)).map(|r| r.info.image) else {
            return;
        };
        let current = self.rows.borrow().iter().find(|r| r.info.image == image).map(|r| r.info.bookmark.clone()).unwrap_or_default();
        let Some(parent) = self.widget.root().and_downcast::<gtk::Window>() else { return };
        let (win, content, buttons) = widgets::dialog(&parent, t!("Bookmark"), 360, 120);
        let entry = widgets::entry(&current);
        entry.set_activates_default(true);
        content.append(&entry);
        let cancel = widgets::button(t!("Cancel"), false);
        let ok = widgets::button(t!("OK"), true);
        cancel.connect_clicked(glib::clone!(
            #[weak]
            win,
            move |_| win.close()
        ));
        let weak = Rc::downgrade(self);
        ok.connect_clicked(glib::clone!(
            #[weak]
            win,
            #[weak]
            entry,
            move |_| {
                if let Some(ed) = weak.upgrade() {
                    let text = entry.text().trim().to_string();
                    if let Some(r) = ed.rows.borrow_mut().iter_mut().find(|r| r.info.image == image) {
                        r.info.bookmark = text;
                    }
                    ed.mark_changed();
                }
                win.close();
            }
        ));
        buttons.append(&cancel);
        buttons.append(&ok);
        win.set_default_widget(Some(&ok));
        win.present();
    }

    /// Copies the first selected page image to the clipboard.
    fn copy_page(self: &Rc<Self>) {
        let selected = self.selected_images();
        let Some(image) = self.rows.borrow().iter().find(|r| selected.contains(&r.info.image)).map(|r| r.info.image) else {
            return;
        };
        let path = self.path.clone();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(crate::archive::read_page(&path, image as usize).ok().map(|p| p.data));
        });
        let widget = self.widget.downgrade();
        glib::spawn_future_local(async move {
            if let (Ok(Some(data)), Some(w)) = (rx.recv().await, widget.upgrade())
                && let Ok(t) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(data))
            {
                w.clipboard().set_texture(&t);
            }
        });
    }

    /// Reads the pages of the book in the background for the thumbnails,
    /// names, sizes and dimensions.
    fn load_thumbnails(self: &Rc<Self>) {
        let path = self.path.clone();
        let (tx, rx) = async_channel::unbounded::<(i32, String, u64, u32, u32, Option<Vec<u8>>)>();
        std::thread::spawn(move || {
            let Ok(pages) = crate::archive::read_pages(&path) else { return };
            for (i, p) in pages.into_iter().enumerate() {
                let size = p.data.len() as u64;
                let (w, h, png) = match image::load_from_memory(&p.data) {
                    Ok(img) => {
                        let thumb = img.thumbnail(THUMB, THUMB);
                        let mut buf = std::io::Cursor::new(Vec::new());
                        let png = thumb.write_to(&mut buf, image::ImageFormat::Png).ok().map(|_| buf.into_inner());
                        (img.width(), img.height(), png)
                    }
                    Err(_) => (0, 0, None),
                };
                if tx.send_blocking((i as i32, p.name, size, w, h, png)).is_err() {
                    return;
                }
            }
        });
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok((image, name, size, w, h, png)) = rx.recv().await {
                let Some(ed) = weak.upgrade() else { return };
                let texture = png.and_then(|d| gdk::Texture::from_bytes(&glib::Bytes::from_owned(d)).ok());
                let mut rows = ed.rows.borrow_mut();
                let Some(r) = rows.iter_mut().find(|r| r.info.image == image) else { continue };
                r.name = name;
                r.size = size;
                r.width = w;
                r.height = h;
                r.thumb = texture;
                if let Some(rw) = ed.widgets.borrow().get(&image) {
                    rw.picture.set_paintable(r.thumb.as_ref());
                    rw.details.set_text(&details_text(r));
                    rw.name.set_text(&r.name);
                }
                drop(rows);
                if ed.list.selected_rows().first().is_some_and(|row| {
                    ed.rows.borrow().get(row.index() as usize).is_some_and(|r| r.info.image == image)
                }) {
                    ed.update_preview();
                }
            }
        });
    }
}

fn details_text(r: &Row) -> String {
    let mut parts = Vec::new();
    if r.width > 0 {
        parts.push(format!("{} × {}", r.width, r.height));
    }
    if r.size > 0 {
        parts.push(crate::util::human_size(r.size));
    }
    let rot = rotation_label(&r.info.rotation);
    if !rot.is_empty() {
        parts.push(t!("rotation {0}", rot));
    }
    if !r.info.position.is_empty() && r.info.position != "Default" {
        parts.push(t!("position: {0}", position_label(&r.info.position)));
    }
    parts.join(" · ")
}
