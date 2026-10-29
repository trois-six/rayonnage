//! Library Organizer: configuration dialog (port of the ComicRack plugin
//! form), quick run and undo of the last run.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use super::widgets::{self, Form};
use super::{JobCtx, State, refresh_sidebar, run_job};
use crate::model::{Book, Field};
use crate::organizer::{
    self, DEFAULT_ILLEGAL_CHARACTERS, DuplicateAction, EMPTY_DATA_FIELDS, ExcludeMode,
    FAILED_FIELDS, FORBIDDEN_IN_REPLACEMENT, GroupOp, Interaction, MANGA_VALUES, Mode, MultiChoice,
    MultiValueRequest, OrgField, OrganizerSettings, Profile, RULE_FIELDS, RuleGroup, RuleItem,
    RuleOp, RunResult, TEMPLATE_FIELDS, YES_NO_VALUES,
};
use crate::settings::Settings;
use crate::util::human_size;

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// "Library Organizer…": edit the profiles, preview and run.
pub fn open(state: &State) {
    let settings = state.settings.borrow().organizer.clone();
    let preview_book = state
        .selected_books()
        .into_iter()
        .next()
        .or_else(|| state.library.borrow().books.first().cloned());
    Editor::build(state, settings, preview_book);
}

/// "Library Organizer (quick)": runs the last used profiles on the selection.
pub fn quick(state: &State) {
    let Some(books) = state.require_selection() else {
        return;
    };
    let s = state.settings.borrow().organizer.clone();
    if s.profiles.len() == 1 && s.profiles[0].base_folder.trim().is_empty() {
        widgets::alert(
            &state.window,
            t!("Empty base folder"),
            t!("Files cannot be organized while the base folder is empty. \
                First configure the profile with \u{201c}Organize Files…\u{201d}."),
        );
        return;
    }
    let mut profiles: Vec<Profile> = s
        .last_used
        .iter()
        .filter_map(|n| s.profile(n).cloned())
        .collect();
    if profiles.is_empty() {
        profiles.extend(s.profiles.first().cloned());
    }
    if let Some(err) = validation_errors(&profiles) {
        widgets::alert(&state.window, t!("Incompatible settings"), &err);
        return;
    }
    launch(state, books, profiles);
}

/// Runs one profile on the selection (profile menu of the toolbar button);
/// it becomes the profile of "Organize Files Again".
pub fn run_profile(state: &State, name: &str) {
    let Some(books) = state.require_selection() else {
        return;
    };
    let Some(profile) = state.settings.borrow().organizer.profile(name).cloned() else {
        return;
    };
    if let Some(err) = validation_errors(std::slice::from_ref(&profile)) {
        widgets::alert(&state.window, t!("Incompatible settings"), &err);
        return;
    }
    {
        let mut s = state.settings.borrow_mut();
        s.organizer.last_used = vec![name.to_string()];
        let _ = s.save();
    }
    launch(state, books, vec![profile]);
}

/// "Undo the last organization".
pub fn undo(state: &State) {
    let path = Settings::undo_path();
    if !path.exists() {
        widgets::alert(&state.window, t!("Organize Files"), t!("Nothing to undo."));
        return;
    }
    let st = state.clone();
    widgets::confirm(
        &state.window,
        t!("Undo the last organization?"),
        t!("The files moved during the last organization will be put back in their original location."),
        t!("Undo organization"),
        move || {
            let library = shadow(&st.library.borrow().books);
            let settings = st.settings.borrow().organizer.clone();
            run_job(
                &st,
                t!("Undoing the organization…"),
                move |ctx| organizer::undo(&library, &settings, &JobInteraction { ctx }, &path),
                |st, r| apply_result(st, r, t!("Organization undo")),
            );
        },
    );
}

fn validation_errors(profiles: &[Profile]) -> Option<String> {
    let errs: Vec<String> = profiles
        .iter()
        .flat_map(|p| {
            p.validate()
                .into_iter()
                .map(move |e| t!("{0}: {1}", p.name, e))
        })
        .collect();
    (!errs.is_empty()).then(|| t!("Fix the following errors:\n\n{0}", errs.join("\n\n")))
}

/// Books as the plugin sees them: ComicRack "Shadow" values (proposed from the
/// file name when series and number are empty).
fn shadow(books: &[Book]) -> Vec<Book> {
    books.iter().map(Book::with_proposed).collect()
}

fn launch(state: &State, books: Vec<Book>, profiles: Vec<Profile>) {
    let books = shadow(&books);
    let library = shadow(&state.library.borrow().books);
    let undo_path = Settings::undo_path();
    run_job(
        state,
        t!("Organizing books…"),
        move |ctx| {
            organizer::run(
                &books,
                &library,
                &profiles,
                &JobInteraction { ctx },
                &undo_path,
            )
        },
        |st, r| apply_result(st, r, t!("Organize Files")),
    );
}

/// Applies a run (or undo) result to the library on the GTK thread.
fn apply_result(st: &State, mut r: RunResult, title: &str) {
    let mut updates: HashMap<u64, Book> = HashMap::new();
    {
        let lib = st.library.borrow();
        for (id, path) in &r.moved {
            if r.removed.contains(id) {
                continue;
            }
            if let Some(b) = lib.get(*id) {
                let b = updates.entry(*id).or_insert_with(|| b.clone());
                b.file_path = path.clone();
                b.refresh_file_stats();
            }
        }
        for (id, page) in &r.read_progress {
            if r.removed.contains(id) {
                continue;
            }
            if let Some(b) = lib.get(*id) {
                updates
                    .entry(*id)
                    .or_insert_with(|| b.clone())
                    .last_page_read = *page;
            }
        }
    }
    if !r.removed.is_empty() {
        st.remove_books(&r.removed);
    }
    let added = std::mem::take(&mut r.added);
    if !added.is_empty() {
        st.add_books(added);
    }
    st.update_books(updates.into_values().collect(), false);
    st.save_library();
    refresh_sidebar(st);
    st.set_status(&t!(
        "{0}: {1}",
        title,
        r.summary.replace('\n', " ").replace('\t', " · ")
    ));
    for line in organizer::report_text(&r).lines().filter(|l| !l.trim().is_empty()) {
        st.journal.info(line);
    }
    if r.failed_or_skipped || r.simulated {
        widgets::show_log(
            &st.window,
            &t!("{0} — report", title),
            &organizer::report_text(&r),
        );
    } else {
        widgets::alert(&st.window, title, &r.summary);
    }
}

// ---------------------------------------------------------------------------
// Worker-side interaction (dialogs shown on the GTK thread)
// ---------------------------------------------------------------------------

struct JobInteraction {
    ctx: JobCtx,
}

impl Interaction for JobInteraction {
    fn choose_multi_values(
        &self,
        book: &Book,
        field: Field,
        values: &[String],
    ) -> Option<MultiChoice> {
        self.choose_multi_values_ex(&MultiValueRequest {
            book,
            field,
            values: values.to_vec(),
            preselected: Vec::new(),
            whole_series: false,
            book_text: book.caption(),
        })
    }

    fn choose_multi_values_ex(&self, req: &MultiValueRequest) -> Option<MultiChoice> {
        let q = MultiQuestion {
            field: req.field.label().to_string(),
            book_text: req.book_text.clone(),
            values: req.values.clone(),
            preselected: req.preselected.clone(),
            whole_series: req.whole_series,
        };
        self.ctx
            .ask(move |st, reply| multi_value_dialog(st, q, reply))
            .flatten()
    }

    fn on_duplicate(
        &self,
        book: &Book,
        dest: &std::path::Path,
        existing_size: Option<u64>,
    ) -> (DuplicateAction, bool) {
        let q = DuplicateQuestion {
            caption: book.caption(),
            src: book.file_path.clone(),
            src_size: std::fs::metadata(&book.file_path)
                .map(|m| m.len())
                .unwrap_or(book.file_size),
            dest: dest.to_path_buf(),
            existing_size,
        };
        self.ctx
            .ask(move |st, reply| duplicate_dialog(st, q, reply))
            .unwrap_or((DuplicateAction::Skip, false))
    }

    fn is_cancelled(&self) -> bool {
        self.ctx.cancelled()
    }

    fn progress(&self, done: usize, total: usize) {
        self.ctx.progress(done, total, format!("{done}/{total}"));
    }
}

struct DuplicateQuestion {
    caption: String,
    src: PathBuf,
    src_size: u64,
    dest: PathBuf,
    existing_size: Option<u64>,
}

type Reply<R> = Rc<RefCell<Option<Box<dyn FnOnce(R) + Send>>>>;

fn answer<R>(slot: &Reply<R>, r: R) {
    if let Some(f) = slot.borrow_mut().take() {
        f(r);
    }
}

fn duplicate_dialog(
    st: &State,
    q: DuplicateQuestion,
    reply: Box<dyn FnOnce((DuplicateAction, bool)) + Send>,
) {
    let slot: Reply<(DuplicateAction, bool)> = Rc::new(RefCell::new(Some(reply)));
    let (win, content, buttons) = widgets::dialog(&st.window, t!("The file already exists"), 640, 280);
    let mut form = Form::new();
    form.add_full(
        &gtk::Label::builder()
            .label(t!("A file already exists at the calculated location. What do you want to do?"))
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    let info = |s: String| {
        gtk::Label::builder()
            .label(s)
            .xalign(0.0)
            .selectable(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build()
    };
    form.add(t!("Book:"), &info(q.caption));
    form.add(
        t!("File:"),
        &info(format!("{} ({})", q.src.display(), human_size(q.src_size))),
    );
    let existing = q
        .existing_size
        .map(human_size)
        .unwrap_or_else(|| t!("unknown size").into());
    form.add(
        t!("Existing:"),
        &info(format!("{} ({existing})", q.dest.display())),
    );
    let all = widgets::check(t!("Do the same for all conflicts"), false);
    form.add_full(&all);
    content.append(&form.grid);

    let add = |label: &str, action: DuplicateAction, suggested: bool| {
        let b = widgets::button(label, suggested);
        let slot = slot.clone();
        let all = all.clone();
        b.connect_clicked(glib::clone!(
            #[weak]
            win,
            move |_| {
                answer(&slot, (action, all.is_active()));
                win.close();
            }
        ));
        buttons.append(&b);
    };
    add(t!("Skip"), DuplicateAction::Skip, false);
    add(t!("Replace"), DuplicateAction::Overwrite, false);
    add(
        t!("Keep both (rename)"),
        DuplicateAction::Rename,
        true,
    );
    win.connect_close_request(move |_| {
        answer(&slot, (DuplicateAction::Skip, false));
        glib::Propagation::Proceed
    });
    win.present();
}

struct MultiQuestion {
    field: String,
    book_text: String,
    values: Vec<String>,
    preselected: Vec<String>,
    whole_series: bool,
}

fn multi_value_dialog(
    st: &State,
    q: MultiQuestion,
    reply: Box<dyn FnOnce(Option<MultiChoice>) + Send>,
) {
    let slot: Reply<Option<MultiChoice>> = Rc::new(RefCell::new(Some(reply)));
    let (win, content, buttons) = widgets::dialog(
        &st.window,
        &t!("Multiple values: {0}", q.field),
        560,
        520,
    );
    let target = if q.whole_series {
        t!("every issue of the series {0}", q.book_text)
    } else {
        t!("issue {0}", q.book_text)
    };
    content.append(
        &gtk::Label::builder()
            .label(t!(
                "Check the \u{201c}{0}\u{201d} values to use for {1}, in the desired order:",
                q.field,
                target
            ))
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    let mut items: Vec<(String, bool)> = q.preselected.iter().map(|v| (v.clone(), true)).collect();
    items.extend(
        q.values
            .iter()
            .filter(|v| !q.preselected.contains(v))
            .map(|v| (v.clone(), false)),
    );
    let items = Rc::new(RefCell::new(items));
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    rebuild_ordered_checks(&list, &items);
    content.append(&widgets::scrolled(&list));

    let folder = widgets::check(t!("Use each value as a folder"), false);
    content.append(&folder);
    let every = widgets::check(
        t!("Use all the selected values for every issue of the series, even if it does not have them"),
        false,
    );
    let always = widgets::check(
        t!("Use this selection for every issue of this operation that has these values"),
        false,
    );
    let dont_ask = widgets::check(t!("Do not ask again if there are other values"), true);
    dont_ask.set_sensitive(false);
    always.connect_toggled(glib::clone!(
        #[weak]
        dont_ask,
        move |c| dont_ask.set_sensitive(c.is_active())
    ));
    if q.whole_series {
        content.append(&every);
    } else {
        content.append(&always);
        content.append(&dont_ask);
    }

    let cancel = widgets::button(t!("Cancel"), false);
    let ok = widgets::button("OK", true);
    cancel.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    let ok_slot = slot.clone();
    ok.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            let choice = MultiChoice {
                selected: items
                    .borrow()
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(v, _)| v.clone())
                    .collect(),
                each_as_folder: folder.is_active(),
                always_use: always.is_active(),
                dont_ask: always.is_active() && dont_ask.is_active(),
                every_issue: every.is_active(),
            };
            answer(&ok_slot, Some(choice));
            win.close();
        }
    ));
    buttons.append(&cancel);
    buttons.append(&ok);
    win.connect_close_request(move |_| {
        answer(&slot, None);
        glib::Propagation::Proceed
    });
    win.present();
}

/// A list of check buttons that can be reordered with up/down buttons.
fn rebuild_ordered_checks(list: &gtk::ListBox, items: &Rc<RefCell<Vec<(String, bool)>>>) {
    list.remove_all();
    let n = items.borrow().len();
    for (i, (label, on)) in items.borrow().iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.set_margin_start(6);
        row.set_margin_end(6);
        let check = widgets::check(label, *on);
        check.set_hexpand(true);
        let it = items.clone();
        check.connect_toggled(move |c| {
            if let Some(e) = it.borrow_mut().get_mut(i) {
                e.1 = c.is_active();
            }
        });
        row.append(&check);
        for (icon, up) in [("go-up-symbolic", true), ("go-down-symbolic", false)] {
            let b = gtk::Button::from_icon_name(icon);
            b.add_css_class("flat");
            b.set_sensitive(if up { i > 0 } else { i + 1 < n });
            let it = items.clone();
            b.connect_clicked(glib::clone!(
                #[weak]
                list,
                move |_| {
                    let j = if up { i - 1 } else { i + 1 };
                    it.borrow_mut().swap(i, j);
                    rebuild_ordered_checks(&list, &it);
                }
            ));
            row.append(&b);
        }
        list.append(&row);
    }
}

// ---------------------------------------------------------------------------
// Configuration dialog
// ---------------------------------------------------------------------------

struct StrList {
    list: gtk::ListBox,
    items: RefCell<Vec<String>>,
}

struct RuleRow {
    field: gtk::DropDown,
    op: gtk::DropDown,
    value: gtk::Entry,
}

struct Editor {
    state: State,
    win: glib::WeakRef<gtk::Window>,
    profiles: RefCell<Vec<Profile>>,
    cur: Cell<usize>,
    loading: Cell<bool>,
    preview_book: Option<Book>,
    // profiles to run, in order
    run_list: Rc<RefCell<Vec<(String, bool)>>>,
    run_box: gtk::ListBox,
    profile_model: gtk::StringList,
    profile_dd: gtk::DropDown,
    // General
    mode_move: gtk::CheckButton,
    mode_copy: gtk::CheckButton,
    mode_sim: gtk::CheckButton,
    copy_add: gtk::CheckButton,
    base_folder: gtk::Entry,
    use_folder: gtk::CheckButton,
    folder_tpl: gtk::Entry,
    use_file: gtk::CheckButton,
    file_tpl: gtk::Entry,
    auto_space: gtk::CheckButton,
    preview: gtk::Label,
    errors: gtk::Label,
    // Empty data
    empty_folder: gtk::Entry,
    empty_field: gtk::DropDown,
    empty_value: gtk::Entry,
    empty_summary: gtk::Label,
    empty_data: RefCell<BTreeMap<OrgField, String>>,
    fail_empty: gtk::CheckButton,
    failed_box: gtk::FlowBox,
    failed_checks: Vec<(OrgField, gtk::CheckButton)>,
    move_failed: gtk::CheckButton,
    failed_folder: gtk::Entry,
    failed_folder_box: gtk::Box,
    // Characters & months
    illegal_box: gtk::ListBox,
    illegal: RefCell<Vec<(String, gtk::Entry)>>,
    months: Vec<(u32, gtk::Entry)>,
    // Exclusions
    excl_folders: StrList,
    rules_mode: gtk::DropDown,
    rules_op: gtk::DropDown,
    rules_box: gtk::ListBox,
    rules: RefCell<Vec<RuleRow>>,
    kept_groups: RefCell<Vec<RuleItem>>,
    groups_note: gtk::Label,
    // Options
    remove_empty: gtk::CheckButton,
    excl_empty: StrList,
    excl_empty_box: gtk::Box,
    dont_ask: gtk::CheckButton,
    replace_spaces: gtk::CheckButton,
    copy_read: gtk::CheckButton,
}

/// A closure calling `f` on the editor if it is still alive.
fn weak_fn(ed: &Rc<Editor>, f: impl Fn(&Rc<Editor>) + 'static) -> impl Fn() + 'static {
    let w = Rc::downgrade(ed);
    move || {
        if let Some(e) = w.upgrade() {
            f(&e)
        }
    }
}

fn page_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.set_margin_start(12);
    b.set_margin_end(12);
    b.set_margin_top(12);
    b.set_margin_bottom(12);
    b
}

fn label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .build()
}

fn title(text: &str) -> gtk::Label {
    let l = gtk::Label::builder().xalign(0.0).margin_top(6).build();
    l.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(text)));
    l
}

fn dim(text: &str) -> gtk::Label {
    let l = label(text);
    l.add_css_class("dim-label");
    l
}

fn list_box() -> gtk::ListBox {
    let l = gtk::ListBox::new();
    l.set_selection_mode(gtk::SelectionMode::None);
    l.add_css_class("boxed-list");
    l
}

/// Text inserted by the "insert field" menu.
fn template_snippet(name: &str, space: bool) -> String {
    let inner = match name {
        "writer" | "penciller" | "inker" | "colorist" | "letterer" | "coverartist" | "editor"
        | "genre" | "tags" | "characters" | "teams" | "locations" | "scaninfo" => {
            format!("{name}(, )(issue)")
        }
        "manga" => "manga(Manga)".into(),
        "seriesComplete" => format!("seriesComplete({})", t!("Complete")),
        "read" => format!("read({})(>)(90)", t!("Read")),
        "counter" => "counter(1)(1)(3)".into(),
        "first" => "first(Series)".into(),
        "AddedDate" | "ReleasedDate" => format!("{name}(yyyy-MM-dd)"),
        "Custom" => format!("Custom({})", t!("key")),
        "startmonth#" | "EndMonth#" | "month#" => format!("{name}2"),
        _ => name.to_string(),
    };
    format!("{{{}<{inner}>}}", if space { " " } else { "" })
}

fn insert_at_cursor(entry: &gtk::Entry, text: &str) {
    let mut pos = entry.position();
    entry.insert_text(text, &mut pos);
    entry.set_position(pos);
}

fn insert_menu(target: &gtk::Entry, auto_space: &gtk::CheckButton) -> gtk::MenuButton {
    let pop = gtk::Popover::new();
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for (name, lbl) in TEMPLATE_FIELDS {
        let b = gtk::Button::new();
        b.add_css_class("flat");
        b.set_child(Some(
            &gtk::Label::builder()
                .label(format!("{}   <{name}>", crate::i18n::tr(lbl)))
                .xalign(0.0)
                .build(),
        ));
        b.connect_clicked(glib::clone!(
            #[weak]
            target,
            #[weak]
            auto_space,
            #[weak]
            pop,
            move |_| {
                pop.popdown();
                insert_at_cursor(&target, &template_snippet(name, auto_space.is_active()));
            }
        ));
        list.append(&b);
    }
    pop.set_child(Some(
        &gtk::ScrolledWindow::builder()
            .child(&list)
            .min_content_height(380)
            .propagate_natural_width(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    ));
    gtk::MenuButton::builder()
        .label(t!("Insert field"))
        .popover(&pop)
        .build()
}

fn rule_field_labels() -> Vec<&'static str> {
    RULE_FIELDS.iter().map(|f| f.label()).collect()
}

fn yes_no_placeholder(f: OrgField) -> String {
    let values = if f == OrgField::Book(Field::Manga) {
        MANGA_VALUES
    } else if f.is_yes_no() {
        YES_NO_VALUES
    } else {
        return String::new();
    };
    values
        .iter()
        .map(|v| crate::i18n::tr(v))
        .collect::<Vec<_>>()
        .join(" / ")
}

impl Editor {
    fn build(state: &State, settings: OrganizerSettings, preview_book: Option<Book>) {
        let (win, content, buttons) = widgets::dialog(&state.window, t!("Organize Files"), 920, 720);
        let mut profiles = settings.profiles.clone();
        if profiles.is_empty() {
            profiles.push(Profile::default());
        }
        let names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
        let cur = settings
            .last_used
            .first()
            .and_then(|n| names.iter().position(|x| x == n))
            .unwrap_or(0);
        let mut run_list: Vec<(String, bool)> = settings
            .last_used
            .iter()
            .filter(|n| names.contains(n))
            .map(|n| (n.clone(), true))
            .collect();
        for n in &names {
            if !run_list.iter().any(|(x, _)| x == n) {
                run_list.push((n.clone(), run_list.is_empty()));
            }
        }

        // --- Profile bar
        let profile_model =
            gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let profile_dd = gtk::DropDown::new(Some(profile_model.clone()), None::<gtk::Expression>);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.append(&gtk::Label::new(Some(t!("Profile:"))));
        profile_dd.set_hexpand(true);
        bar.append(&profile_dd);
        let pb = |l: &str| {
            let b = gtk::Button::with_label(l);
            bar.append(&b);
            b
        };
        let new_btn = pb(t!("New…"));
        let rename_btn = pb(t!("Rename…"));
        let dup_btn = pb(t!("Duplicate…"));
        let del_btn = pb(t!("Delete"));
        let import_btn = pb(t!("Import…"));
        let export_btn = pb(t!("Export…"));
        let export_all_btn = pb(t!("Export all…"));
        content.append(&bar);

        let notebook = gtk::Notebook::new();
        notebook.set_vexpand(true);
        content.append(&notebook);

        // --- General
        let g = page_box();
        let mode_move = gtk::CheckButton::with_label(Mode::Move.label());
        let mode_copy = gtk::CheckButton::with_label(Mode::Copy.label());
        let mode_sim =
            gtk::CheckButton::with_label(t!("Simulate (no file modified, full report)"));
        mode_copy.set_group(Some(&mode_move));
        mode_sim.set_group(Some(&mode_move));
        let copy_add = widgets::check(t!("Add the copies to the library"), true);
        let modes = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        modes.append(&gtk::Label::new(Some(t!("Mode:"))));
        for w in [&mode_move, &mode_copy, &copy_add, &mode_sim] {
            modes.append(w);
        }
        g.append(&modes);
        let mut form = Form::new();
        let (base_box, base_folder) = widgets::path_entry(win.upcast_ref(), "", false);
        form.add(t!("Base folder:"), &base_box);
        g.append(&form.grid);

        let auto_space = widgets::check(
            t!("Automatically insert a space before inserted fields"),
            true,
        );
        let use_folder = widgets::check(t!("Organize folders — folder template:"), true);
        let folder_tpl = widgets::entry("");
        folder_tpl.set_hexpand(true);
        let sep_btn = gtk::Button::with_label(t!("Folder separator"));
        sep_btn.connect_clicked(glib::clone!(
            #[weak]
            folder_tpl,
            move |_| insert_at_cursor(&folder_tpl, "/")
        ));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&folder_tpl);
        row.append(&insert_menu(&folder_tpl, &auto_space));
        row.append(&sep_btn);
        g.append(&title(t!("Folders")));
        g.append(&use_folder);
        g.append(&row);

        let use_file = widgets::check(t!("Rename files — file name template:"), true);
        let file_tpl = widgets::entry("");
        file_tpl.set_hexpand(true);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&file_tpl);
        row.append(&insert_menu(&file_tpl, &auto_space));
        g.append(&title(t!("Files")));
        g.append(&use_file);
        g.append(&row);
        g.append(&auto_space);
        g.append(&dim(t!(
            "Syntax: {prefix<field>postfix} — the whole block disappears if the field is empty; blocks \
             can be nested: { ({<month>, }<year>)}.\n\
             <number3>: zero padding (0 = automatic, from the last issue of the series). \
             <month>: month name, <month#2>: number.\n\
             <writer(, )(issue)>: multiple values with a separator, chosen per issue (issue) or per series (series).\n\
             {text<?field(value)>}: text inserted if the field equals \u{201c}value\u{201d} ((!regex) for a regular expression); \
             {text<!field>}: if the field is empty; {text<!field(value)>}: if it is not \u{201c}value\u{201d}.\n\
             / or \\ in the folder template: subfolder."
        )));
        notebook.append_page(
            &widgets::scrolled(&g),
            Some(&gtk::Label::new(Some(t!("General")))),
        );

        // --- Empty data
        let d = page_box();
        let empty_folder = widgets::entry("");
        let mut form = Form::new();
        form.add(t!("Replace empty folder names with:"), &empty_folder);
        form.add_full(&dim(t!("Leave empty to remove that folder level.")));
        d.append(&form.grid);
        d.append(&title(t!("When a field is empty, replace it with")));
        let empty_field = widgets::dropdown(
            &EMPTY_DATA_FIELDS
                .iter()
                .map(|f| f.label())
                .collect::<Vec<_>>(),
            0,
        );
        let empty_value = widgets::entry("");
        empty_value.set_hexpand(true);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&gtk::Label::new(Some(t!("Field:"))));
        row.append(&empty_field);
        row.append(&gtk::Label::new(Some(t!("replacement:"))));
        row.append(&empty_value);
        d.append(&row);
        let empty_summary = dim("");
        d.append(&empty_summary);
        d.append(&title(t!("Required fields")));
        let fail_empty = widgets::check(
            t!("If one of the checked fields is empty, mark the operation as failed"),
            false,
        );
        d.append(&fail_empty);
        let failed_box = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(3)
            .max_children_per_line(4)
            .row_spacing(2)
            .build();
        let failed_checks: Vec<(OrgField, gtk::CheckButton)> = FAILED_FIELDS
            .iter()
            .map(|f| {
                let c = widgets::check(f.label(), false);
                failed_box.insert(&c, -1);
                (*f, c)
            })
            .collect();
        d.append(&failed_box);
        let move_failed = widgets::check(
            t!("… and still move/copy these books into this folder:"),
            false,
        );
        d.append(&move_failed);
        let (failed_folder_box, failed_folder) = widgets::path_entry(win.upcast_ref(), "", false);
        d.append(&failed_folder_box);
        notebook.append_page(
            &widgets::scrolled(&d),
            Some(&gtk::Label::new(Some(t!("Empty data")))),
        );

        // --- Characters & months
        let c = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        c.set_margin_start(12);
        c.set_margin_end(12);
        c.set_margin_top(12);
        let left = gtk::Box::new(gtk::Orientation::Vertical, 6);
        left.set_hexpand(true);
        left.append(&title(t!("Illegal characters")));
        left.append(&dim(t!(
            "Each character is replaced by the given text (the nine default characters cannot be removed)."
        )));
        let illegal_box = list_box();
        left.append(&illegal_box);
        let right = gtk::Box::new(gtk::Orientation::Vertical, 6);
        right.append(&title(t!("Month names")));
        let grid = gtk::Grid::builder()
            .row_spacing(4)
            .column_spacing(8)
            .build();
        let months: Vec<(u32, gtk::Entry)> = (1..=16u32)
            .map(|m| {
                let e = widgets::entry("");
                grid.attach(
                    &gtk::Label::builder()
                        .label(format!("{m}"))
                        .xalign(1.0)
                        .build(),
                    0,
                    m as i32,
                    1,
                    1,
                );
                grid.attach(&e, 1, m as i32, 1, 1);
                (m, e)
            })
            .collect();
        right.append(&grid);
        right.append(&dim(t!("13 to 16: seasons.")));
        c.append(&left);
        c.append(&right);
        notebook.append_page(
            &widgets::scrolled(&c),
            Some(&gtk::Label::new(Some(t!("Characters & months")))),
        );

        // --- Exclusions
        let x = page_box();
        x.append(&title(t!("Excluded folders")));
        x.append(&label(t!(
            "Do not move books located in the following folders:"
        )));
        let excl_folders = StrList {
            list: list_box(),
            items: RefCell::new(Vec::new()),
        };
        x.append(&excl_folders.list);
        let add_excl = gtk::Button::with_label(t!("Add a folder…"));
        add_excl.set_halign(gtk::Align::Start);
        x.append(&add_excl);
        x.append(&title(t!("Metadata rules")));
        let rules_mode = widgets::dropdown(
            &ExcludeMode::ALL
                .iter()
                .map(|m| m.label())
                .collect::<Vec<_>>(),
            0,
        );
        let rules_op = widgets::dropdown(
            &GroupOp::ALL.iter().map(|m| m.label()).collect::<Vec<_>>(),
            0,
        );
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&rules_mode);
        row.append(&gtk::Label::new(Some(t!("move books that match"))));
        row.append(&rules_op);
        row.append(&gtk::Label::new(Some(t!("of the following rules:"))));
        x.append(&row);
        let rules_box = list_box();
        x.append(&rules_box);
        let add_rule = gtk::Button::with_label(t!("Add a rule"));
        add_rule.set_halign(gtk::Align::Start);
        x.append(&add_rule);
        let groups_note = dim("");
        x.append(&groups_note);
        x.append(&dim(t!("Without any rule, every book is moved.")));
        notebook.append_page(
            &widgets::scrolled(&x),
            Some(&gtk::Label::new(Some(t!("Exclusions")))),
        );

        // --- Options
        let o = page_box();
        let remove_empty = widgets::check(t!("Remove empty folders"), true);
        o.append(&remove_empty);
        let excl_empty_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        excl_empty_box.set_margin_start(24);
        excl_empty_box.append(&label(t!("But do not remove the following folders:")));
        let excl_empty = StrList {
            list: list_box(),
            items: RefCell::new(Vec::new()),
        };
        excl_empty_box.append(&excl_empty.list);
        let add_empty = gtk::Button::with_label(t!("Add a folder…"));
        add_empty.set_halign(gtk::Align::Start);
        excl_empty_box.append(&add_empty);
        o.append(&excl_empty_box);
        let dont_ask = widgets::check(
            t!("If a multiple-value field contains only one value, insert it without asking"),
            true,
        );
        let replace_spaces =
            widgets::check(t!("Replace multiple spaces with a single space"), true);
        let copy_read = widgets::check(
            t!("When replacing an existing file, keep its reading progress"),
            true,
        );
        for w in [&dont_ask, &replace_spaces, &copy_read] {
            o.append(w);
        }
        notebook.append_page(
            &widgets::scrolled(&o),
            Some(&gtk::Label::new(Some(t!("Options")))),
        );

        // --- Preview
        let preview_title = match &preview_book {
            Some(b) => t!("Preview for \u{201c}{0}\u{201d}:", b.caption()),
            None => t!("Preview: no book in the library").into(),
        };
        content.append(&title(&preview_title));
        let preview = gtk::Label::builder()
            .xalign(0.0)
            .selectable(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build();
        content.append(&preview);
        let errors = label("");
        errors.add_css_class("error");
        content.append(&errors);

        // --- Buttons
        let run_box = list_box();
        let pop = gtk::Popover::new();
        let pbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        pbox.append(&label(t!(
            "Profiles run (in order) by \u{201c}Organize\u{201d}:"
        )));
        pbox.append(&run_box);
        pop.set_child(Some(&pbox));
        let run_menu = gtk::MenuButton::builder()
            .label(t!("Profiles to run"))
            .popover(&pop)
            .build();
        let cancel = widgets::button(t!("Cancel"), false);
        let save = widgets::button(t!("Save"), false);
        let organize = widgets::button(t!("Organize"), true);
        buttons.append(&run_menu);
        buttons.append(&cancel);
        buttons.append(&save);
        buttons.append(&organize);

        let ed = Rc::new(Editor {
            state: state.clone(),
            win: win.downgrade(),
            profiles: RefCell::new(profiles),
            cur: Cell::new(cur),
            loading: Cell::new(false),
            preview_book,
            run_list: Rc::new(RefCell::new(run_list)),
            run_box,
            profile_model,
            profile_dd,
            mode_move,
            mode_copy,
            mode_sim,
            copy_add,
            base_folder,
            use_folder,
            folder_tpl,
            use_file,
            file_tpl,
            auto_space,
            preview,
            errors,
            empty_folder,
            empty_field,
            empty_value,
            empty_summary,
            empty_data: RefCell::new(BTreeMap::new()),
            fail_empty,
            failed_box,
            failed_checks,
            move_failed,
            failed_folder,
            failed_folder_box,
            illegal_box,
            illegal: RefCell::new(Vec::new()),
            months,
            excl_folders,
            rules_mode,
            rules_op,
            rules_box,
            rules: RefCell::new(Vec::new()),
            kept_groups: RefCell::new(Vec::new()),
            groups_note,
            remove_empty,
            excl_empty,
            excl_empty_box,
            dont_ask,
            replace_spaces,
            copy_read,
        });

        // --- Change notifications
        let changed = || {
            let f = weak_fn(&ed, |e| e.changed());
            move || f()
        };
        for c in [
            &ed.mode_move,
            &ed.mode_copy,
            &ed.mode_sim,
            &ed.copy_add,
            &ed.use_folder,
            &ed.use_file,
            &ed.auto_space,
            &ed.fail_empty,
            &ed.move_failed,
            &ed.remove_empty,
            &ed.dont_ask,
            &ed.replace_spaces,
            &ed.copy_read,
        ] {
            let f = changed();
            c.connect_toggled(move |_| f());
        }
        for (_, c) in &ed.failed_checks {
            let f = changed();
            c.connect_toggled(move |_| f());
        }
        for e in [
            &ed.base_folder,
            &ed.folder_tpl,
            &ed.file_tpl,
            &ed.empty_folder,
            &ed.failed_folder,
        ] {
            let f = changed();
            e.connect_changed(move |_| f());
        }
        for (_, e) in &ed.months {
            let f = changed();
            e.connect_changed(move |_| f());
        }
        for d in [&ed.rules_mode, &ed.rules_op] {
            let f = changed();
            d.connect_selected_notify(move |_| f());
        }
        let f = weak_fn(&ed, |e| {
            e.loading.set(true);
            e.empty_value.set_text(
                e.empty_data
                    .borrow()
                    .get(&e.empty_key())
                    .map(String::as_str)
                    .unwrap_or(""),
            );
            e.loading.set(false);
        });
        ed.empty_field.connect_selected_notify(move |_| f());
        let f = weak_fn(&ed, |e| {
            if e.loading.get() {
                return;
            }
            let key = e.empty_key();
            e.empty_data
                .borrow_mut()
                .insert(key, e.empty_value.text().to_string());
            e.update_empty_summary();
            e.changed();
        });
        ed.empty_value.connect_changed(move |_| f());
        let f = weak_fn(&ed, |e| {
            if e.loading.get() {
                return;
            }
            let i = e.profile_dd.selected() as usize;
            if i < e.profiles.borrow().len() && i != e.cur.get() {
                e.cur.set(i);
                e.load();
            }
        });
        ed.profile_dd.connect_selected_notify(move |_| f());

        for (btn, which) in [(&add_excl, 0), (&add_empty, 1)] {
            let f = weak_fn(&ed, move |e| {
                let Some(win) = e.win.upgrade() else { return };
                let w = Rc::downgrade(e);
                widgets::choose_folder(&win, t!("Choose a folder"), move |p| {
                    if let Some(e) = w.upgrade() {
                        let sel = if which == 0 {
                            sel_excl_folders
                        } else {
                            sel_excl_empty
                        };
                        sel(&e)
                            .items
                            .borrow_mut()
                            .push(p.to_string_lossy().into_owned());
                        rebuild_strlist(&e, sel);
                        e.changed();
                    }
                });
            });
            btn.connect_clicked(move |_| f());
        }
        let f = weak_fn(&ed, |e| {
            let mut items = e.rule_items();
            items.push((
                OrgField::Book(Field::Series),
                RuleOp::Contains,
                String::new(),
            ));
            rebuild_rules(e, items);
            e.changed();
        });
        add_rule.connect_clicked(move |_| f());

        // Profile actions
        let f = weak_fn(&ed, |e| {
            let w = Rc::downgrade(e);
            e.ask_name(t!("New profile"), "", move |name| {
                if let Some(e) = w.upgrade() {
                    e.add_profile(Profile {
                        name,
                        ..Default::default()
                    });
                }
            });
        });
        new_btn.connect_clicked(move |_| f());
        let f = weak_fn(&ed, |e| {
            let w = Rc::downgrade(e);
            let old = e.current().name;
            let initial = t!("{0} (copy)", old);
            e.ask_name(t!("Duplicate profile"), &initial, move |name| {
                if let Some(e) = w.upgrade() {
                    let p = Profile {
                        name,
                        ..e.current()
                    };
                    e.add_profile(p);
                }
            });
        });
        dup_btn.connect_clicked(move |_| f());
        let f = weak_fn(&ed, |e| {
            let w = Rc::downgrade(e);
            let old = e.current().name;
            e.ask_name(t!("Rename profile"), &old.clone(), move |name| {
                if let Some(e) = w.upgrade() {
                    let i = e.cur.get();
                    e.profiles.borrow_mut()[i].name = name.clone();
                    for r in e
                        .run_list
                        .borrow_mut()
                        .iter_mut()
                        .filter(|(n, _)| *n == old)
                    {
                        r.0 = name.clone();
                    }
                    e.refresh_profiles();
                }
            });
        });
        rename_btn.connect_clicked(move |_| f());
        let f = weak_fn(&ed, |e| {
            let Some(win) = e.win.upgrade() else { return };
            if e.profiles.borrow().len() <= 1 {
                widgets::alert(
                    &win,
                    t!("Cannot delete"),
                    t!("At least one profile must remain."),
                );
                return;
            }
            let w = Rc::downgrade(e);
            let name = e.current().name;
            widgets::confirm(
                &win,
                t!("Delete the profile?"),
                &t!("The profile \u{201c}{0}\u{201d} will be deleted.", name),
                t!("Delete"),
                move || {
                    if let Some(e) = w.upgrade() {
                        let i = e.cur.get();
                        e.profiles.borrow_mut().remove(i);
                        e.run_list.borrow_mut().retain(|(n, _)| *n != name);
                        e.cur.set(i.min(e.profiles.borrow().len() - 1));
                        e.refresh_profiles();
                        e.load();
                    }
                },
            );
        });
        del_btn.connect_clicked(move |_| f());
        let f = weak_fn(&ed, |e| e.import());
        import_btn.connect_clicked(move |_| f());
        let f = weak_fn(&ed, |e| e.export(false));
        export_btn.connect_clicked(move |_| f());
        let f = weak_fn(&ed, |e| e.export(true));
        export_all_btn.connect_clicked(move |_| f());

        cancel.connect_clicked(glib::clone!(
            #[weak]
            win,
            move |_| win.close()
        ));
        let f = weak_fn(&ed, |e| {
            if e.save_settings()
                && let Some(w) = e.win.upgrade()
            {
                w.close();
            }
        });
        save.connect_clicked(move |_| f());
        let f = weak_fn(&ed, |e| e.organize());
        organize.connect_clicked(move |_| f());

        // The editor lives as long as the window.
        let keep = RefCell::new(Some(ed.clone()));
        win.connect_destroy(move |_| {
            keep.borrow_mut().take();
        });

        ed.profile_dd.set_selected(cur as u32);
        rebuild_run_list(&ed);
        ed.load();
        win.present();
    }

    fn current(&self) -> Profile {
        self.profiles.borrow()[self.cur.get()].clone()
    }

    fn empty_key(&self) -> OrgField {
        EMPTY_DATA_FIELDS
            .get(self.empty_field.selected() as usize)
            .copied()
            .unwrap_or(OrgField::Book(Field::Series))
    }

    fn rule_items(&self) -> Vec<(OrgField, RuleOp, String)> {
        self.rules
            .borrow()
            .iter()
            .map(|r| {
                (
                    RULE_FIELDS
                        .get(r.field.selected() as usize)
                        .copied()
                        .unwrap_or(OrgField::Book(Field::Series)),
                    RuleOp::ALL
                        .get(r.op.selected() as usize)
                        .copied()
                        .unwrap_or_default(),
                    r.value.text().to_string(),
                )
            })
            .collect()
    }

    fn illegal_map(&self) -> BTreeMap<String, String> {
        self.illegal
            .borrow()
            .iter()
            .map(|(k, e)| (k.clone(), e.text().to_string()))
            .collect()
    }

    /// Reads every widget into a profile (based on the current one).
    fn store(&self) -> Profile {
        let mut p = self.current();
        p.mode = if self.mode_copy.is_active() {
            Mode::Copy
        } else if self.mode_sim.is_active() {
            Mode::Simulate
        } else {
            Mode::Move
        };
        p.copy_add_to_library = self.copy_add.is_active();
        p.base_folder = self.base_folder.text().trim().to_string();
        p.use_folder = self.use_folder.is_active();
        p.folder_template = self.folder_tpl.text().to_string();
        p.use_file_name = self.use_file.is_active();
        p.file_template = self.file_tpl.text().to_string();
        p.auto_space_fields = self.auto_space.is_active();
        p.empty_folder = self.empty_folder.text().to_string();
        p.empty_data = self
            .empty_data
            .borrow()
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        p.fail_empty_values = self.fail_empty.is_active();
        p.failed_fields = self
            .failed_checks
            .iter()
            .filter(|(_, c)| c.is_active())
            .map(|(f, _)| *f)
            .collect();
        p.move_failed = self.move_failed.is_active();
        p.failed_folder = self.failed_folder.text().trim().to_string();
        p.illegal_characters = self.illegal_map();
        for (m, e) in &self.months {
            p.months.insert(*m, e.text().to_string());
        }
        p.exclude_folders = self.excl_folders.items.borrow().clone();
        p.exclude_mode = ExcludeMode::ALL[(self.rules_mode.selected() as usize).min(1)];
        let mut items: Vec<RuleItem> = self
            .rule_items()
            .into_iter()
            .map(|(field, operator, value)| RuleItem::Rule {
                field,
                operator,
                value,
            })
            .collect();
        items.extend(self.kept_groups.borrow().iter().cloned());
        p.exclude_rules = RuleGroup {
            operator: GroupOp::ALL[(self.rules_op.selected() as usize).min(1)],
            items,
        };
        p.remove_empty_folders = self.remove_empty.is_active();
        p.excluded_empty_folders = self.excl_empty.items.borrow().clone();
        p.dont_ask_when_multi_one = self.dont_ask.is_active();
        p.replace_multiple_spaces = self.replace_spaces.is_active();
        p.copy_read_percentage = self.copy_read.is_active();
        p
    }

    fn changed(&self) {
        if self.loading.get() {
            return;
        }
        let p = self.store();
        self.profiles.borrow_mut()[self.cur.get()] = p;
        self.update_sensitivity();
        self.refresh_preview();
    }

    fn update_sensitivity(&self) {
        self.copy_add.set_sensitive(self.mode_copy.is_active());
        self.folder_tpl.set_sensitive(self.use_folder.is_active());
        self.file_tpl.set_sensitive(self.use_file.is_active());
        let fail = self.fail_empty.is_active();
        self.failed_box.set_sensitive(fail);
        self.move_failed.set_sensitive(fail);
        self.failed_folder_box
            .set_sensitive(fail && self.move_failed.is_active());
        self.excl_empty_box
            .set_sensitive(self.remove_empty.is_active());
    }

    fn refresh_preview(&self) {
        let p = self.current();
        let text = match &self.preview_book {
            Some(b) => organizer::preview(&b.with_proposed(), &shadow(&self.state.library.borrow().books), &p),
            None => String::new(),
        };
        self.preview.set_text(&text);
        self.errors.set_text(&p.validate().join("\n"));
    }

    fn update_empty_summary(&self) {
        let data = self.empty_data.borrow();
        let parts: Vec<String> = data
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| t!("{0} → \u{201c}{1}\u{201d}", k.label(), v))
            .collect();
        self.empty_summary.set_text(&if parts.is_empty() {
            t!("No substitution.").to_string()
        } else {
            t!("Substitutions: {0}", parts.join(", "))
        });
    }

    /// Shows the current profile in the widgets.
    fn load(self: &Rc<Self>) {
        let p = self.current();
        self.loading.set(true);
        match p.mode {
            Mode::Move => self.mode_move.set_active(true),
            Mode::Copy => self.mode_copy.set_active(true),
            Mode::Simulate => self.mode_sim.set_active(true),
        }
        self.copy_add.set_active(p.copy_add_to_library);
        self.base_folder.set_text(&p.base_folder);
        self.use_folder.set_active(p.use_folder);
        self.folder_tpl.set_text(&p.folder_template);
        self.use_file.set_active(p.use_file_name);
        self.file_tpl.set_text(&p.file_template);
        self.auto_space.set_active(p.auto_space_fields);
        self.empty_folder.set_text(&p.empty_folder);
        *self.empty_data.borrow_mut() = p.empty_data.clone();
        self.empty_value.set_text(
            p.empty_data
                .get(&self.empty_key())
                .map(String::as_str)
                .unwrap_or(""),
        );
        self.update_empty_summary();
        self.fail_empty.set_active(p.fail_empty_values);
        for (f, c) in &self.failed_checks {
            c.set_active(p.failed_fields.contains(f));
        }
        self.move_failed.set_active(p.move_failed);
        self.failed_folder.set_text(&p.failed_folder);
        let mut illegal = p.illegal_characters.clone();
        for (k, v) in DEFAULT_ILLEGAL_CHARACTERS {
            illegal
                .entry(k.to_string())
                .or_insert_with(|| v.to_string());
        }
        rebuild_illegal(self, illegal);
        for (m, e) in &self.months {
            e.set_text(p.months.get(m).map(String::as_str).unwrap_or(""));
        }
        *self.excl_folders.items.borrow_mut() = p.exclude_folders.clone();
        rebuild_strlist(self, sel_excl_folders);
        self.rules_mode.set_selected(
            ExcludeMode::ALL
                .iter()
                .position(|m| *m == p.exclude_mode)
                .unwrap_or(0) as u32,
        );
        self.rules_op.set_selected(
            GroupOp::ALL
                .iter()
                .position(|m| *m == p.exclude_rules.operator)
                .unwrap_or(0) as u32,
        );
        let mut rules = Vec::new();
        let mut groups = Vec::new();
        for item in &p.exclude_rules.items {
            match item {
                RuleItem::Rule {
                    field,
                    operator,
                    value,
                } => rules.push((*field, *operator, value.clone())),
                RuleItem::Group(_) => groups.push(item.clone()),
            }
        }
        rebuild_rules(self, rules);
        self.groups_note.set_text(&if groups.is_empty() {
            String::new()
        } else {
            t!(
                "{0} nested rule group(s) kept (not editable here).",
                groups.len()
            )
        });
        *self.kept_groups.borrow_mut() = groups;
        self.remove_empty.set_active(p.remove_empty_folders);
        *self.excl_empty.items.borrow_mut() = p.excluded_empty_folders.clone();
        rebuild_strlist(self, sel_excl_empty);
        self.dont_ask.set_active(p.dont_ask_when_multi_one);
        self.replace_spaces.set_active(p.replace_multiple_spaces);
        self.copy_read.set_active(p.copy_read_percentage);
        self.loading.set(false);
        self.update_sensitivity();
        self.refresh_preview();
    }

    fn refresh_profiles(self: &Rc<Self>) {
        self.loading.set(true);
        let names: Vec<String> = self
            .profiles
            .borrow()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        self.profile_model
            .splice(0, self.profile_model.n_items(), &refs);
        self.profile_dd.set_selected(self.cur.get() as u32);
        self.loading.set(false);
        rebuild_run_list(self);
    }

    fn add_profile(self: &Rc<Self>, p: Profile) {
        self.run_list.borrow_mut().push((p.name.clone(), false));
        self.profiles.borrow_mut().push(p);
        self.cur.set(self.profiles.borrow().len() - 1);
        self.refresh_profiles();
        self.load();
    }

    fn ask_name(&self, title: &str, initial: &str, on_ok: impl Fn(String) + 'static) {
        let Some(parent) = self.win.upgrade() else {
            return;
        };
        let taken: Vec<String> = self
            .profiles
            .borrow()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let (win, content, buttons) = widgets::dialog(&parent, title, 420, 120);
        let entry = widgets::entry(initial);
        content.append(&label(t!("Profile name:")));
        content.append(&entry);
        let cancel = widgets::button(t!("Cancel"), false);
        let ok = widgets::button("OK", true);
        cancel.connect_clicked(glib::clone!(
            #[weak]
            win,
            move |_| win.close()
        ));
        let validate = glib::clone!(
            #[weak]
            win,
            #[weak]
            entry,
            move || {
                let name = entry.text().trim().to_string();
                if name.is_empty() {
                    widgets::alert(&win, t!("Invalid name"), t!("Enter a name."));
                } else if taken.contains(&name) {
                    widgets::alert(
                        &win,
                        t!("Name already used"),
                        t!("This profile name is already used."),
                    );
                } else {
                    win.close();
                    on_ok(name);
                }
            }
        );
        let validate = Rc::new(validate);
        let v = validate.clone();
        ok.connect_clicked(move |_| v());
        entry.connect_activate(move |_| validate());
        buttons.append(&cancel);
        buttons.append(&ok);
        win.present();
    }

    fn import(self: &Rc<Self>) {
        let Some(win) = self.win.upgrade() else {
            return;
        };
        let w = Rc::downgrade(self);
        let parent = win.clone();
        widgets::choose_file(&win, t!("Import profiles"), move |path| {
            let Some(e) = w.upgrade() else { return };
            let data = std::fs::read_to_string(&path).unwrap_or_default();
            let imported: Vec<Profile> = serde_json::from_str::<Vec<Profile>>(&data)
                .or_else(|_| serde_json::from_str::<Profile>(&data).map(|p| vec![p]))
                .or_else(|_| serde_json::from_str::<OrganizerSettings>(&data).map(|s| s.profiles))
                .unwrap_or_default();
            if imported.is_empty() {
                widgets::alert(
                    &parent,
                    t!("Import failed"),
                    t!("This file does not contain any organization profile."),
                );
                return;
            }
            let n = imported.len();
            for mut p in imported {
                let base = p.name.clone();
                let mut i = 2;
                while e.profiles.borrow().iter().any(|x| x.name == p.name) {
                    p.name = format!("{base} ({i})");
                    i += 1;
                }
                e.run_list.borrow_mut().push((p.name.clone(), false));
                e.profiles.borrow_mut().push(p);
            }
            e.refresh_profiles();
            widgets::alert(
                &parent,
                t!("Profiles imported"),
                &t!("{0} profile(s) imported.", n),
            );
        });
    }

    fn export(&self, all: bool) {
        let Some(win) = self.win.upgrade() else {
            return;
        };
        let list = if all {
            self.profiles.borrow().clone()
        } else {
            vec![self.current()]
        };
        let name = if all {
            t!("library-organizer-profiles").to_string()
        } else {
            list[0].name.clone()
        };
        let dlg = gtk::FileDialog::builder()
            .title(t!("Export profiles"))
            .initial_name(format!("{name}.json"))
            .modal(true)
            .build();
        let parent = win.clone();
        dlg.save(Some(&win), gio::Cancellable::NONE, move |r| {
            let Ok(file) = r else { return };
            let Some(path) = file.path() else { return };
            let res = serde_json::to_string_pretty(&list)
                .map_err(anyhow::Error::from)
                .and_then(|s| std::fs::write(&path, s).map_err(anyhow::Error::from));
            if let Err(e) = res {
                widgets::alert(&parent, t!("Error"), &format!("{e:#}"));
            }
        });
    }

    /// Profiles to run, in order (the current one when none is checked).
    fn run_profiles(&self) -> Vec<Profile> {
        let profiles = self.profiles.borrow();
        let list: Vec<Profile> = self
            .run_list
            .borrow()
            .iter()
            .filter(|(_, on)| *on)
            .filter_map(|(n, _)| profiles.iter().find(|p| p.name == *n).cloned())
            .collect();
        if list.is_empty() {
            vec![self.current()]
        } else {
            list
        }
    }

    fn save_settings(&self) -> bool {
        let last_used: Vec<String> = self.run_profiles().into_iter().map(|p| p.name).collect();
        let mut s = self.state.settings.borrow_mut();
        s.organizer = OrganizerSettings {
            profiles: self.profiles.borrow().clone(),
            last_used,
        };
        if let Err(e) = s.save() {
            if let Some(w) = self.win.upgrade() {
                widgets::alert(&w, t!("Error"), &format!("{e:#}"));
            }
            return false;
        }
        true
    }

    fn organize(&self) {
        let Some(win) = self.win.upgrade() else {
            return;
        };
        let profiles = self.run_profiles();
        if let Some(err) = validation_errors(&profiles) {
            widgets::alert(&win, t!("Incompatible settings"), &err);
            return;
        }
        let books = self.state.selected_books();
        if books.is_empty() {
            widgets::alert(
                &win,
                t!("No book selected"),
                t!("Select the books to organize in the list."),
            );
            return;
        }
        if !self.save_settings() {
            return;
        }
        let state = self.state.clone();
        win.close();
        launch(&state, books, profiles);
    }
}

fn sel_excl_folders(e: &Editor) -> &StrList {
    &e.excl_folders
}

fn sel_excl_empty(e: &Editor) -> &StrList {
    &e.excl_empty
}

fn rebuild_strlist(ed: &Rc<Editor>, sel: fn(&Editor) -> &StrList) {
    let l = sel(ed);
    l.list.remove_all();
    for (i, s) in l.items.borrow().iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.set_margin_start(6);
        row.set_margin_end(6);
        row.append(
            &gtk::Label::builder()
                .label(s)
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .build(),
        );
        let del = gtk::Button::from_icon_name("list-remove-symbolic");
        del.add_css_class("flat");
        del.set_tooltip_text(Some(t!("Remove from the list")));
        let f = weak_fn(ed, move |e| {
            sel(e).items.borrow_mut().remove(i);
            rebuild_strlist(e, sel);
            e.changed();
        });
        del.connect_clicked(move |_| f());
        row.append(&del);
        l.list.append(&row);
    }
}

fn rebuild_rules(ed: &Rc<Editor>, items: Vec<(OrgField, RuleOp, String)>) {
    ed.rules_box.remove_all();
    let labels = rule_field_labels();
    let ops: Vec<&str> = RuleOp::ALL.iter().map(|o| o.label()).collect();
    let mut rows = Vec::new();
    for (i, (field, op, value)) in items.into_iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.set_margin_start(6);
        row.set_margin_end(6);
        let fdd = widgets::dropdown(
            &labels,
            RULE_FIELDS.iter().position(|f| *f == field).unwrap_or(0),
        );
        let odd = widgets::dropdown(&ops, RuleOp::ALL.iter().position(|o| *o == op).unwrap_or(0));
        let entry = widgets::entry(&value);
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some(&yes_no_placeholder(field)));
        let del = gtk::Button::from_icon_name("list-remove-symbolic");
        del.add_css_class("flat");
        del.set_tooltip_text(Some(t!("Remove this rule")));
        let f = weak_fn(ed, move |e| e.changed());
        odd.connect_selected_notify(move |_| f());
        let f = weak_fn(ed, move |e| e.changed());
        entry.connect_changed(move |_| f());
        let f = weak_fn(ed, move |e| {
            if let Some(r) = e.rules.borrow().get(i) {
                let field = RULE_FIELDS
                    .get(r.field.selected() as usize)
                    .copied()
                    .unwrap_or(OrgField::Book(Field::Series));
                r.value
                    .set_placeholder_text(Some(&yes_no_placeholder(field)));
            }
            e.changed();
        });
        fdd.connect_selected_notify(move |_| f());
        let f = weak_fn(ed, move |e| {
            let mut items = e.rule_items();
            items.remove(i);
            rebuild_rules(e, items);
            e.changed();
        });
        del.connect_clicked(move |_| f());
        row.append(&fdd);
        row.append(&odd);
        row.append(&entry);
        row.append(&del);
        ed.rules_box.append(&row);
        rows.push(RuleRow {
            field: fdd,
            op: odd,
            value: entry,
        });
    }
    *ed.rules.borrow_mut() = rows;
}

fn rebuild_illegal(ed: &Rc<Editor>, map: BTreeMap<String, String>) {
    ed.illegal_box.remove_all();
    let mut rows = Vec::new();
    for (k, v) in map {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.set_margin_start(6);
        row.set_margin_end(6);
        row.append(
            &gtk::Label::builder()
                .label(t!("\u{201c}{0}\u{201d}", k))
                .width_chars(6)
                .build(),
        );
        row.append(&gtk::Label::new(Some("→")));
        let entry = widgets::entry(&v);
        entry.set_hexpand(true);
        let f = weak_fn(ed, |e| e.changed());
        entry.connect_changed(move |en| {
            let t = en.text();
            if t.contains(FORBIDDEN_IN_REPLACEMENT) {
                let clean: String = t
                    .chars()
                    .filter(|c| !FORBIDDEN_IN_REPLACEMENT.contains(c))
                    .collect();
                en.set_text(&clean);
                return;
            }
            f();
        });
        let del = gtk::Button::from_icon_name("list-remove-symbolic");
        del.add_css_class("flat");
        let essential = DEFAULT_ILLEGAL_CHARACTERS.iter().any(|(c, _)| *c == k);
        del.set_sensitive(!essential);
        let key = k.clone();
        let f = weak_fn(ed, move |e| {
            let mut m = e.illegal_map();
            m.remove(&key);
            rebuild_illegal(e, m);
            e.changed();
        });
        del.connect_clicked(move |_| f());
        row.append(&entry);
        row.append(&del);
        ed.illegal_box.append(&row);
        rows.push((k, entry));
    }
    *ed.illegal.borrow_mut() = rows;

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.set_margin_start(6);
    row.set_margin_end(6);
    let new_char = gtk::Entry::builder()
        .max_length(1)
        .width_chars(3)
        .placeholder_text(t!("char."))
        .build();
    let add = gtk::Button::with_label(t!("Add"));
    let f = weak_fn(ed, {
        let new_char = new_char.clone();
        move |e| {
            let c = new_char.text().to_string();
            let mut m = e.illegal_map();
            if c.is_empty() || m.contains_key(&c) {
                return;
            }
            m.insert(c, String::new());
            rebuild_illegal(e, m);
            e.changed();
        }
    });
    add.connect_clicked(move |_| f());
    row.append(&gtk::Label::new(Some(t!("New character:"))));
    row.append(&new_char);
    row.append(&add);
    ed.illegal_box.append(&row);
}

fn rebuild_run_list(ed: &Rc<Editor>) {
    ed.run_box.remove_all();
    let n = ed.run_list.borrow().len();
    for (i, (name, on)) in ed.run_list.borrow().iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.set_margin_start(6);
        row.set_margin_end(6);
        let check = widgets::check(name, *on);
        check.set_hexpand(true);
        let list = ed.run_list.clone();
        check.connect_toggled(move |c| {
            if let Some(e) = list.borrow_mut().get_mut(i) {
                e.1 = c.is_active();
            }
        });
        row.append(&check);
        for (icon, up) in [("go-up-symbolic", true), ("go-down-symbolic", false)] {
            let b = gtk::Button::from_icon_name(icon);
            b.add_css_class("flat");
            b.set_sensitive(if up { i > 0 } else { i + 1 < n });
            let f = weak_fn(ed, move |e| {
                let j = if up { i - 1 } else { i + 1 };
                e.run_list.borrow_mut().swap(i, j);
                rebuild_run_list(e);
            });
            b.connect_clicked(move |_| f());
            row.append(&b);
        }
        ed.run_box.append(&row);
    }
}
