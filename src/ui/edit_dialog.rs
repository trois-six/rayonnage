//! "Info" dialog: edits the metadata of one or several books, like ComicRack's
//! ComicBookDialog (Summary, Details, Plot & Notes, Catalog, Custom, Pages tabs).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use super::widgets;
use super::{BookObject, State};
use crate::model::{Book, Field, FieldKind};

/// Marker shown when the selected books have different values.
fn mixed_label() -> &'static str {
    t!("(multiple values)")
}

const AGE_RATINGS: &[&str] = &[
    "Unknown", "Adults Only 18+", "Early Childhood", "Everyone", "Everyone 10+", "G", "Kids to Adults",
    "M", "MA15+", "Mature 17+", "PG", "R18+", "Rating Pending", "Teen", "X18+",
];
const FORMATS: &[&str] = &[
    "Album", "Intégrale", "One Shot", "Hors-série", "Tankobon", "Annual", "Graphic Novel", "Limited Series",
    "Main Series", "Mini-Series", "TPB", "Hardcover", "Digital", "Web", "Magazine", "Preview",
];
const LANGUAGES: &[&str] = &["fr", "en", "de", "es", "it", "nl", "pt", "ja", "ko", "zh"];

/// Custom value row: name entry, value entry, original name, changed flag.
type CustomRow = (gtk::Entry, gtk::Entry, String, Rc<Cell<bool>>);

enum Editor {
    Entry(gtk::Entry),
    Text(gtk::TextView),
    Drop(gtk::DropDown, Vec<&'static str>),
}

struct FieldEditor {
    field: Field,
    editor: Editor,
    changed: Rc<Cell<bool>>,
}

impl FieldEditor {
    fn value(&self) -> String {
        match &self.editor {
            Editor::Entry(e) => e.text().trim().to_string(),
            Editor::Text(t) => {
                let b = t.buffer();
                b.text(&b.start_iter(), &b.end_iter(), false).trim().to_string()
            }
            Editor::Drop(d, values) => values.get(d.selected() as usize).copied().unwrap_or("").to_string(),
        }
    }
}

struct Editors {
    list: RefCell<Vec<FieldEditor>>,
    library_values: BTreeMap<Field, BTreeSet<String>>,
}

impl Editors {
    /// Creates the editor for a field, pre-filled with the common value of the books.
    fn make(&self, field: Field, books: &[Book]) -> gtk::Widget {
        let values: BTreeSet<String> = books.iter().map(|b| field.get(b)).collect();
        let mixed = values.len() > 1;
        let value = if mixed { String::new() } else { values.into_iter().next().unwrap_or_default() };
        let changed = Rc::new(Cell::new(false));
        let (editor, widget): (Editor, gtk::Widget) = match field.kind() {
            FieldKind::LongText => {
                let t = gtk::TextView::builder().wrap_mode(gtk::WrapMode::WordChar).accepts_tab(false).build();
                t.buffer().set_text(&value);
                let c = changed.clone();
                t.buffer().connect_changed(move |_| c.set(true));
                let sw = gtk::ScrolledWindow::builder().child(&t).min_content_height(120).vexpand(true).build();
                sw.add_css_class("frame");
                (Editor::Text(t), sw.upcast())
            }
            FieldKind::YesNo | FieldKind::Manga => {
                let (keys, labels): (Vec<&'static str>, Vec<&str>) = if field.kind() == FieldKind::YesNo {
                    (vec!["", "Yes", "No"], vec![t!("Unknown"), t!("Yes"), t!("No")])
                } else {
                    (
                        vec!["", "Yes", "YesAndRightToLeft", "No"],
                        vec![t!("Unknown"), t!("Yes"), t!("Yes (Right to Left)"), t!("No")],
                    )
                };
                let mut keys = keys;
                let mut labels = labels;
                if mixed {
                    keys.insert(0, "\u{0}");
                    labels.insert(0, mixed_label());
                }
                let d = gtk::DropDown::from_strings(&labels);
                let idx = if mixed { 0 } else { keys.iter().position(|k| *k == value).unwrap_or(0) };
                d.set_selected(idx as u32);
                let c = changed.clone();
                d.connect_selected_notify(move |d| c.set(!(mixed && d.selected() == 0)));
                (Editor::Drop(d.clone(), keys), d.upcast())
            }
            _ => {
                let e = gtk::Entry::builder().text(&value).build();
                if mixed {
                    e.set_placeholder_text(Some(mixed_label()));
                }
                if field.kind() == FieldKind::Rating {
                    e.set_placeholder_text(Some(if mixed { mixed_label() } else { t!("0 to 5") }));
                }
                let c = changed.clone();
                e.connect_changed(move |_| c.set(true));
                let suggestions = self.suggestions(field);
                let w: gtk::Widget = if suggestions.is_empty() {
                    e.clone().upcast()
                } else {
                    suggest_box(&e, field, suggestions)
                };
                (Editor::Entry(e), w)
            }
        };
        self.list.borrow_mut().push(FieldEditor { field, editor, changed });
        widget
    }

    fn suggestions(&self, field: Field) -> Vec<String> {
        let mut out: Vec<String> = match field {
            Field::AgeRating => AGE_RATINGS.iter().map(|s| s.to_string()).collect(),
            Field::Format => FORMATS.iter().map(|s| s.to_string()).collect(),
            Field::LanguageISO => LANGUAGES.iter().map(|s| s.to_string()).collect(),
            _ => Vec::new(),
        };
        if let Some(v) = self.library_values.get(&field) {
            for x in v {
                if !out.contains(x) {
                    out.push(x.clone());
                }
            }
        }
        out.truncate(300);
        out
    }
}

/// Entry + drop-down button listing known values. For list fields, the
/// chosen value is appended instead of replacing the text.
fn suggest_box(entry: &gtk::Entry, field: Field, values: Vec<String>) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    entry.set_hexpand(true);
    b.append(entry);
    let list = gtk::ListBox::new();
    for v in &values {
        list.append(&gtk::Label::builder().label(v).xalign(0.0).build());
    }
    let sw = gtk::ScrolledWindow::builder()
        .child(&list)
        .max_content_height(300)
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let pop = gtk::Popover::builder().child(&sw).build();
    let mb = gtk::MenuButton::builder().popover(&pop).icon_name("pan-down-symbolic").build();
    let append = field.kind() == FieldKind::List;
    list.connect_row_activated(glib::clone!(
        #[weak]
        entry,
        #[weak]
        pop,
        move |_, row| {
            let v = values[row.index() as usize].clone();
            let cur = entry.text().trim().to_string();
            if append && !cur.is_empty() {
                if !crate::util::split_list(&cur).contains(&v) {
                    entry.set_text(&format!("{cur}, {v}"));
                }
            } else {
                entry.set_text(&v);
            }
            pop.popdown();
        }
    ));
    b.append(&mb);
    b.upcast()
}

fn grid() -> gtk::Grid {
    gtk::Grid::builder()
        .row_spacing(6)
        .column_spacing(10)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build()
}

fn label(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(1.0).build()
}

/// Lays out fields in rows; each row holds 1 or 2 fields (4 grid columns).
fn fields_grid(ed: &Editors, books: &[Book], rows: &[&[Field]]) -> gtk::Grid {
    let g = grid();
    for (r, row) in rows.iter().enumerate() {
        for (c, f) in row.iter().enumerate() {
            let w = ed.make(*f, books);
            w.set_hexpand(true);
            g.attach(&label(&t!("{0}:", f.label())), (c * 2) as i32, r as i32, 1, 1);
            let span = if row.len() == 1 { 3 } else { 1 };
            g.attach(&w, (c * 2 + 1) as i32, r as i32, span, 1);
        }
    }
    g
}

fn page(widget: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(widget)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build()
}

fn library_values(state: &State) -> BTreeMap<Field, BTreeSet<String>> {
    let mut map: BTreeMap<Field, BTreeSet<String>> = BTreeMap::new();
    let fields = [
        Field::Series, Field::Publisher, Field::Imprint, Field::Genre, Field::Tags, Field::Format,
        Field::Writer, Field::Penciller, Field::Colorist, Field::Inker, Field::Letterer, Field::CoverArtist,
        Field::StoryArc, Field::SeriesGroup, Field::Characters, Field::Teams, Field::Locations,
        Field::BookStore, Field::BookOwner, Field::BookCondition, Field::BookCollectionStatus, Field::BookLocation,
        Field::LanguageISO, Field::AgeRating, Field::Translator, Field::Editor,
    ];
    for b in &state.library.borrow().books {
        for f in fields {
            let v = f.get(b);
            if v.is_empty() {
                continue;
            }
            let set = map.entry(f).or_default();
            if f.kind() == FieldKind::List {
                set.extend(crate::util::split_list(&v));
            } else {
                set.insert(v);
            }
        }
    }
    map
}

pub fn open(state: &State) {
    let Some(books) = state.require_selection() else { return };
    show(state, books);
}

fn show(state: &State, books: Vec<Book>) {
    let multi = books.len() > 1;
    let title = if multi {
        t!("Info — {0} books", books.len())
    } else {
        t!("Info — {0}", books[0].caption())
    };
    let (win, content, buttons) = widgets::dialog(&state.window, &title, 860, 700);
    let ed = Rc::new(Editors { list: RefCell::new(Vec::new()), library_values: library_values(state) });
    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);
    content.append(&notebook);

    // --- Summary
    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    summary.set_margin_start(12);
    summary.set_margin_top(12);
    summary.set_margin_end(12);
    let cover = gtk::Picture::builder().width_request(260).height_request(380).content_fit(gtk::ContentFit::Contain).build();
    summary.append(&cover);
    let sg = grid();
    sg.set_hexpand(true);
    let first = books[0].clone();
    let r = Cell::new(0);
    let next_row = || {
        let v = r.get();
        r.set(v + 1);
        v
    };
    let info_row = |k: &str, v: String| {
        let row = next_row();
        sg.attach(&label(k), 0, row, 1, 1);
        sg.attach(&gtk::Label::builder().label(v).xalign(0.0).wrap(true).selectable(true).build(), 1, row, 3, 1);
    };
    if multi {
        info_row(t!("Books:"), t!("{0} selected", books.len()));
        info_row(t!("Total size:"), crate::util::human_size(books.iter().map(|b| b.file_size).sum()));
    } else {
        let caption = gtk::Label::builder().xalign(0.0).wrap(true).build();
        caption.set_markup(&format!("<big><b>{}</b></big>", glib::markup_escape_text(&first.caption())));
        sg.attach(&caption, 0, next_row(), 4, 1);
        info_row(t!("Type:"), first.file_format());
        info_row(t!("Location:"), first.file_path.display().to_string());
        info_row(t!("Pages:"), first.info.page_count.to_string());
        info_row(t!("Size:"), crate::util::human_size(first.file_size));
        info_row(t!("Added:"), Field::Added.get(&first));
        info_row(t!("Modified:"), Field::FileModified.get(&first));
    }
    for f in [Field::Rating, Field::CommunityRating, Field::Web] {
        let w = ed.make(f, &books);
        w.set_hexpand(true);
        let row = next_row();
        sg.attach(&label(&t!("{0}:", f.label())), 0, row, 1, 1);
        sg.attach(&w, 1, row, 3, 1);
    }
    summary.append(&sg);
    notebook.append_page(&page(&summary), Some(&gtk::Label::new(Some(t!("Summary")))));
    load_cover(&cover, first.clone());

    // --- Details
    let details = fields_grid(
        &ed,
        &books,
        &[
            &[Field::Series],
            &[Field::Number, Field::Count],
            &[Field::Volume, Field::Title],
            &[Field::AlternateSeries],
            &[Field::AlternateNumber, Field::AlternateCount],
            &[Field::StoryArc, Field::StoryArcNumber],
            &[Field::SeriesGroup, Field::SeriesComplete],
            &[Field::Year, Field::Month],
            &[Field::Day, Field::Format],
            &[Field::Publisher, Field::Imprint],
            &[Field::Genre],
            &[Field::Tags],
            &[Field::Writer],
            &[Field::Penciller, Field::Inker],
            &[Field::Colorist, Field::Letterer],
            &[Field::CoverArtist, Field::Editor],
            &[Field::Translator, Field::AgeRating],
            &[Field::LanguageISO, Field::Manga],
            &[Field::BlackAndWhite],
        ],
    );
    notebook.append_page(&page(&details), Some(&gtk::Label::new(Some(t!("Details")))));

    // --- Plot & notes
    let plot = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let texts = gtk::Notebook::new();
    for f in [Field::Summary, Field::Notes, Field::Review] {
        let w = ed.make(f, &books);
        w.set_margin_start(6);
        w.set_margin_end(6);
        w.set_margin_top(6);
        w.set_margin_bottom(6);
        texts.append_page(&w, Some(&gtk::Label::new(Some(f.label()))));
    }
    texts.set_margin_start(12);
    texts.set_margin_end(12);
    texts.set_margin_top(12);
    texts.set_vexpand(true);
    plot.append(&texts);
    plot.append(&fields_grid(
        &ed,
        &books,
        &[
            &[Field::Characters],
            &[Field::Teams],
            &[Field::Locations],
            &[Field::MainCharacterOrTeam],
            &[Field::ScanInformation],
        ],
    ));
    notebook.append_page(&plot, Some(&gtk::Label::new(Some(t!("Plot & Notes")))));

    // --- Catalog
    let catalog = gtk::Box::new(gtk::Orientation::Vertical, 6);
    catalog.append(&fields_grid(
        &ed,
        &books,
        &[
            &[Field::Isbn, Field::Gtin],
            &[Field::BookLocation, Field::BookPrice],
            &[Field::BookAge, Field::BookCollectionStatus],
            &[Field::BookCondition, Field::BookStore],
            &[Field::BookOwner],
        ],
    ));
    let notes_grid = fields_grid(&ed, &books, &[&[Field::BookNotes]]);
    notes_grid.set_vexpand(true);
    catalog.append(&notes_grid);
    if !multi {
        let ro = grid();
        for (i, f) in [Field::Released, Field::Added, Field::Opened].iter().enumerate() {
            ro.attach(&label(&t!("{0}:", f.label())), 0, i as i32, 1, 1);
            ro.attach(&gtk::Label::builder().label(f.get(&first)).xalign(0.0).build(), 1, i as i32, 1, 1);
        }
        catalog.append(&ro);
    }
    notebook.append_page(&page(&catalog), Some(&gtk::Label::new(Some(t!("Catalog")))));

    // --- Custom values
    let custom_entries: Rc<RefCell<Vec<CustomRow>>> = Rc::new(RefCell::new(Vec::new()));
    let cgrid = grid();
    let mut names: BTreeSet<String> = state.settings.borrow().custom_fields.iter().cloned().collect();
    for b in &books {
        names.extend(b.custom_values.keys().cloned());
    }
    let add_custom_row = {
        let custom_entries = custom_entries.clone();
        let cgrid = cgrid.clone();
        let books = books.clone();
        move |name: &str| {
            let values: BTreeSet<String> = books.iter().map(|b| b.custom_values.get(name).cloned().unwrap_or_default()).collect();
            let mixed = values.len() > 1;
            let n = gtk::Entry::builder().text(name).placeholder_text(t!("Name")).build();
            let v = gtk::Entry::builder()
                .text(if mixed { String::new() } else { values.into_iter().next().unwrap_or_default() })
                .placeholder_text(if mixed { mixed_label() } else { t!("Value") })
                .hexpand(true)
                .build();
            let changed = Rc::new(Cell::new(false));
            let c = changed.clone();
            v.connect_changed(move |_| c.set(true));
            let c = changed.clone();
            n.connect_changed(move |_| c.set(true));
            let row = custom_entries.borrow().len() as i32;
            cgrid.attach(&n, 0, row, 1, 1);
            cgrid.attach(&v, 1, row, 1, 1);
            custom_entries.borrow_mut().push((n, v, name.to_string(), changed));
        }
    };
    for n in &names {
        add_custom_row(n);
    }
    let cbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
    cbox.append(&cgrid);
    let add_btn = gtk::Button::with_label(t!("Add Value"));
    add_btn.set_halign(gtk::Align::Start);
    add_btn.set_margin_start(12);
    add_btn.connect_clicked(move |_| add_custom_row(""));
    cbox.append(&add_btn);
    notebook.append_page(&page(&cbox), Some(&gtk::Label::new(Some(t!("Custom")))));

    // --- Pages (single book linked to a file)
    let pages_editor = (!multi && first.info.page_count > 0).then(|| {
        let ed = super::pages_editor::PagesEditor::new(&first);
        notebook.append_page(&ed.widget, Some(&gtk::Label::new(Some(t!("Pages")))));
        ed
    });

    // --- Buttons
    let apply: Rc<dyn Fn() -> bool> = {
        let ed = ed.clone();
        let st = state.clone();
        let books = books.clone();
        let custom_entries = custom_entries.clone();
        let pages_editor = pages_editor.clone();
        Rc::new(move || {
            // Validation
            for fe in ed.list.borrow().iter().filter(|fe| fe.changed.get()) {
                let v = fe.value();
                let bad = match fe.field.kind() {
                    FieldKind::Int => !v.is_empty() && v.parse::<i32>().is_err(),
                    FieldKind::Rating => !v.is_empty() && v.replace(',', ".").parse::<f32>().map(|x| !(0.0..=5.0).contains(&x)).unwrap_or(true),
                    _ => false,
                };
                if bad {
                    widgets::alert(&st.window, t!("Invalid value"), &t!("\"{0}\" is not a valid value for the {1} field.", v, fe.field.label()));
                    return false;
                }
            }
            let mut out = Vec::new();
            let mut any_info = false;
            for b in &books {
                let mut nb = st.library.borrow().get(b.id).cloned().unwrap_or_else(|| b.clone());
                let mut changed_info = false;
                for fe in ed.list.borrow().iter().filter(|fe| fe.changed.get()) {
                    let v = fe.value();
                    if fe.field.get(&nb) != v {
                        fe.field.set(&mut nb, &v);
                        changed_info |= fe.field.in_comicinfo();
                    }
                }
                for (n, v, orig, changed) in custom_entries.borrow().iter() {
                    if !changed.get() {
                        continue;
                    }
                    let name = n.text().trim().to_string();
                    if !orig.is_empty() && *orig != name {
                        nb.custom_values.remove(orig);
                    }
                    if !name.is_empty() {
                        let val = v.text().trim().to_string();
                        if val.is_empty() {
                            nb.custom_values.remove(&name);
                        } else {
                            nb.custom_values.insert(name, val);
                        }
                    }
                }
                if let Some(pages) = pages_editor.as_ref().and_then(|ed| ed.pages()) {
                    nb.info.pages = pages;
                    changed_info = true;
                }
                if changed_info {
                    nb.info_dirty = true;
                    any_info = true;
                }
                out.push(nb);
            }
            st.update_books(out, any_info);
            true
        })
    };

    if !multi {
        let prev = widgets::button(t!("◀ Previous"), false);
        let next = widgets::button(t!("Next ▶"), false);
        for (btn, delta) in [(&prev, -1i64), (&next, 1i64)] {
            let apply = apply.clone();
            let st = state.clone();
            let id = first.id;
            btn.connect_clicked(glib::clone!(
                #[weak]
                win,
                move |_| {
                    if !apply() {
                        return;
                    }
                    let n = st.sort_model.n_items() as i64;
                    let pos = (0..n).find(|i| {
                        st.sort_model.item(*i as u32).and_downcast::<BookObject>().map(|o| o.id()) == Some(id)
                    });
                    let Some(pos) = pos else { return };
                    let np = pos + delta;
                    if np < 0 || np >= n {
                        return;
                    }
                    st.selection.select_item(np as u32, true);
                    st.column_view.scroll_to(np as u32, None, gtk::ListScrollFlags::FOCUS, None);
                    win.close();
                    if let Some(b) = st.selected_books().into_iter().next() {
                        show(&st, vec![b]);
                    }
                }
            ));
        }
        prev.set_margin_end(24);
        buttons.append(&prev);
        buttons.append(&next);
    }
    let cancel = widgets::button(t!("Cancel"), false);
    let ok = widgets::button(t!("OK"), true);
    cancel.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    ok.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            if apply() {
                win.close();
            }
        }
    ));
    buttons.append(&cancel);
    buttons.append(&ok);
    win.set_default_widget(Some(&ok));
    #[cfg(debug_assertions)]
    if let Ok(t) = std::env::var("BO_TAB") {
        notebook.set_current_page(t.parse().ok());
    }
    win.present();
}

/// Display name of a page type key (`PAGE_TYPES`).
pub(super) fn page_type_label(key: &str) -> &'static str {
    match key {
        "FrontCover" => t!("Front Cover"),
        "InnerCover" => t!("Inner Cover"),
        "Roundup" => t!("Roundup"),
        "Story" => t!("Story"),
        "Advertisement" => t!("Advertisement"),
        "Editorial" => t!("Editorial"),
        "Letters" => t!("Letters"),
        "Preview" => t!("Preview"),
        "BackCover" => t!("Back Cover"),
        "Other" => t!("Other"),
        "Deleted" => t!("Deleted"),
        _ => t!("Story"),
    }
}

fn load_cover(picture: &gtk::Picture, book: Book) {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(crate::library::thumbnail(&book, 400).ok());
    });
    let picture = picture.downgrade();
    glib::spawn_future_local(async move {
        if let (Ok(Some(data)), Some(p)) = (rx.recv().await, picture.upgrade())
            && let Ok(t) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(data))
        {
            p.set_paintable(Some(&t));
        }
    });
}
