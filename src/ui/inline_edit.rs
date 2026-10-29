//! Book list cells and in-place editing, as in ComicRack CE's item view:
//!
//! - a slow click on a cell of an already selected row (or F2) edits the cell;
//! - Tab / Shift+Tab (or Alt+Left / Alt+Right) move to the next / previous
//!   editable column, Up / Down to the same column of the next / previous
//!   row, Enter validates, Escape cancels, leaving the cell validates;
//! - while typing, values already present in the library are suggested
//!   (for multi-value fields, for the item being typed after the last comma);
//! - the value is applied to every selected book;
//! - numeric columns of the selected rows show small up/down buttons that
//!   increment / decrement the value (same clamping rules as ComicRack).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use gtk::{gdk, glib};

use super::{AppState, BookObject, State};
use crate::model::{Book, Field, FieldKind};
use crate::util::{natural_cmp, parse_number, split_list};

/// Delay of the "slow click" that starts editing (ComicRack: 1 s).
const SLOW_CLICK_MS: u64 = 700;
const MAX_SUGGESTIONS: usize = 12;

#[derive(Default)]
pub struct Inline {
    /// Bound cells, to find the widget of a (book, column) pair.
    cells: RefCell<HashMap<(u64, Field), glib::WeakRef<gtk::Box>>>,
    session: RefCell<Option<Rc<Session>>>,
    /// Incremented on every click, to cancel a pending slow click.
    clicks: Cell<u64>,
}

struct Session {
    book: u64,
    field: Field,
    cell: gtk::Box,
    entry: gtk::Entry,
    popover: gtk::Popover,
    list: gtk::ListBox,
    original: String,
    values: Vec<String>,
    shown: RefCell<Vec<String>>,
}

/// Columns that can be edited in place.
pub fn is_editable(field: Field) -> bool {
    match field {
        Field::PageCount => false,
        Field::FileName | Field::Review => true,
        f => matches!(f.kind(), FieldKind::Text | FieldKind::List | FieldKind::Int | FieldKind::Number),
    }
}

/// Columns with up/down buttons.
fn has_spin(field: Field) -> bool {
    matches!(
        field,
        Field::Volume
            | Field::Month
            | Field::Day
            | Field::Year
            | Field::Number
            | Field::Count
            | Field::AlternateNumber
            | Field::AlternateCount
    )
}

fn cell_markup(field: Field, book: &Book) -> String {
    match field.proposed(book) {
        Some(p) => format!("<span alpha=\"55%\"><i>{}</i></span>", glib::markup_escape_text(&p)),
        None => glib::markup_escape_text(&field.display(book)).to_string(),
    }
}

fn label_of(cell: &gtk::Box) -> Option<gtk::Label> {
    cell.first_child().and_downcast::<gtk::Label>()
}

fn spin_of(cell: &gtk::Box) -> Option<gtk::Box> {
    cell.first_child()?.next_sibling().and_downcast::<gtk::Box>()
}

fn book_id_of(cell: &gtk::Box) -> Option<u64> {
    // SAFETY: "book-id" is only ever set to a u64 in `bind`.
    unsafe { cell.data::<u64>("book-id").map(|p| *p.as_ref()) }
}

fn list_item_of(cell: &gtk::Box) -> Option<gtk::ListItem> {
    // SAFETY: "list-item" is only ever set to a WeakRef<ListItem> in `bind`.
    unsafe { cell.data::<glib::WeakRef<gtk::ListItem>>("list-item").and_then(|p| p.as_ref().upgrade()) }
}

/// Column of the book list for `field`, with editable cells.
pub fn make_column(state: &State, field: Field) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let numeric = matches!(field.kind(), FieldKind::Int | FieldKind::Number)
        || matches!(field, Field::FileSize | Field::ReadPercentage);
    let weak = Rc::downgrade(state);

    factory.connect_setup(glib::clone!(
        #[strong]
        weak,
        move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let cell = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            let label = gtk::Label::builder()
                .xalign(if numeric { 1.0 } else { 0.0 })
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .single_line_mode(true)
                .use_markup(true)
                .hexpand(true)
                .build();
            cell.append(&label);
            if has_spin(field) {
                let spin_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
                spin_box.set_valign(gtk::Align::Center);
                spin_box.set_visible(false);
                for (icon, delta) in [("pan-up-symbolic", 1), ("pan-down-symbolic", -1)] {
                    let b = gtk::Button::from_icon_name(icon);
                    b.add_css_class("flat");
                    b.add_css_class("spin-mini");
                    b.set_focusable(false);
                    b.connect_clicked(glib::clone!(
                        #[weak]
                        cell,
                        #[strong]
                        weak,
                        move |_| {
                            if let (Some(st), Some(id)) = (weak.upgrade(), book_id_of(&cell)) {
                                spin(&st, id, field, delta);
                            }
                        }
                    ));
                    spin_box.append(&b);
                }
                cell.append(&spin_box);
            }
            if is_editable(field) {
                let click = gtk::GestureClick::builder().button(gdk::BUTTON_PRIMARY).build();
                // Was the row already selected before this click? (the row's
                // own click handler, which changes the selection, runs after
                // the cell's)
                let was_selected = Rc::new(Cell::new(false));
                click.connect_pressed(glib::clone!(
                    #[weak]
                    cell,
                    #[strong]
                    weak,
                    #[strong]
                    was_selected,
                    move |_, n, _, _| {
                        let Some(st) = weak.upgrade() else { return };
                        st.inline.clicks.set(st.inline.clicks.get() + 1);
                        let selected = list_item_of(&cell).is_some_and(|i| i.is_selected());
                        was_selected.set(n == 1 && selected && st.selection.selection().size() == 1);
                    }
                ));
                click.connect_released(glib::clone!(
                    #[weak]
                    cell,
                    #[strong]
                    weak,
                    move |_, n, _, _| {
                        let Some(st) = weak.upgrade() else { return };
                        if n != 1 || !was_selected.get() || st.inline.session.borrow().is_some() {
                            return;
                        }
                        let Some(id) = book_id_of(&cell) else { return };
                        let generation = st.inline.clicks.get();
                        let weak = weak.clone();
                        glib::timeout_add_local_once(std::time::Duration::from_millis(SLOW_CLICK_MS), move || {
                            if let Some(st) = weak.upgrade()
                                && st.inline.clicks.get() == generation
                                && st.inline.session.borrow().is_none()
                            {
                                start_edit(&st, id, field);
                            }
                        });
                    }
                ));
                cell.add_controller(click);
            }
            item.set_child(Some(&cell));
        }
    ));

    factory.connect_bind(glib::clone!(
        #[strong]
        weak,
        move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let obj = item.item().and_downcast::<BookObject>().unwrap();
            let cell = item.child().and_downcast::<gtk::Box>().unwrap();
            let label = label_of(&cell).unwrap();
            let binding = obj
                .bind_property("revision", &label, "label")
                .transform_to(move |b, _: u64| {
                    let o = b.source()?.downcast::<BookObject>().ok()?;
                    Some(cell_markup(field, &o.book()))
                })
                .sync_create()
                .build();
            let id = obj.id();
            // SAFETY: each key is only ever used with the type stored here.
            unsafe {
                label.set_data("binding", binding);
                cell.set_data("book-id", id);
                cell.set_data("list-item", item.downgrade());
            }
            if let Some(spin) = spin_of(&cell) {
                let b = item.bind_property("selected", &spin, "visible").sync_create().build();
                // SAFETY: as above.
                unsafe { spin.set_data("binding", b) };
            }
            if let Some(st) = weak.upgrade() {
                st.inline.cells.borrow_mut().insert((id, field), cell.downgrade());
            }
        }
    ));

    factory.connect_unbind(glib::clone!(
        #[strong]
        weak,
        move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(cell) = item.child().and_downcast::<gtk::Box>() else { return };
            if let Some(st) = weak.upgrade() {
                // The edited cell goes away (scrolling, sorting): keep the value.
                let editing_here = st.inline.session.borrow().as_ref().is_some_and(|s| s.cell == cell);
                if editing_here {
                    finish(&st, true, true);
                }
                if let Some(id) = book_id_of(&cell) {
                    let mut cells = st.inline.cells.borrow_mut();
                    if cells.get(&(id, field)).and_then(|w| w.upgrade()).is_some_and(|c| c == cell) {
                        cells.remove(&(id, field));
                    }
                }
            }
            // SAFETY: same types as stored in bind.
            unsafe {
                if let Some(label) = label_of(&cell)
                    && let Some(b) = label.steal_data::<glib::Binding>("binding")
                {
                    b.unbind();
                }
                if let Some(spin) = spin_of(&cell)
                    && let Some(b) = spin.steal_data::<glib::Binding>("binding")
                {
                    b.unbind();
                }
            }
        }
    ));

    let col = gtk::ColumnViewColumn::new(Some(field.label()), Some(factory));
    col.set_id(Some(field.key()));
    col.set_resizable(true);
    col.set_sorter(Some(&gtk::CustomSorter::new(move |a, b| {
        let a = a.downcast_ref::<BookObject>().unwrap();
        let b = b.downcast_ref::<BookObject>().unwrap();
        field.compare(&a.book(), &b.book()).into()
    })));
    if field == Field::Modified {
        col.set_title(Some("✎"));
    }
    col
}

/// Styles of the compact editing widgets.
pub fn install_css() {
    let css = gtk::CssProvider::new();
    css.load_from_string(
        "button.spin-mini { min-height: 0; min-width: 0; padding: 0 2px; margin: 0; border-radius: 3px; }
         button.spin-mini image { -gtk-icon-size: 10px; }
         entry.inline-edit { min-height: 0; padding-top: 0; padding-bottom: 0; }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
}

fn position_of(st: &AppState, id: u64) -> Option<u32> {
    (0..st.sort_model.n_items())
        .find(|i| st.sort_model.item(*i).and_downcast::<BookObject>().is_some_and(|o| o.id() == id))
}

fn column_of(st: &AppState, field: Field) -> Option<gtk::ColumnViewColumn> {
    st.columns.borrow().iter().find(|(f, _)| *f == field).map(|(_, c)| c.clone())
}

/// F2: edits the first editable visible column of the selected book.
pub fn edit_selection(st: &State) {
    let Some(book) = st.selected_books().into_iter().next() else { return };
    if let Some(field) = next_column(st, None, 1) {
        start_edit(st, book.id, field);
    }
}

/// Next editable visible column after `from` (or the first one) in the
/// displayed column order, in direction `dir`.
fn next_column(st: &AppState, from: Option<Field>, dir: i32) -> Option<Field> {
    let cols = st.column_view.columns();
    let fields: Vec<Field> = (0..cols.n_items())
        .filter_map(|i| cols.item(i).and_downcast::<gtk::ColumnViewColumn>())
        .filter(|c| c.is_visible())
        .filter_map(|c| c.id().and_then(|id| Field::from_key(&id)))
        .filter(|f| is_editable(*f))
        .collect();
    match from {
        None => fields.first().copied(),
        Some(f) => {
            let i = fields.iter().position(|x| *x == f)? as i32 + dir;
            (i >= 0).then(|| fields.get(i as usize).copied()).flatten()
        }
    }
}

/// Shows the cell of (book, field) and starts editing it.
pub fn start_edit(st: &State, book: u64, field: Field) {
    if !is_editable(field) || st.inline.session.borrow().is_some() {
        return;
    }
    let (Some(pos), Some(col)) = (position_of(st, book), column_of(st, field)) else { return };
    if !col.is_visible() {
        return;
    }
    st.column_view.scroll_to(pos, Some(&col), gtk::ListScrollFlags::FOCUS, None);
    // The cell may only be bound after the next layout.
    let weak = Rc::downgrade(st);
    glib::idle_add_local_once(move || {
        let Some(st) = weak.upgrade() else { return };
        if !begin(&st, book, field) {
            let weak = Rc::downgrade(&st);
            glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
                if let Some(st) = weak.upgrade() {
                    begin(&st, book, field);
                }
            });
        }
    });
}

/// Values of the library for the auto-completion of `field`.
fn completion_values(st: &AppState, field: Field) -> Vec<String> {
    if matches!(field.kind(), FieldKind::Int | FieldKind::Number) || field == Field::FileName {
        return Vec::new();
    }
    let mut set = BTreeSet::new();
    for b in &st.library.borrow().books {
        let v = field.get(b);
        if v.is_empty() {
            continue;
        }
        if field.kind() == FieldKind::List {
            set.extend(split_list(&v));
        } else {
            set.insert(v);
        }
    }
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort_by(|a, b| natural_cmp(a, b));
    v
}

fn begin(st: &State, book_id: u64, field: Field) -> bool {
    if st.inline.session.borrow().is_some() {
        return true;
    }
    let Some(cell) = st.inline.cells.borrow().get(&(book_id, field)).and_then(|w| w.upgrade()) else {
        return false;
    };
    let Some(book) = st.library.borrow().get(book_id).cloned() else { return true };
    let original = match field {
        Field::FileName => book.file_stem(),
        _ => field.proposed(&book).unwrap_or_else(|| field.get(&book)),
    };

    let entry = gtk::Entry::builder().text(&original).hexpand(true).build();
    entry.add_css_class("inline-edit");
    if let Some(l) = label_of(&cell) {
        l.set_visible(false);
    }
    if let Some(s) = spin_of(&cell) {
        s.set_visible(false);
    }
    cell.append(&entry);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(true)
        .can_focus(false)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(260)
        .propagate_natural_height(true)
        .min_content_width(200)
        .build();
    let popover = gtk::Popover::builder()
        .child(&scroll)
        .autohide(false)
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .can_focus(false)
        .halign(gtk::Align::Start)
        .build();
    popover.set_parent(&entry);

    let session = Rc::new(Session {
        book: book_id,
        field,
        cell,
        entry: entry.clone(),
        popover,
        list: list.clone(),
        original,
        values: completion_values(st, field),
        shown: RefCell::new(Vec::new()),
    });
    st.inline.session.replace(Some(session.clone()));

    let weak: Weak<AppState> = Rc::downgrade(st);
    entry.connect_changed(glib::clone!(
        #[strong]
        weak,
        move |_| {
            if let Some(st) = weak.upgrade()
                && let Some(s) = current(&st)
            {
                update_suggestions(&s);
            }
        }
    ));
    list.connect_row_activated(glib::clone!(
        #[strong]
        weak,
        move |_, row| {
            if let Some(st) = weak.upgrade()
                && let Some(s) = current(&st)
            {
                accept_suggestion(&s, row.index() as usize);
                s.entry.grab_focus_without_selecting();
            }
        }
    ));

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(
        #[strong]
        weak,
        move |_, key, _, mods| {
            let Some(st) = weak.upgrade() else { return glib::Propagation::Proceed };
            on_key(&st, key, mods)
        }
    ));
    entry.add_controller(keys);

    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave(glib::clone!(
        #[strong]
        weak,
        move |_| {
            let weak = weak.clone();
            // Let a key handler that moves to another cell finish first.
            glib::idle_add_local_once(move || {
                if let Some(st) = weak.upgrade()
                    && let Some(s) = current(&st)
                    && !s.entry.has_focus()
                    && s.entry.root().and_then(|r| r.focus()).is_none_or(|f| !f.is_ancestor(&s.entry))
                {
                    finish(&st, true, false);
                }
            });
        }
    ));
    entry.add_controller(focus);

    entry.grab_focus();
    entry.select_region(0, -1);
    true
}

fn current(st: &AppState) -> Option<Rc<Session>> {
    st.inline.session.borrow().clone()
}

/// Item being typed: the whole text, or the text after the last comma for
/// multi-value fields.
fn typed_token(s: &Session) -> String {
    let text = s.entry.text().to_string();
    if s.field.kind() == FieldKind::List {
        text.rsplit(',').next().unwrap_or("").trim_start().to_string()
    } else {
        text
    }
}

fn update_suggestions(s: &Session) {
    let token = typed_token(s);
    while let Some(row) = s.list.row_at_index(0) {
        s.list.remove(&row);
    }
    if token.is_empty() || s.values.is_empty() {
        s.popover.popdown();
        s.shown.borrow_mut().clear();
        return;
    }
    let lower = token.to_lowercase();
    let mut starts: Vec<&String> = Vec::new();
    let mut contains: Vec<&String> = Vec::new();
    for v in &s.values {
        let l = v.to_lowercase();
        if l == lower {
            continue;
        }
        if l.starts_with(&lower) {
            starts.push(v);
        } else if l.contains(&lower) {
            contains.push(v);
        }
    }
    let shown: Vec<String> = starts.into_iter().chain(contains).take(MAX_SUGGESTIONS).cloned().collect();
    for v in &shown {
        s.list.append(&gtk::Label::builder().label(v).xalign(0.0).margin_start(4).margin_end(4).build());
    }
    let empty = shown.is_empty();
    s.shown.replace(shown);
    if empty {
        s.popover.popdown();
    } else {
        s.popover.popup();
    }
}

fn accept_suggestion(s: &Session, index: usize) {
    let Some(v) = s.shown.borrow().get(index).cloned() else { return };
    let text = s.entry.text().to_string();
    let new = match text.rfind(',').filter(|_| s.field.kind() == FieldKind::List) {
        Some(i) => format!("{}, {v}", text[..i].trim_end()),
        None => v,
    };
    s.entry.set_text(&new);
    s.entry.set_position(-1);
    s.popover.popdown();
}

fn on_key(st: &State, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
    let Some(s) = current(st) else { return glib::Propagation::Proceed };
    let popup = s.popover.is_visible() && !s.shown.borrow().is_empty();
    let selected_row = s.list.selected_row().map(|r| r.index() as usize);
    let alt = mods.contains(gdk::ModifierType::ALT_MASK);
    match key {
        gdk::Key::Escape => {
            if popup {
                s.popover.popdown();
            } else {
                finish(st, false, false);
            }
        }
        gdk::Key::Return | gdk::Key::KP_Enter => match (popup, selected_row) {
            (true, Some(i)) => accept_suggestion(&s, i),
            _ => {
                finish(st, true, false);
            }
        },
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => {
            if let (true, Some(i)) = (popup, selected_row) {
                accept_suggestion(&s, i);
            }
            let dir = if key == gdk::Key::ISO_Left_Tab || mods.contains(gdk::ModifierType::SHIFT_MASK) { -1 } else { 1 };
            move_column(st, &s, dir);
        }
        gdk::Key::Left | gdk::Key::Right if alt => {
            move_column(st, &s, if key == gdk::Key::Left { -1 } else { 1 });
        }
        gdk::Key::Up | gdk::Key::Down => {
            let dir: i32 = if key == gdk::Key::Up { -1 } else { 1 };
            if popup {
                let n = s.shown.borrow().len() as i32;
                let i = match selected_row {
                    Some(i) => (i as i32 + dir).clamp(0, n - 1),
                    None if dir > 0 => 0,
                    None => return glib::Propagation::Stop,
                };
                if let Some(row) = s.list.row_at_index(i) {
                    s.list.select_row(Some(&row));
                }
            } else {
                move_row(st, &s, dir);
            }
        }
        _ => return glib::Propagation::Proceed,
    }
    glib::Propagation::Stop
}

fn move_column(st: &State, s: &Session, dir: i32) {
    let Some(next) = next_column(st, Some(s.field), dir) else { return };
    let book = s.book;
    finish(st, true, false);
    start_edit(st, book, next);
}

fn move_row(st: &State, s: &Session, dir: i32) {
    let (book, field) = (s.book, s.field);
    finish(st, true, false);
    let Some(pos) = position_of(st, book) else { return };
    let target = pos as i64 + dir as i64;
    if target < 0 || target >= st.sort_model.n_items() as i64 {
        return;
    }
    let Some(obj) = st.sort_model.item(target as u32).and_downcast::<BookObject>() else { return };
    st.selection.select_item(target as u32, true);
    start_edit(st, obj.id(), field);
}

/// Ends the edition; with `commit`, applies the new value (later, in an idle
/// callback, when called while the list is being rebuilt).
fn finish(st: &State, commit: bool, deferred: bool) {
    let Some(s) = st.inline.session.take() else { return };
    let text = s.entry.text().trim().to_string();
    s.popover.popdown();
    s.popover.unparent();
    s.cell.remove(&s.entry);
    if let Some(l) = label_of(&s.cell) {
        l.set_visible(true);
    }
    if let Some(spin) = spin_of(&s.cell) {
        spin.set_visible(list_item_of(&s.cell).is_some_and(|i| i.is_selected()));
    }
    if !commit || text == s.original {
        return;
    }
    let (book, field) = (s.book, s.field);
    if deferred {
        let weak = Rc::downgrade(st);
        glib::idle_add_local_once(move || {
            if let Some(st) = weak.upgrade() {
                apply(&st, book, field, &text);
            }
        });
    } else {
        apply(st, book, field, &text);
    }
}

/// Books changed by an edit of `book`: all the selected books when it is
/// selected (ComicRack applies a cell edit to the whole selection).
fn targets(st: &AppState, book: u64) -> Vec<Book> {
    let selected = st.selected_books();
    if selected.iter().any(|b| b.id == book) {
        selected
    } else {
        st.library.borrow().get(book).cloned().into_iter().collect()
    }
}

fn apply(st: &State, book: u64, field: Field, text: &str) {
    if field == Field::FileName {
        rename(st, book, text);
        return;
    }
    if field.kind() == FieldKind::Int && !text.is_empty() && text.parse::<i32>().is_err() {
        st.journal.error(&t!("Invalid number for {0}: {1}", field.label(), text));
        st.set_status(&t!("Invalid number for {0}: {1}", field.label(), text));
        return;
    }
    let mut books = targets(st, book);
    for b in &mut books {
        field.set(b, text);
    }
    st.update_books(books, field.in_comicinfo());
}

/// Renames the book file (the extension is kept).
fn rename(st: &State, id: u64, text: &str) {
    let Some(mut book) = st.library.borrow().get(id).cloned() else { return };
    let ext = book.file_path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let mut name = text.trim().to_string();
    if !ext.is_empty() && !name.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) {
        name = format!("{name}.{ext}");
    }
    if name.contains(['/', '\\']) || name.starts_with('.') {
        st.journal.error(&t!("Invalid file name: {0}", name));
        return;
    }
    let dest = book.file_path.with_file_name(&name);
    if dest == book.file_path {
        return;
    }
    let case_only = dest.to_string_lossy().to_lowercase() == book.file_path.to_string_lossy().to_lowercase();
    if dest.exists() && !case_only {
        let msg = t!("A file named {0} already exists", name);
        st.journal.error(&msg);
        super::widgets::alert(&st.window, t!("Rename"), &msg);
        return;
    }
    match std::fs::rename(&book.file_path, &dest) {
        Ok(()) => {
            st.journal.info(&t!("Renamed: {0} → {1}", book.file_path.display(), dest.display()));
            book.file_path = dest;
            book.refresh_file_stats();
            st.update_books(vec![book], false);
        }
        Err(e) => {
            let msg = t!("{0}: {1}", book.file_path.display(), e);
            st.journal.error(&msg);
            super::widgets::alert(&st.window, t!("Rename"), &msg);
        }
    }
}

/// ComicRack's clamping for 1-based values: going below 1 clears the value
/// (-1), going up from "unset" starts at 1.
fn one_clamping(max: i32, old: i32, new: i32) -> i32 {
    if new > max {
        max
    } else if new < 1 {
        if old <= new { 1 } else { -1 }
    } else {
        new
    }
}

/// Up/down buttons: the clicked book's value ± 1, applied to the selection.
fn spin(st: &State, id: u64, field: Field, delta: i32) {
    finish(st, true, false);
    let Some(book) = st.library.borrow().get(id).cloned() else { return };
    let i = &book.info;
    let number = |f: Field| {
        let v = f.proposed(&book).unwrap_or_else(|| f.get(&book));
        parse_number(&v).map(|n| n.trunc() as i32).unwrap_or(0)
    };
    let start = match field {
        Field::Volume => i.volume,
        Field::Month => i.month,
        Field::Day => i.day,
        Field::Year => i.year,
        Field::Count => i.count,
        Field::AlternateCount => i.alternate_count,
        Field::Number | Field::AlternateNumber => number(field),
        _ => return,
    };
    let new = start + delta;
    let value = match field {
        Field::Volume => new.clamp(-1, i32::MAX),
        Field::Month => one_clamping(12, start, new),
        Field::Day => one_clamping(31, start, new),
        Field::Year => one_clamping(10000, start, new),
        Field::Count | Field::AlternateCount => one_clamping(i32::MAX, start, new),
        _ => new,
    };
    let mut books = targets(st, id);
    for b in &mut books {
        field.set(b, &value.to_string());
    }
    st.update_books(books, true);
}

/// Development helpers for `BO_ACTIONS` (see `debug_screenshots`).
#[cfg(debug_assertions)]
pub fn debug_action(st: &State, action: &str) {
    if let Some(n) = action.strip_prefix("select:") {
        st.selection.select_item(n.parse().unwrap_or(0), true);
    } else if let Some(text) = action.strip_prefix("type:") {
        if let Some(s) = current(st) {
            s.entry.set_text(text);
            s.entry.set_position(-1);
        }
    } else if let Some(key) = action.strip_prefix("key:") {
        let key = match key {
            "Tab" => gdk::Key::Tab,
            "Down" => gdk::Key::Down,
            "Up" => gdk::Key::Up,
            "Return" => gdk::Key::Return,
            _ => gdk::Key::Escape,
        };
        on_key(st, key, gdk::ModifierType::empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamping_like_comicrack() {
        assert_eq!(one_clamping(12, 12, 13), 12);
        assert_eq!(one_clamping(12, 1, 0), -1);
        assert_eq!(one_clamping(12, -1, 0), 1);
        assert_eq!(one_clamping(12, 5, 6), 6);
    }

    #[test]
    fn editable_columns() {
        assert!(is_editable(Field::Series));
        assert!(is_editable(Field::Number));
        assert!(is_editable(Field::Writer));
        assert!(is_editable(Field::FileName));
        assert!(!is_editable(Field::PageCount));
        assert!(!is_editable(Field::Summary));
        assert!(!is_editable(Field::FileSize));
    }
}
