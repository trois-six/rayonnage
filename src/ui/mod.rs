//! GTK user interface: main window, book list, actions and background jobs.

mod autonumber_dialog;
mod book_object;
mod columns_dialog;
mod convert_dialog;
mod edit_dialog;
mod icons;
mod import_dialog;
mod inline_edit;
mod journal;
mod organizer_ui;
mod pages_editor;
mod scraper_ui;
mod settings_dialog;
mod split_dialog;
pub mod theme;
pub mod widgets;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::archive;
use crate::library::{self, Library, ScanEvent};
use crate::model::{Book, Field, default_book_cmp};
use crate::settings::Settings;
pub use book_object::BookObject;

pub const APP_ID: &str = "org.troissix.Rayonnage";

/// Sidebar entries.
#[derive(Clone, Debug, PartialEq)]
enum Source {
    All,
    Modified,
    Folder(PathBuf),
}

pub struct AppState {
    pub window: gtk::ApplicationWindow,
    pub library: RefCell<Library>,
    pub settings: RefCell<Settings>,
    store: gio::ListStore,
    sort_model: gtk::SortListModel,
    selection: gtk::MultiSelection,
    filter: gtk::CustomFilter,
    column_view: gtk::ColumnView,
    columns: RefCell<Vec<(Field, gtk::ColumnViewColumn)>>,
    sidebar: gtk::ListBox,
    sources: RefCell<Vec<Source>>,
    source: RefCell<Source>,
    search_text: RefCell<String>,
    status: gtk::Label,
    progress: gtk::ProgressBar,
    cancel_button: gtk::Button,
    busy: Cell<bool>,
    /// The user confirmed quitting while an operation was running.
    quitting: Cell<bool>,
    columns_save_pending: Cell<bool>,
    save_pending: Cell<bool>,
    info_panel: gtk::Box,
    info_cover: gtk::Picture,
    info_text: gtk::Label,
    info_generation: Cell<u64>,
    pub journal: journal::Journal,
    inline: inline_edit::Inline,
}

pub type State = Rc<AppState>;

pub fn build_ui(app: &gtk::Application) {
    let settings = Settings::load();
    let library = Library::load();

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Rayonnage")
        .default_width(settings.window_width)
        .default_height(settings.window_height)
        .build();

    let store = gio::ListStore::new::<BookObject>();
    let filter = gtk::CustomFilter::new(|_| true);
    let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
    let column_view = gtk::ColumnView::new(None::<gtk::MultiSelection>);
    let default_sorter = gtk::CustomSorter::new(|a, b| {
        let a = a.downcast_ref::<BookObject>().unwrap();
        let b = b.downcast_ref::<BookObject>().unwrap();
        default_book_cmp(&a.book(), &b.book()).into()
    });
    let sorter = gtk::MultiSorter::new();
    if let Some(s) = column_view.sorter() {
        sorter.append(s);
    }
    sorter.append(default_sorter);
    let sort_model = gtk::SortListModel::new(Some(filter_model), Some(sorter));
    let selection = gtk::MultiSelection::new(Some(sort_model.clone()));
    column_view.set_model(Some(&selection));
    column_view.set_show_column_separators(true);
    column_view.set_show_row_separators(false);
    column_view.add_css_class("data-table");

    let status = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::End).build();
    let progress = gtk::ProgressBar::builder().visible(false).valign(gtk::Align::Center).width_request(220).build();
    let cancel_button = gtk::Button::builder().label(t!("Cancel")).visible(false).build();

    let info_cover = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Contain)
        .height_request(320)
        .can_shrink(true)
        .build();
    let info_text = gtk::Label::builder()
        .xalign(0.0)
        .yalign(0.0)
        .wrap(true)
        .selectable(true)
        .use_markup(true)
        .build();
    let info_panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(8)
        .width_request(260)
        .build();
    info_panel.append(&info_cover);
    let info_scroll = gtk::ScrolledWindow::builder().vexpand(true).child(&info_text).build();
    info_panel.append(&info_scroll);

    let state: State = Rc::new(AppState {
        window: window.clone(),
        library: RefCell::new(library),
        settings: RefCell::new(settings),
        store,
        sort_model,
        selection,
        filter,
        column_view,
        columns: RefCell::new(Vec::new()),
        sidebar: gtk::ListBox::new(),
        sources: RefCell::new(Vec::new()),
        source: RefCell::new(Source::All),
        search_text: RefCell::new(String::new()),
        status,
        progress,
        cancel_button,
        busy: Cell::new(false),
        quitting: Cell::new(false),
        columns_save_pending: Cell::new(false),
        save_pending: Cell::new(false),
        info_panel,
        info_cover,
        info_text,
        info_generation: Cell::new(0),
        journal: journal::Journal::new(),
        inline: inline_edit::Inline::default(),
    });

    // Filter: sidebar source + quick search.
    let weak = Rc::downgrade(&state);
    state.filter.set_filter_func(move |obj| {
        let Some(st) = weak.upgrade() else { return true };
        let b = obj.downcast_ref::<BookObject>().unwrap().book();
        let source_ok = match &*st.source.borrow() {
            Source::All => true,
            Source::Modified => b.info_dirty,
            Source::Folder(f) => b.file_path.starts_with(f),
        };
        source_ok && matches_search(&b, &st.search_text.borrow())
    });

    inline_edit::install_css();
    theme::apply(state.settings.borrow().theme);
    build_columns(&state);
    watch_columns(&state);
    fill_store(&state);
    build_actions(app, &state);
    let content = build_layout(&state);
    window.set_child(Some(&content));
    refresh_sidebar(&state);
    update_status(&state);

    window.connect_close_request(glib::clone!(
        #[weak]
        state,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |w| {
            if state.busy.get() && !state.quitting.get() {
                let st = state.clone();
                widgets::confirm(
                    w,
                    t!("An operation is in progress"),
                    t!("Quit anyway? The current operation will be interrupted; what has already been done (for example the books already scanned) is kept."),
                    t!("Quit"),
                    move || {
                        st.quitting.set(true);
                        st.cancel_button.emit_clicked();
                        st.journal.title(t!("Quit requested during an operation"));
                        st.window.close();
                    },
                );
                return glib::Propagation::Stop;
            }
            save_columns(&state);
            {
                let mut s = state.settings.borrow_mut();
                s.window_width = w.default_width();
                s.window_height = w.default_height();
            }
            let _ = state.settings.borrow().save();
            state.flush_library();
            glib::Propagation::Proceed
        }
    ));

    // The window owns the application state (every callback holds a weak reference).
    // SAFETY: the key is only used here, with this type.
    unsafe { window.set_data("app-state", state.clone()) };
    window.present();

    #[cfg(debug_assertions)]
    debug_screenshots(&state);

    if state.settings.borrow().scan_on_startup && !state.settings.borrow().library_folders.is_empty() {
        scan(&state);
    } else if state.settings.borrow().library_folders.is_empty() && state.library.borrow().books.is_empty() {
        state.set_status(t!("Welcome! Add folders to scan in the settings (Ctrl+,), then start a scan (F5)."));
    }
}

fn matches_search(b: &Book, text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let hay = [
        Field::Series,
        Field::Title,
        Field::Number,
        Field::AlternateSeries,
        Field::Writer,
        Field::Penciller,
        Field::Publisher,
        Field::Imprint,
        Field::Genre,
        Field::Tags,
        Field::Characters,
        Field::StoryArc,
        Field::FileName,
        Field::Year,
    ]
    .iter()
    .map(|f| f.get(b))
    .collect::<Vec<_>>()
    .join("\u{1}")
    .to_lowercase();
    text.to_lowercase().split_whitespace().all(|w| hay.contains(w))
}

fn fill_store(state: &State) {
    let objs: Vec<BookObject> = state
        .library
        .borrow()
        .books
        .iter()
        .cloned()
        .map(BookObject::new)
        .collect();
    state.store.remove_all();
    state.store.extend_from_slice(&objs);
}

fn build_columns(state: &State) {
    let cols = state.settings.borrow().columns.clone();
    for c in cols {
        let col = inline_edit::make_column(state, c.field);
        col.set_visible(c.visible);
        col.set_fixed_width(c.width);
        state.column_view.append_column(&col);
        state.columns.borrow_mut().push((c.field, col));
    }
    let sort = state.settings.borrow().sort_field;
    if let Some(f) = sort
        && let Some((_, col)) = state.columns.borrow().iter().find(|(x, _)| *x == f)
    {
        let dir = if state.settings.borrow().sort_descending {
            gtk::SortType::Descending
        } else {
            gtk::SortType::Ascending
        };
        state.column_view.sort_by_column(Some(col), dir);
    }
}

/// Saves the column layout as soon as the user changes it: order (drag and
/// drop of the headers), width, visibility and sort.
fn watch_columns(state: &State) {
    state.column_view.set_reorderable(true);
    let weak = Rc::downgrade(state);
    let schedule = move || {
        if let Some(st) = weak.upgrade() {
            schedule_columns_save(&st);
        }
    };
    let s = schedule.clone();
    state.column_view.columns().connect_items_changed(move |_, _, _, _| s());
    for (_, col) in state.columns.borrow().iter() {
        let s = schedule.clone();
        col.connect_fixed_width_notify(move |_| s());
        let s = schedule.clone();
        col.connect_visible_notify(move |_| s());
    }
    if let Some(sorter) = state.column_view.sorter() {
        sorter.connect_changed(move |_, _| schedule());
    }
}

/// Saves the columns once, shortly after a burst of changes (resizing a
/// column sends many notifications).
fn schedule_columns_save(state: &State) {
    if state.columns_save_pending.replace(true) {
        return;
    }
    let weak = Rc::downgrade(state);
    glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
        let Some(st) = weak.upgrade() else { return };
        st.columns_save_pending.set(false);
        save_columns(&st);
        if let Err(e) = st.settings.borrow().save() {
            st.journal.error(&t!("Saving the columns: {0}", format!("{e:#}")));
        }
    });
}

/// Stores the current column order, widths and sort into the settings.
fn save_columns(state: &State) {
    let cols = state.column_view.columns();
    let mut out = Vec::new();
    for i in 0..cols.n_items() {
        let col = cols.item(i).and_downcast::<gtk::ColumnViewColumn>().unwrap();
        let Some(field) = col.id().and_then(|id| Field::from_key(&id)) else { continue };
        let width = col.fixed_width();
        out.push(crate::settings::ColumnSetting { field, visible: col.is_visible(), width });
    }
    let mut s = state.settings.borrow_mut();
    s.columns = out;
    let sorter = state.column_view.sorter().and_downcast::<gtk::ColumnViewSorter>();
    s.sort_field = sorter
        .as_ref()
        .and_then(|s| s.primary_sort_column())
        .and_then(|c| c.id())
        .and_then(|id| Field::from_key(&id));
    s.sort_descending = sorter.map(|s| s.primary_sort_order() == gtk::SortType::Descending).unwrap_or(false);
}

fn build_layout(state: &State) -> gtk::Box {
    let header = gtk::HeaderBar::new();
    let scan_btn = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(t!("Scan the library folders (F5)"))
        .action_name("win.scan")
        .build();
    let edit_btn = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(t!("Edit Info (Ctrl+I)"))
        .action_name("win.edit")
        .build();
    let write_btn = gtk::Button::builder()
        .icon_name("document-save-symbolic")
        .tooltip_text(t!("Update Book Files (write ComicInfo.xml)"))
        .action_name("win.update-files")
        .build();
    let convert_btn = gtk::MenuButton::builder()
        .child(&icons::icon(icons::Icon::Convert))
        .tooltip_text(t!("Convert"))
        .menu_model(&convert_menu())
        .build();
    let scrape_btn = gtk::Button::builder()
        .child(&icons::icon(icons::Icon::Scraper))
        .tooltip_text(t!("Fetch info from Bedetheque (Ctrl+B)"))
        .action_name("win.bd-scrape")
        .build();
    // Organize button with an arrow listing the profiles (as the
    // ComicRack toolbar button of Library Organizer).
    let organize_main = gtk::Button::builder()
        .child(&icons::icon(icons::Icon::Organizer))
        .tooltip_text(t!("Organize files (Ctrl+L)"))
        .action_name("win.organize")
        .build();
    let organize_arrow = gtk::MenuButton::builder()
        .icon_name("pan-down-symbolic")
        .tooltip_text(t!("Organize with a profile"))
        .css_classes(["split-arrow"])
        .build();
    let weak = Rc::downgrade(state);
    organize_arrow.set_create_popup_func(move |b| {
        let Some(st) = weak.upgrade() else { return };
        let menu = gio::Menu::new();
        let profiles = gio::Menu::new();
        for p in &st.settings.borrow().organizer.profiles {
            let item = gio::MenuItem::new(Some(&p.name), None);
            item.set_action_and_target_value(Some("win.organize-profile"), Some(&p.name.to_variant()));
            profiles.append_item(&item);
        }
        menu.append_section(Some(t!("Organize with the profile")), &profiles);
        menu.append_section(None, &organize_section());
        b.set_menu_model(Some(&menu));
    });
    let organize_btn = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    organize_btn.add_css_class("linked");
    organize_btn.append(&organize_main);
    organize_btn.append(&organize_arrow);
    header.pack_start(&scan_btn);
    header.pack_start(&edit_btn);
    header.pack_start(&write_btn);
    header.pack_start(&convert_btn);
    header.pack_start(&scrape_btn);
    header.pack_start(&organize_btn);

    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&main_menu())
        .tooltip_text(t!("Menu"))
        .build();
    header.pack_end(&menu_btn);
    let panel_btn = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-right-symbolic")
        .tooltip_text(t!("Info panel"))
        .active(state.settings.borrow().show_info_panel)
        .build();
    header.pack_end(&panel_btn);

    let search = gtk::SearchEntry::builder()
        .placeholder_text(t!("Search (series, title, creators, tags…)"))
        .width_request(380)
        .build();
    header.set_title_widget(Some(&search));
    search.connect_search_changed(glib::clone!(
        #[weak]
        state,
        move |e| {
            state.search_text.replace(e.text().trim().to_string());
            state.filter.changed(gtk::FilterChange::Different);
            update_status(&state);
        }
    ));
    let search_key = gtk::ShortcutController::new();
    search_key.set_scope(gtk::ShortcutScope::Global);
    search_key.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>f"),
        Some(gtk::CallbackAction::new(glib::clone!(
            #[weak]
            search,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _| {
                search.grab_focus();
                glib::Propagation::Stop
            }
        ))),
    ));
    state.window.add_controller(search_key);
    state.window.set_titlebar(Some(&header));

    // Sidebar
    state.sidebar.add_css_class("navigation-sidebar");
    state.sidebar.connect_row_selected(glib::clone!(
        #[weak]
        state,
        move |_, row| {
            let Some(row) = row else { return };
            let src = state.sources.borrow().get(row.index() as usize).cloned();
            if let Some(src) = src
                && *state.source.borrow() != src
            {
                state.source.replace(src);
                state.filter.changed(gtk::FilterChange::Different);
                update_status(&state);
            }
        }
    ));
    let sidebar_scroll = gtk::ScrolledWindow::builder()
        .child(&state.sidebar)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .width_request(200)
        .build();

    // Book list
    let list_scroll = gtk::ScrolledWindow::builder()
        .child(&state.column_view)
        .hexpand(true)
        .vexpand(true)
        .build();
    state.column_view.connect_activate(glib::clone!(
        #[weak]
        state,
        move |_, _| edit_dialog::open(&state)
    ));
    state.selection.connect_selection_changed(glib::clone!(
        #[weak]
        state,
        move |_, _, _| {
            update_info_panel(&state);
            update_status(&state);
        }
    ));
    // Context menu
    let popover = gtk::PopoverMenu::from_model(Some(&context_menu()));
    popover.set_parent(&state.column_view);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    let click = gtk::GestureClick::builder().button(gdk::BUTTON_SECONDARY).build();
    click.connect_pressed(glib::clone!(
        #[weak]
        popover,
        move |_, _, x, y| {
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        }
    ));
    state.column_view.add_controller(click);

    // Info panel
    state.info_panel.set_visible(state.settings.borrow().show_info_panel);
    panel_btn.connect_toggled(glib::clone!(
        #[weak]
        state,
        move |b| {
            state.info_panel.set_visible(b.is_active());
            state.settings.borrow_mut().show_info_panel = b.is_active();
            update_info_panel(&state);
        }
    ));

    let inner = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&list_scroll)
        .end_child(&state.info_panel)
        .resize_end_child(false)
        .shrink_end_child(false)
        .build();
    let outer = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&sidebar_scroll)
        .end_child(&inner)
        .resize_start_child(false)
        .shrink_start_child(false)
        .build();
    outer.set_vexpand(true);

    // Status bar
    let status_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    status_bar.append(&state.status);
    status_bar.append(&state.progress);
    status_bar.append(&state.cancel_button);
    status_bar.append(&state.journal.button);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&outer);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&status_bar);
    root
}

fn convert_menu() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some(t!("Convert to CBZ")), Some("win.convert::cbz"));
    m.append(Some(t!("Convert to CB7")), Some("win.convert::cb7"));
    m.append(Some(t!("Convert to CBR")), Some("win.convert::cbr"));
    m.append(Some(t!("Export Books…")), Some("win.export"));
    m.append(Some(t!("Split Packs into Albums…")), Some("win.split-packs"));
    m
}

fn books_section() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some(t!("Edit Info…")), Some("win.edit"));
    m.append(Some(t!("Automatic Numbering…")), Some("win.autonumber"));
    m.append(Some(t!("Update Book Files")), Some("win.update-files"));
    m.append(Some(t!("Refresh")), Some("win.refresh"));
    m.append(Some(t!("Refresh (force reread)")), Some("win.refresh-force"));
    m
}

/// Fetching the book info from bedetheque.com.
fn bedetheque_section() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some(t!("Fetch Info from Bedetheque")), Some("win.bd-scrape"));
    m.append(Some(t!("Fetch Info from a Bedetheque Link…")), Some("win.bd-quick"));
    m
}

/// Moving / renaming the book files according to templates.
fn organize_section() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some(t!("Organize Files…")), Some("win.organize"));
    m.append(Some(t!("Organize Files Again")), Some("win.organize-quick"));
    m.append(Some(t!("Undo Last Organization")), Some("win.organize-undo"));
    m
}

fn file_section() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some(t!("Open Book")), Some("win.open-file"));
    m.append(Some(t!("Open Containing Folder")), Some("win.open-folder"));
    m.append(Some(t!("Remove from Library")), Some("win.remove"));
    m
}

fn context_menu() -> gio::Menu {
    let m = gio::Menu::new();
    m.append_section(None, &books_section());
    m.append_submenu(Some(t!("Convert")), &convert_menu());
    m.append_section(None, &bedetheque_section());
    m.append_section(None, &organize_section());
    m.append_section(None, &file_section());
    m
}

fn main_menu() -> gio::Menu {
    let m = gio::Menu::new();
    let lib = gio::Menu::new();
    lib.append(Some(t!("Scan Folders")), Some("win.scan"));
    lib.append(Some(t!("Select All")), Some("win.select-all"));
    lib.append(Some(t!("Columns…")), Some("win.columns"));
    lib.append(Some(t!("Message Log")), Some("win.journal"));
    lib.append(Some(t!("Import ComicRack Settings…")), Some("win.import-comicrack"));
    lib.append(Some(t!("Settings…")), Some("win.settings"));
    m.append_section(None, &lib);
    m.append_section(None, &books_section());
    m.append_submenu(Some(t!("Convert")), &convert_menu());
    m.append_section(None, &bedetheque_section());
    m.append_section(None, &organize_section());
    m.append_section(None, &file_section());
    let about = gio::Menu::new();
    about.append(Some(t!("About")), Some("win.about"));
    m.append_section(None, &about);
    m
}

fn build_actions(app: &gtk::Application, state: &State) {
    let win = &state.window;
    let add = |name: &str, f: fn(&State)| {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(state);
        action.connect_activate(move |_, _| {
            if let Some(st) = weak.upgrade() {
                f(&st);
            }
        });
        win.add_action(&action);
    };
    add("scan", scan);
    add("settings", settings_dialog::open);
    add("columns", columns_dialog::open);
    add("edit", edit_dialog::open);
    add("autonumber", autonumber_dialog::open);
    add("update-files", update_files);
    add("refresh", |st| refresh(st, false));
    add("refresh-force", |st| refresh(st, true));
    add("export", convert_dialog::open);
    add("split-packs", split_dialog::open);
    add("bd-scrape", scraper_ui::scrape);
    add("bd-quick", scraper_ui::quick_scrape);
    add("organize", organizer_ui::open);
    add("organize-quick", organizer_ui::quick);
    add("organize-undo", organizer_ui::undo);
    let organize_profile = gio::SimpleAction::new("organize-profile", Some(glib::VariantTy::STRING));
    let weak = Rc::downgrade(state);
    organize_profile.connect_activate(move |_, p| {
        if let (Some(st), Some(name)) = (weak.upgrade(), p.and_then(|v| v.str().map(str::to_string))) {
            organizer_ui::run_profile(&st, &name);
        }
    });
    win.add_action(&organize_profile);
    add("open-folder", open_folder);
    add("open-file", open_file);
    add("remove", remove_selected);
    add("select-all", |st| {
        st.selection.select_all();
    });
    add("about", about);
    add("inline-edit", inline_edit::edit_selection);
    add("journal", |st| st.journal.open(&st.window));
    add("import-comicrack", import_dialog::open);
    let errors_only = gio::SimpleAction::new("journal-errors-only", Some(glib::VariantTy::BOOLEAN));
    let weak = Rc::downgrade(state);
    errors_only.connect_activate(move |_, p| {
        if let (Some(st), Some(on)) = (weak.upgrade(), p.and_then(|v| v.get::<bool>())) {
            st.journal.set_errors_only(on);
        }
    });
    win.add_action(&errors_only);

    let convert = gio::SimpleAction::new("convert", Some(glib::VariantTy::STRING));
    let weak = Rc::downgrade(state);
    convert.connect_activate(move |_, p| {
        let (Some(st), Some(fmt)) = (weak.upgrade(), p.and_then(|v| v.str().map(str::to_string))) else {
            return;
        };
        let preset = crate::convert::ExportSettings::presets()
            .into_iter()
            .find(|p| p.format.map(|f| f.extension()) == Some(fmt.as_str()));
        if let Some(p) = preset {
            convert_dialog::run_export(&st, p);
        }
    });
    win.add_action(&convert);

    app.set_accels_for_action("win.scan", &["F5"]);
    app.set_accels_for_action("win.inline-edit", &["F2"]);
    app.set_accels_for_action("win.journal", &["<Control>j"]);
    app.set_accels_for_action("win.edit", &["<Control>i"]);
    app.set_accels_for_action("win.settings", &["<Control>comma"]);
    app.set_accels_for_action("win.update-files", &["<Control>s"]);
    app.set_accels_for_action("win.autonumber", &["<Control><Shift>n"]);
    app.set_accels_for_action("win.bd-scrape", &["<Control>b"]);
    app.set_accels_for_action("win.organize", &["<Control>l"]);
    app.set_accels_for_action("win.remove", &["Delete"]);
}

pub fn refresh_sidebar(state: &State) {
    let lib = state.library.borrow();
    let mut sources = vec![Source::All, Source::Modified];
    let mut labels = vec![
        t!("Library ({0})", lib.books.len()),
        t!("Unsaved info ({0})", lib.books.iter().filter(|b| b.info_dirty).count()),
    ];
    for f in &state.settings.borrow().library_folders {
        let n = lib.books.iter().filter(|b| b.file_path.starts_with(f)).count();
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.display().to_string());
        labels.push(format!("📁 {name} ({n})"));
        sources.push(Source::Folder(f.clone()));
    }
    drop(lib);
    let current = state.source.borrow().clone();
    while let Some(row) = state.sidebar.row_at_index(0) {
        state.sidebar.remove(&row);
    }
    let mut selected = 0;
    for (i, (label, src)) in labels.iter().zip(&sources).enumerate() {
        let l = gtk::Label::builder().label(label).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        if let Source::Folder(p) = src {
            l.set_tooltip_text(Some(&p.display().to_string()));
        }
        state.sidebar.append(&l);
        if *src == current {
            selected = i;
        }
    }
    state.sources.replace(sources);
    if let Some(row) = state.sidebar.row_at_index(selected as i32) {
        state.sidebar.select_row(Some(&row));
    }
}

fn update_status(state: &State) {
    if state.busy.get() {
        return;
    }
    let shown = state.sort_model.n_items();
    let total = state.store.n_items();
    let sel = state.selection.selection().size();
    let s = if sel > 0 {
        t!("{0} book(s) shown of {1} — {2} selected", shown, total, sel)
    } else {
        t!("{0} book(s) shown of {1}", shown, total)
    };
    state.status.set_text(&s);
}

fn update_info_panel(state: &State) {
    if !state.info_panel.is_visible() {
        return;
    }
    let books = state.selected_books();
    let generation = state.info_generation.get() + 1;
    state.info_generation.set(generation);
    let Some(book) = books.first().cloned() else {
        state.info_cover.set_paintable(None::<&gdk::Paintable>);
        state.info_text.set_text("");
        return;
    };
    let esc = |s: &str| glib::markup_escape_text(s).to_string();
    let mut text = format!("<b>{}</b>\n", esc(&book.caption()));
    if books.len() > 1 {
        text.push_str(&format!("<i>{}</i>\n", t!("{0} books selected", books.len())));
    }
    text.push('\n');
    for f in [
        Field::Writer,
        Field::Penciller,
        Field::Colorist,
        Field::Publisher,
        Field::Imprint,
        Field::Genre,
        Field::Released,
        Field::PageCount,
        Field::LanguageISO,
        Field::Rating,
        Field::CommunityRating,
        Field::FileFormat,
        Field::FileSize,
        Field::FilePath,
    ] {
        let v = f.display(&book);
        if !v.is_empty() {
            text.push_str(&t!("<b>{0}</b>: {1}", f.label(), esc(&v)));
            text.push('\n');
        }
    }
    if !book.info.summary.is_empty() {
        text.push_str(&format!("\n{}", esc(&book.info.summary)));
    }
    state.info_text.set_markup(&text);
    state.info_cover.set_paintable(None::<&gdk::Paintable>);

    let (tx, rx) = async_channel::bounded::<Option<Vec<u8>>>(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(library::thumbnail(&book, 400).ok());
    });
    let weak = Rc::downgrade(state);
    glib::spawn_future_local(async move {
        let Ok(Some(data)) = rx.recv().await else { return };
        let Some(st) = weak.upgrade() else { return };
        if st.info_generation.get() != generation {
            return;
        }
        if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(data)) {
            st.info_cover.set_paintable(Some(&tex));
        }
    });
}

impl AppState {
    pub fn set_status(&self, s: &str) {
        self.status.set_text(s);
    }

    /// Saves the library a moment later: successive changes (a scan, a
    /// scraping session...) are written once.
    pub fn save_library(self: &Rc<Self>) {
        if self.save_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            if let Some(st) = weak.upgrade() {
                st.save_pending.set(false);
                st.flush_library();
            }
        });
    }

    /// Writes the books changed since the last save (nothing when none).
    pub fn flush_library(&self) {
        if let Err(e) = self.library.borrow_mut().save() {
            self.journal.error(&format!("{e:#}"));
            widgets::alert(&self.window, t!("Error"), &format!("{e:#}"));
        }
    }

    /// Selected books, in display order.
    pub fn selected_books(&self) -> Vec<Book> {
        let mut out = Vec::new();
        let sel = self.selection.selection();
        if sel.is_empty() {
            return out;
        }
        for i in 0..self.sort_model.n_items() {
            if sel.contains(i)
                && let Some(o) = self.sort_model.item(i).and_downcast::<BookObject>()
            {
                out.push(o.book().clone());
            }
        }
        out
    }

    /// Replaces books (matched by id) in the library and the list.
    /// `dirty` marks their metadata as not yet written into the files.
    pub fn update_books(self: &Rc<Self>, books: Vec<Book>, dirty: bool) {
        if books.is_empty() {
            return;
        }
        let objects: std::collections::HashMap<u64, BookObject> = (0..self.store.n_items())
            .filter_map(|i| self.store.item(i).and_downcast::<BookObject>())
            .map(|o| (o.id(), o))
            .collect();
        let mut written = Vec::new();
        {
            let mut lib = self.library.borrow_mut();
            for mut b in books {
                if dirty {
                    b.info_dirty = true;
                }
                if let Some(slot) = lib.get_mut(b.id) {
                    *slot = b.clone();
                }
                if let Some(o) = objects.get(&b.id) {
                    o.set_book(b.clone());
                }
                written.push(b);
            }
        }
        self.save_library();
        self.resort();
        refresh_sidebar(self);
        update_info_panel(self);
        // During a job, the job itself writes the files at the end.
        if dirty
            && !self.busy.get()
            && self.settings.borrow().auto_update_files
            && self.settings.borrow().allow_write_info
        {
            write_files(self, written);
        }
    }

    pub fn add_books(self: &Rc<Self>, books: Vec<Book>) {
        let mut objs = Vec::new();
        {
            let mut lib = self.library.borrow_mut();
            for b in books {
                lib.add(b);
                objs.push(BookObject::new(lib.books.last().unwrap().clone()));
            }
        }
        self.store.extend_from_slice(&objs);
    }

    /// Adds/updates the books found by a scan (called during the scan).
    fn apply_scanned(self: &Rc<Self>, added: Vec<Book>, updated: Vec<Book>) {
        self.add_books(added);
        if !updated.is_empty() {
            let mut lib = self.library.borrow_mut();
            for b in &updated {
                if let Some(slot) = lib.get_mut(b.id) {
                    *slot = b.clone();
                }
            }
            drop(lib);
            let objects: std::collections::HashMap<u64, BookObject> = (0..self.store.n_items())
                .filter_map(|i| self.store.item(i).and_downcast::<BookObject>())
                .map(|o| (o.id(), o))
                .collect();
            for b in updated {
                if let Some(o) = objects.get(&b.id) {
                    o.set_book(b);
                }
            }
        }
        refresh_sidebar(self);
    }

    pub fn remove_books(self: &Rc<Self>, ids: &[u64]) {
        if ids.is_empty() {
            return;
        }
        self.library.borrow_mut().remove(ids);
        let mut i = 0;
        while i < self.store.n_items() {
            let o = self.store.item(i).and_downcast::<BookObject>().unwrap();
            if ids.contains(&o.id()) {
                self.store.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn resort(&self) {
        if let Some(s) = self.sort_model.sorter() {
            s.changed(gtk::SorterChange::Different);
        }
        self.filter.changed(gtk::FilterChange::Different);
    }

    pub fn require_selection(&self) -> Option<Vec<Book>> {
        let books = self.selected_books();
        if books.is_empty() {
            widgets::alert(&self.window, t!("No book selected"), t!("Select at least one book in the list."));
            None
        } else {
            Some(books)
        }
    }
}

// ---------------------------------------------------------------------------
// Background jobs
// ---------------------------------------------------------------------------

type UiFn = Box<dyn FnOnce(&State) + Send>;

enum JobMsg {
    Progress(f64, String),
    /// Progress of unknown length.
    Pulse(String),
    Log(String),
    Error(String),
    Ui(UiFn),
    Finished,
}

/// Handle given to the worker thread of a job.
#[derive(Clone)]
pub struct JobCtx {
    tx: async_channel::Sender<JobMsg>,
    cancel: Arc<AtomicBool>,
}

impl JobCtx {
    pub fn progress(&self, done: usize, total: usize, text: impl Into<String>) {
        let frac = if total == 0 { 0.0 } else { done as f64 / total as f64 };
        let _ = self.tx.send_blocking(JobMsg::Progress(frac, text.into()));
    }

    /// Activity of unknown length (the progress bar bounces).
    pub fn pulse(&self, text: impl Into<String>) {
        let _ = self.tx.send_blocking(JobMsg::Pulse(text.into()));
    }

    /// Adds a line to the message journal.
    pub fn log(&self, msg: impl Into<String>) {
        let _ = self.tx.send_blocking(JobMsg::Log(msg.into()));
    }

    /// Adds an error to the message journal.
    pub fn error(&self, msg: impl Into<String>) {
        let _ = self.tx.send_blocking(JobMsg::Error(msg.into()));
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Runs `f` on the GTK thread.
    pub fn ui(&self, f: impl FnOnce(&State) + Send + 'static) {
        let _ = self.tx.send_blocking(JobMsg::Ui(Box::new(f)));
    }

    /// Asks something on the GTK thread and blocks until `reply` is called.
    pub fn ask<R: Send + 'static>(
        &self,
        f: impl FnOnce(&State, Box<dyn FnOnce(R) + Send>) + Send + 'static,
    ) -> Option<R> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.ui(move |st| {
            f(
                st,
                Box::new(move |r| {
                    let _ = tx.send(r);
                }),
            )
        });
        rx.recv().ok()
    }

    /// Sleeps in small steps, returning early when cancelled.
    pub fn sleep(&self, secs: u32) {
        for _ in 0..secs * 10 {
            if self.cancelled() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Runs `work` in a thread with a progress bar; `done` gets its result on the GTK thread.
pub fn run_job<T: Send + 'static>(
    state: &State,
    title: &str,
    work: impl FnOnce(JobCtx) -> T + Send + 'static,
    done: impl FnOnce(&State, T) + 'static,
) {
    if state.busy.replace(true) {
        widgets::alert(&state.window, t!("Operation in progress"), t!("Wait for the current operation to finish."));
        return;
    }
    let (tx, rx) = async_channel::unbounded::<JobMsg>();
    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = JobCtx { tx: tx.clone(), cancel: cancel.clone() };
    let result: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
    let slot = result.clone();

    state.status.set_text(title);
    state.journal.title(&format!("▶ {}", title.trim_end_matches('…')));
    state.journal.set_active(true);
    let started = std::time::Instant::now();
    state.progress.set_fraction(0.0);
    state.progress.set_visible(true);
    state.cancel_button.set_visible(true);
    state.cancel_button.set_sensitive(true);
    let cancel_flag = cancel.clone();
    let handler = state.cancel_button.connect_clicked(move |b| {
        cancel.store(true, Ordering::Relaxed);
        b.set_sensitive(false);
    });

    std::thread::spawn(move || {
        let r = work(ctx);
        *slot.lock().unwrap() = Some(r);
        let _ = tx.send_blocking(JobMsg::Finished);
    });

    let weak = Rc::downgrade(state);
    let title = title.to_string();
    glib::spawn_future_local(async move {
        let mut done = Some(done);
        let mut handler = Some(handler);
        while let Ok(msg) = rx.recv().await {
            let Some(st) = weak.upgrade() else { return };
            match msg {
                JobMsg::Progress(f, text) => {
                    st.progress.set_fraction(f);
                    st.status.set_text(&format!("{title} {text}"));
                }
                JobMsg::Pulse(text) => {
                    st.progress.pulse();
                    st.status.set_text(&format!("{title} {text}"));
                }
                JobMsg::Log(msg) => st.journal.info(&msg),
                JobMsg::Error(msg) => st.journal.error(&msg),
                JobMsg::Ui(f) => f(&st),
                JobMsg::Finished => {
                    let name = title.trim_end_matches('…');
                    let secs = started.elapsed().as_secs_f64();
                    if cancel_flag.load(Ordering::Relaxed) {
                        st.journal.title(&t!("■ {0}: cancelled after {1} s", name, format!("{secs:.1}")));
                    } else {
                        st.journal.title(&t!("■ {0}: finished in {1} s", name, format!("{secs:.1}")));
                    }
                    st.journal.set_active(false);
                    st.busy.set(false);
                    st.progress.set_visible(false);
                    st.cancel_button.set_visible(false);
                    if let Some(h) = handler.take() {
                        st.cancel_button.disconnect(h);
                    }
                    update_status(&st);
                    let r = result.lock().unwrap().take();
                    if let (Some(r), Some(done)) = (r, done.take()) {
                        done(&st, r);
                    }
                    return;
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

pub fn scan(state: &State) {
    let folders = state.settings.borrow().library_folders.clone();
    if folders.is_empty() {
        widgets::alert(
            &state.window,
            t!("No folder"),
            t!("Add the folders containing your books in the settings."),
        );
        settings_dialog::open(state);
        return;
    }
    let known = state.library.borrow().books.clone();
    let threads = state.settings.borrow().scan_threads.max(1);
    for f in &folders {
        state.journal.info(&t!("Folder to scan: {0}", f.display()));
    }
    run_job(
        state,
        t!("Scanning library…"),
        move |ctx| {
            let mut stats = ScanStats::default();
            // New and changed books are sent to the list in batches, so they
            // appear while the scan goes on.
            let mut added = Vec::new();
            let mut updated = Vec::new();
            let mut last_flush = std::time::Instant::now();
            let flush = |added: &mut Vec<Book>, updated: &mut Vec<Book>| {
                if added.is_empty() && updated.is_empty() {
                    return;
                }
                let a = std::mem::take(added);
                let u = std::mem::take(updated);
                ctx.ui(move |st| st.apply_scanned(a, u));
            };
            let c = ctx.clone();
            library::scan(folders, known, threads, || c.cancelled(), |ev| {
                match ev {
                    ScanEvent::Discovering { found, folder } => {
                        c.pulse(t!("looking for files: {0} found — {1}", found, folder));
                    }
                    ScanEvent::Progress { done, total, current } => {
                        if done == 0 {
                            c.log(t!("{0}; {1} new or changed to read ({2} parallel reads)", current, total, threads));
                        }
                        c.progress(done, total, format!("{done}/{total} {current}"));
                    }
                    ScanEvent::Added(b) => {
                        c.log(t!("Added: {0} ({1} pages)", b.file_path.display(), b.info.page_count));
                        stats.added += 1;
                        added.push(b);
                    }
                    ScanEvent::Updated(b) => {
                        c.log(t!("Updated: {0}", b.file_path.display()));
                        stats.updated += 1;
                        updated.push(b);
                    }
                    ScanEvent::Error(e) => {
                        c.error(e);
                        stats.errors += 1;
                    }
                    ScanEvent::Finished { missing, cancelled } => {
                        stats.missing = missing;
                        stats.cancelled = cancelled;
                    }
                }
                if last_flush.elapsed().as_millis() >= 400 || added.len() >= 500 {
                    flush(&mut added, &mut updated);
                    last_flush = std::time::Instant::now();
                }
            });
            flush(&mut added, &mut updated);
            stats
        },
        |st, stats| {
            let nm = stats.missing.len();
            if st.settings.borrow().remove_missing && !stats.missing.is_empty() {
                for id in &stats.missing {
                    if let Some(b) = st.library.borrow().get(*id) {
                        st.journal.info(&t!("Removed (file not found): {0}", b.file_path.display()));
                    }
                }
                st.remove_books(&stats.missing);
            }
            st.save_library();
            st.resort();
            refresh_sidebar(st);
            update_status(st);
            let mut msg = if stats.cancelled {
                t!("Scan cancelled: {0} new, {1} updated, {2} missing", stats.added, stats.updated, nm)
            } else {
                t!("Scan finished: {0} new, {1} updated, {2} missing", stats.added, stats.updated, nm)
            };
            if stats.errors > 0 {
                msg.push_str(&t!(", {0} error(s) — see the log (Ctrl+J)", stats.errors));
            }
            st.journal.info(&msg);
            st.set_status(&msg);
        },
    );
}

/// Status message, with a pointer to the journal when there were errors.
pub fn with_errors(mut msg: String, errors: usize) -> String {
    if errors > 0 {
        msg.push_str(&t!(" — {0} error(s), see the log (Ctrl+J)", errors));
    }
    msg
}

#[derive(Default)]
struct ScanStats {
    added: usize,
    updated: usize,
    errors: usize,
    missing: Vec<u64>,
    cancelled: bool,
}

/// "Update Book Files": writes ComicInfo.xml into the selected files.
fn update_files(state: &State) {
    if !state.settings.borrow().allow_write_info {
        widgets::alert(
            &state.window,
            t!("Writing disabled"),
            t!("Writing information into the files is disabled in the settings."),
        );
        return;
    }
    let Some(books) = state.require_selection() else { return };
    write_files(state, books);
}

pub fn write_files(state: &State, books: Vec<Book>) {
    run_job(
        state,
        t!("Updating book files…"),
        move |ctx| {
            let total = books.len();
            let mut ok = Vec::new();
            let mut errors = Vec::new();
            for (i, mut b) in books.into_iter().enumerate() {
                if ctx.cancelled() {
                    break;
                }
                ctx.progress(i, total, b.file_name());
                if !archive::BookFormat::detect(&b.file_path).is_some_and(|f| f.can_write_info()) {
                    errors.push(t!(
                        "{0}: format cannot be modified (the information stays in the library)",
                        b.file_path.display()
                    ));
                    continue;
                }
                match archive::write_comic_info(&b.file_path, &b.info) {
                    Ok(()) => {
                        b.info_dirty = false;
                        b.refresh_file_stats();
                        ok.push(b);
                    }
                    Err(e) => errors.push(t!("{0}: {1}", b.file_path.display(), format!("{e:#}"))),
                }
            }
            (ok, errors)
        },
        |st, (ok, errors)| {
            let n = ok.len();
            for b in &ok {
                st.journal.info(&t!("ComicInfo.xml written: {0}", b.file_path.display()));
            }
            for e in &errors {
                st.journal.error(e);
            }
            st.update_books(ok, false);
            st.set_status(&with_errors(t!("{0} file(s) updated", n), errors.len()));
        },
    );
}

fn refresh(state: &State, force: bool) {
    let Some(books) = state.require_selection() else { return };
    run_job(
        state,
        t!("Rereading files…"),
        move |ctx| {
            let total = books.len();
            let mut ok = Vec::new();
            let mut errors = Vec::new();
            for (i, mut b) in books.into_iter().enumerate() {
                if ctx.cancelled() {
                    break;
                }
                ctx.progress(i, total, b.file_name());
                if b.info_dirty && !force {
                    continue;
                }
                match library::refresh_from_file(&mut b, force) {
                    Ok(()) => {
                        b.info_dirty = false;
                        ok.push(b);
                    }
                    Err(e) => errors.push(t!("{0}: {1}", b.file_path.display(), format!("{e:#}"))),
                }
            }
            (ok, errors)
        },
        |st, (ok, errors)| {
            let n = ok.len();
            for b in &ok {
                st.journal.info(&t!("Reread: {0}", b.file_path.display()));
            }
            for e in &errors {
                st.journal.error(e);
            }
            st.update_books(ok, false);
            st.set_status(&with_errors(t!("{0} book(s) refreshed", n), errors.len()));
        },
    );
}

fn open_folder(state: &State) {
    let Some(books) = state.require_selection() else { return };
    if let Some(dir) = books[0].file_path.parent() {
        let file = gio::File::for_path(dir);
        gtk::FileLauncher::new(Some(&file)).launch(Some(&state.window), gio::Cancellable::NONE, |_| {});
    }
}

fn open_file(state: &State) {
    let Some(books) = state.require_selection() else { return };
    let file = gio::File::for_path(&books[0].file_path);
    gtk::FileLauncher::new(Some(&file)).launch(Some(&state.window), gio::Cancellable::NONE, |_| {});
}

fn remove_selected(state: &State) {
    let Some(books) = state.require_selection() else { return };
    let ids: Vec<u64> = books.iter().map(|b| b.id).collect();
    let weak = Rc::downgrade(state);
    widgets::confirm(
        &state.window,
        t!("Remove from Library"),
        &t!(
            "Remove {0} book(s) from the library? The files are not deleted (they will reappear at the next scan if they are in a scanned folder).",
            ids.len()
        ),
        t!("Remove"),
        move || {
            if let Some(st) = weak.upgrade() {
                st.remove_books(&ids);
                st.save_library();
                refresh_sidebar(&st);
                update_status(&st);
            }
        },
    );
}

fn about(state: &State) {
    gtk::AboutDialog::builder()
        .transient_for(&state.window)
        .modal(true)
        .program_name("Rayonnage")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(t!("Comics, BD and manga library manager inspired by ComicRack CE."))
        .license_type(gtk::License::MitX11)
        .build()
        .present();
}

#[cfg(debug_assertions)]
trait DataState {
    fn data_state(&self) -> Option<State>;
}

#[cfg(debug_assertions)]
impl DataState for gtk::ApplicationWindow {
    fn data_state(&self) -> Option<State> {
        // SAFETY: "app-state" is only ever set to a `State` in `build_ui`.
        unsafe { self.data::<State>("app-state").map(|p| p.as_ref().clone()) }
    }
}

/// Development helper: `BO_ACTIONS=select-all,edit BO_SHOT=/tmp/shot` activates
/// window actions, then saves every visible window as PNG and quits.
#[cfg(debug_assertions)]
fn debug_screenshots(state: &State) {
    let Ok(prefix) = std::env::var("BO_SHOT") else { return };
    let win = state.window.clone();
    let actions = std::env::var("BO_ACTIONS").unwrap_or_default();
    // Actions run one after the other, 400 ms apart.
    for (i, a) in actions.split(',').filter(|a| !a.is_empty()).map(str::to_string).enumerate() {
        let delay = std::time::Duration::from_millis(2500 + 400 * i as u64);
        glib::timeout_add_local_once(delay, glib::clone!(
            #[weak]
            win,
            move || {
                let a = a.as_str();
                // "find:TEXT" types TEXT in the search field of the open log window.
                if let Some(text) = a.strip_prefix("find:") {
                    fn find_entry(w: &gtk::Widget) -> Option<gtk::SearchEntry> {
                        if let Some(e) = w.downcast_ref::<gtk::SearchEntry>() {
                            return Some(e.clone());
                        }
                        let mut c = w.first_child();
                        while let Some(child) = c {
                            if let Some(e) = find_entry(&child) {
                                return Some(e);
                            }
                            c = child.next_sibling();
                        }
                        None
                    }
                    for w in gtk::Window::list_toplevels() {
                        if w.downcast_ref::<gtk::Window>().is_some_and(|w| w.transient_for().is_some())
                            && let Some(e) = find_entry(&w)
                        {
                            e.set_text(text);
                        }
                    }
                    return;
                }
                if a.starts_with("select:") || a.starts_with("type:") || a.starts_with("key:") {
                    if let Some(st) = win.data_state() {
                        inline_edit::debug_action(&st, a);
                    }
                    return;
                }
                // "move-col:Title" simulates dragging a column header to the first place.
                if let Some(key) = a.strip_prefix("move-col:") {
                    if let Some(st) = win.data_state()
                        && let Some((_, col)) = st.columns.borrow().iter().find(|(f, _)| f.key() == key)
                    {
                        st.column_view.insert_column(0, col);
                    }
                    return;
                }
                let (name, param) = match a.split_once(':') {
                    Some((n, p)) => (n, Some(p.to_variant())),
                    None => (a, None),
                };
                let _ = WidgetExt::activate_action(&win, &format!("win.{name}"), param.as_ref());
            }
        ));
    }
    let delay = std::env::var("BO_SHOT_DELAY").ok().and_then(|d| d.parse().ok()).unwrap_or(5000);
    glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
        let windows = gtk::Window::list_toplevels();
        for (i, w) in windows.iter().filter_map(|w| w.downcast_ref::<gtk::Window>()).filter(|w| w.is_visible()).enumerate() {
            let paintable = gtk::WidgetPaintable::new(Some(w));
            let snap = gtk::Snapshot::new();
            paintable.snapshot(snap.upcast_ref::<gdk::Snapshot>(), w.width() as f64, w.height() as f64);
            if let (Some(node), Some(r)) = (snap.to_node(), w.native().and_then(|n| n.renderer())) {
                let _ = r.render_texture(&node, None).save_to_png(format!("{prefix}-{i}.png"));
            }
        }
        if let Some(app) = win.application() {
            app.quit();
        }
    });
}
