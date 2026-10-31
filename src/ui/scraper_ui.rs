//! "Bedetheque Scraper 2" user interface: full scrape, QuickScrape from a
//! link, settings dialog and choice window (series/album/edition).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use super::widgets::{self, Form};
use super::{JobCtx, JobKind, State, run_job};
use crate::bedetheque::{Ambiguity, Candidate, ChoiceKind, Chooser, ScrapeOutcome, Scraper, ScraperSettings};
use crate::model::{Book, Field};

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// "Bedetheque Scraper 2" on the selection.
pub fn scrape(state: &State) {
    let Some(books) = state.require_selection() else { return };
    let settings = state.settings.borrow().scraper.clone();
    let items: Vec<(Book, Option<String>)> = books.into_iter().map(|b| (b, None)).collect();
    if settings.rescrape {
        let weak = Rc::downgrade(state);
        widgets::confirm(
            &state.window,
            t!("Rescraping…"),
            t!("You are running a rescrape: albums without a web link will be skipped. Continue?"),
            t!("Rescrape"),
            move || {
                if let Some(st) = weak.upgrade() {
                    run_scrape_job(&st, t!("Fetching the info again from the Web links…"), items, settings);
                }
            },
        );
    } else {
        run_scrape_job(state, t!("Fetching the info from Bedetheque…"), items, settings);
    }
}

/// Fetch from a link: a bedetheque.com link is asked for each book.
pub fn quick_scrape(state: &State) {
    let Some(books) = state.require_selection() else { return };
    ask_urls(state, books, 0, Vec::new());
}

/// Reads the settings back from the widgets of the Bedetheque settings tab.
pub type ReadScraperSettings = Box<dyn Fn() -> ScraperSettings>;

/// "Bedetheque" tab of the settings dialog: its widgets, and a function
/// returning the edited settings.
pub fn settings_page(current: ScraperSettings) -> (gtk::Notebook, ReadScraperSettings) {
    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);

    // -- Data --------------------------------------------------------------
    let data = gtk::Box::new(gtk::Orientation::Vertical, 8);
    data.set_margin_start(8);
    data.set_margin_end(8);
    data.set_margin_top(8);
    let intro = gtk::Label::builder()
        .label(t!("Fields to retrieve (unchecked fields are never modified):"))
        .xalign(0.0)
        .wrap(true)
        .build();
    data.append(&intro);
    let grid = gtk::Grid::builder().row_spacing(2).column_spacing(24).build();
    let mut field_checks = Vec::new();
    for (i, (label, get)) in FIELD_FLAGS.iter().enumerate() {
        let mut tmp = current.clone();
        let c = widgets::check(crate::i18n::tr(label), *get(&mut tmp));
        grid.attach(&c, (i % 2) as i32, (i / 2) as i32, 1, 1);
        field_checks.push(c);
    }
    data.append(&grid);
    let all_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let check_all = widgets::button(t!("Check all"), false);
    let check_none = widgets::button(t!("Uncheck all"), false);
    for (btn, value) in [(&check_all, true), (&check_none, false)] {
        let checks = field_checks.clone();
        btn.connect_clicked(move |_| checks.iter().for_each(|c| c.set_active(value)));
    }
    all_row.append(&check_all);
    all_row.append(&check_none);
    data.append(&all_row);

    let mut form = Form::new();
    form.add_title(t!("Options"));
    let tags = widgets::entry(&current.tags);
    tags.set_tooltip_text(Some(t!("Empty: tags unchanged; \"DEL\": tags cleared; otherwise they are replaced by this text.")));
    form.add(t!("Album tags"), &tags);
    let subpatt = widgets::entry(&current.subpatt);
    form.add(t!("Series name ending to remove"), &subpatt);
    let pad = widgets::spin(0.0, 9.0, 1.0, current.pad_number as f64);
    pad.set_tooltip_text(Some(t!("0: numbers unchanged")));
    form.add(t!("Number length (padded with zeros)"), &pad);
    let one_shot = widgets::check(t!("Replace the album format with \"One Shot\" when applicable"), current.one_shot_format);
    form.add_full(&one_shot);
    let short_web = widgets::check(t!("Use short web links for albums"), current.short_web_link);
    form.add_full(&short_web);
    let count_of = widgets::check(t!("Total album count based on volume numbering"), current.count_of);
    form.add_full(&count_of);
    let count_complete = widgets::check(
        t!("Total album count only if the series is finished"),
        current.count_only_if_complete,
    );
    form.add_full(&count_complete);
    let resume_everywhere = widgets::check(
        t!("Series summary on every album (otherwise on volume 1 only)"),
        current.serie_resume_everywhere,
    );
    form.add_full(&resume_everywhere);
    let generic = widgets::check(
        t!("Accept generic artist values (<Collectif>, <Indéterminé>…)"),
        current.accept_generic_artists,
    );
    form.add_full(&generic);
    data.append(&form.grid);
    notebook.append_page(&widgets::scrolled(&data), Some(&gtk::Label::new(Some(t!("Data")))));

    // -- Settings ----------------------------------------------------------
    let mut form = Form::new();
    form.grid.set_margin_start(8);
    form.grid.set_margin_end(8);
    form.grid.set_margin_top(8);
    let lang = widgets::dropdown(&["Français", "English"], if current.lang_fr { 0 } else { 1 });
    form.add(t!("Language of added texts"), &lang);
    let articles = widgets::entry(&current.articles);
    articles.set_tooltip_text(Some(t!("Comma-separated articles")));
    form.add(t!("Main articles"), &articles);
    let titleit = widgets::check(t!("\"Pascal Case\" convention for series and album names"), current.titleit);
    form.add_full(&titleit);
    let format_articles = widgets::check(
        t!("Move the main articles of series names to the end"),
        current.format_articles,
    );
    form.add_full(&format_articles);
    let auto_edition = widgets::check(t!("Automatically choose the album edition"), current.auto_choose_edition);
    form.add_full(&auto_edition);
    let always_serie = widgets::check(t!("Always choose series manually"), current.always_choose_serie);
    form.add_full(&always_serie);
    let max_books = widgets::spin(0.0, 100_000.0, 1.0, current.max_books as f64);
    max_books.set_tooltip_text(Some(t!("0: no limit")));
    form.add(t!("Maximum number of albums to process"), &max_books);
    let delay = widgets::spin(0.0, 600.0, 1.0, current.delay_secs as f64);
    form.add(t!("Pause between 2 albums (s)"), &delay);
    form.add_title(t!("Decision when a series or album choice is ambiguous"));
    let ambiguity = widgets::dropdown(
        &[
            t!("Skip and move on"),
            t!("Ask the user to choose"),
            t!("Ask, with automatic closing"),
        ],
        match current.ambiguity {
            Ambiguity::Skip => 0,
            Ambiguity::Ask => 1,
            Ambiguity::AskWithTimeout => 2,
        },
    );
    form.add(t!("Ambiguity"), &ambiguity);
    let popup = widgets::spin(1.0, 3600.0, 1.0, current.popup_timeout_secs as f64);
    popup.set_sensitive(current.ambiguity == Ambiguity::AskWithTimeout);
    form.add(t!("Automatically close the choice after (s)"), &popup);
    ambiguity.connect_selected_notify(glib::clone!(
        #[weak]
        popup,
        move |d| popup.set_sensitive(d.selected() == 2)
    ));
    form.add_title(t!("Rescrape"));
    let rescrape = widgets::check(
        t!("Run a rescrape from the albums' web links"),
        current.rescrape,
    );
    form.add_full(&rescrape);
    notebook.append_page(&widgets::scrolled(&form.grid), Some(&gtk::Label::new(Some(t!("Settings")))));

    // -- Log ----------------------------------------------------------------
    let mut form = Form::new();
    form.grid.set_margin_start(8);
    form.grid.set_margin_top(8);
    let debug = widgets::check(
        t!("Detailed log (requests and extracted values, shown after processing)"),
        current.debug_log,
    );
    form.add_full(&debug);
    notebook.append_page(&form.grid, Some(&gtk::Label::new(Some(t!("Log")))));

    let read = move || {
        let mut s = current.clone();
        for ((_, get), c) in FIELD_FLAGS.iter().zip(&field_checks) {
            *get(&mut s) = c.is_active();
        }
        s.tags = tags.text().to_string();
        s.subpatt = subpatt.text().to_string();
        s.pad_number = pad.value_as_int().max(0) as u32;
        s.one_shot_format = one_shot.is_active();
        s.short_web_link = short_web.is_active();
        s.count_of = count_of.is_active();
        s.count_only_if_complete = count_complete.is_active();
        s.serie_resume_everywhere = resume_everywhere.is_active();
        s.accept_generic_artists = generic.is_active();
        s.lang_fr = lang.selected() == 0;
        s.articles = articles.text().to_string();
        s.titleit = titleit.is_active();
        s.format_articles = format_articles.is_active();
        s.auto_choose_edition = auto_edition.is_active();
        s.always_choose_serie = always_serie.is_active();
        s.max_books = max_books.value_as_int().max(0) as u32;
        s.delay_secs = delay.value_as_int().max(0) as u32;
        s.ambiguity = match ambiguity.selected() {
            0 => Ambiguity::Skip,
            1 => Ambiguity::Ask,
            _ => Ambiguity::AskWithTimeout,
        };
        s.popup_timeout_secs = popup.value_as_int().max(1) as u32;
        s.rescrape = rescrape.is_active();
        s.debug_log = debug.is_active();
        s
    };
    (notebook, Box::new(read))
}

type FlagAccessor = fn(&mut ScraperSettings) -> &mut bool;

/// Checkboxes of the retrieved fields (plugin labels, translated at display time).
const FIELD_FLAGS: [(&str, FlagAccessor); 21] = [
    ("Series name", |s| &mut s.series),
    ("Album title", |s| &mut s.title),
    ("Writer(s)", |s| &mut s.writer),
    ("Penciller(s)", |s| &mut s.penciller),
    ("Colorist(s)", |s| &mut s.colorist),
    ("Letterer(s)", |s| &mut s.letterer),
    ("Inker(s)", |s| &mut s.inker),
    ("Cover artist(s)", |s| &mut s.cover_artist),
    ("Publisher", |s| &mut s.publisher),
    ("Series language", |s| &mut s.language),
    ("Format", |s| &mut s.format),
    ("Album rating", |s| &mut s.rating),
    ("Publication date", |s| &mut s.printed),
    ("Imprint (collection)", |s| &mut s.imprint),
    ("Series and album summary", |s| &mut s.synopsis),
    ("Total number of albums in the series", |s| &mut s.count),
    ("Notes (annotations)", |s| &mut s.notes),
    ("Series complete", |s| &mut s.status),
    ("Album web link", |s| &mut s.web),
    ("Series genre", |s| &mut s.genre),
    ("ISBN/EAN number", |s| &mut s.isbn),
];

// ---------------------------------------------------------------------------
// Processing
// ---------------------------------------------------------------------------

/// Choices asked from the worker thread: the dialog is shown on the GTK
/// thread and the worker waits for the answer.
struct JobChooser {
    ctx: JobCtx,
}

impl Chooser for JobChooser {
    fn choose(&self, kind: ChoiceKind, book_caption: &str, candidates: &[Candidate], timeout_secs: Option<u32>) -> Option<usize> {
        if self.ctx.cancelled() {
            return None;
        }
        let caption = book_caption.to_string();
        let candidates = candidates.to_vec();
        self.ctx
            .ask(move |st, reply| choice_dialog(st, kind, &caption, candidates, timeout_secs, reply))
            .flatten()
    }
}

struct JobResult {
    updated: Vec<u64>,
    skipped: Vec<String>,
    errors: Vec<String>,
    log: Vec<String>,
    cancelled: bool,
    not_processed: usize,
}

/// Starts scraping `items` (book, optional QuickScrape link).
fn run_scrape_job(state: &State, title: &str, items: Vec<(Book, Option<String>)>, settings: ScraperSettings) {
    let debug_log = settings.debug_log;
    run_job(
        state,
        JobKind::Scrape,
        Vec::new(),
        title,
        move |ctx| {
            let mut res = JobResult {
                updated: Vec::new(),
                skipped: Vec::new(),
                errors: Vec::new(),
                log: Vec::new(),
                cancelled: false,
                not_processed: 0,
            };
            let mut scraper = match Scraper::new(settings.clone()) {
                Ok(s) => s,
                Err(e) => {
                    res.errors.push(format!("{e:#}"));
                    return res;
                }
            };
            scraper.set_single_book(items.len() == 1);
            let chooser = JobChooser { ctx: ctx.clone() };
            let limit = if settings.max_books == 0 { usize::MAX } else { settings.max_books as usize };
            let total = items.len().min(limit);
            res.not_processed = items.len() - total;
            for (i, (book, url)) in items.into_iter().take(total).enumerate() {
                if ctx.cancelled() {
                    res.cancelled = true;
                    res.not_processed += total - i;
                    break;
                }
                let caption = book.with_proposed().caption();
                ctx.progress(i, total, format!("{}/{total} {caption}", i + 1));
                let input = book.with_proposed();
                let outcome = match &url {
                    Some(u) => scraper.scrape_url(&input, u, &chooser),
                    None => scraper.scrape(&input, &chooser),
                };
                match outcome {
                    Ok(ScrapeOutcome::Updated(out)) => {
                        res.updated.push(book.id);
                        ctx.ui(move |st| update_book_live(st, book.id, &input, &out));
                    }
                    Ok(ScrapeOutcome::Skipped(reason)) => res.skipped.push(t!("{0}: {1}", caption, reason)),
                    Err(e) => res.errors.push(t!("{0}: {1}", caption, format!("{e:#}"))),
                }
                if i + 1 < total && settings.delay_secs > 0 && !ctx.cancelled() {
                    ctx.progress(i + 1, total, t!("pausing for {0} s…", settings.delay_secs));
                    ctx.sleep(settings.delay_secs);
                }
            }
            res.log = scraper.log().to_vec();
            res
        },
        move |st, res| {
            let mut msg = t!(
                "Bedetheque: {0} book(s) updated, {1} skipped",
                res.updated.len(),
                res.skipped.len()
            );
            if !res.errors.is_empty() {
                msg.push_str(&t!(", {0} error(s)", res.errors.len()));
            }
            if res.not_processed > 0 {
                msg.push_str(&t!(", {0} not processed", res.not_processed));
            }
            if res.cancelled {
                msg.push_str(t!(" (cancelled)"));
            }
            st.set_status(&msg);
            st.journal.info(&msg);
            for e in &res.errors {
                st.journal.error(e);
            }
            for l in &res.skipped {
                st.journal.info(&t!("Skipped: {0}", l));
            }
            for l in &res.log {
                st.journal.info(l);
            }

            // Automatic file writing, deferred during processing.
            let write = {
                let s = st.settings.borrow();
                s.auto_update_files && s.allow_write_info
            };
            if write && !res.updated.is_empty() {
                let books: Vec<Book> = {
                    let lib = st.library.borrow();
                    res.updated.iter().filter_map(|id| lib.get(*id).cloned()).collect()
                };
                super::write_files(st, books);
            }

            if !res.skipped.is_empty() || !res.errors.is_empty() || debug_log {
                let mut text = msg.clone();
                if !res.errors.is_empty() {
                    text.push_str(t!("\n\nErrors:\n"));
                    text.push_str(&res.errors.join("\n"));
                }
                if !res.skipped.is_empty() {
                    text.push_str(t!("\n\nSkipped:\n"));
                    text.push_str(&res.skipped.join("\n"));
                }
                if !res.log.is_empty() {
                    text.push_str(t!("\n\nLog:\n"));
                    text.push_str(&res.log.join("\n"));
                }
                widgets::show_log(&st.window, t!("Bedetheque — report"), &text);
            }
        },
    );
}

/// Updates a book in the list during processing (automatic file writing is
/// done at the end of processing).
fn update_book_live(st: &State, id: u64, input: &Book, out: &Book) {
    // Applied to the book as it is now: other operations may have changed it
    // since.
    let Some(b) = st.library.borrow().get(id).map(|real| apply_changes(real, input, out)) else { return };
    st.replace_books(vec![b], true, false);
}

/// Copies onto the real book the fields changed by the scraper (which was
/// given the book completed with the values proposed from the file name).
fn apply_changes(real: &Book, input: &Book, out: &Book) -> Book {
    let mut b = real.clone();
    for f in Field::ALL.iter().copied().filter(|f| f.is_editable()) {
        let new = f.get(out);
        let changed = new != f.get(input);
        // Like ComicRack: the proposed values (series, number) are stored
        // when the book is scraped.
        let fill_proposed = matches!(f, Field::Series | Field::Number) && f.get(real).is_empty() && !new.is_empty();
        if changed || fill_proposed {
            f.set(&mut b, &new);
        }
    }
    b
}

// ---------------------------------------------------------------------------
// QuickScrape: link input
// ---------------------------------------------------------------------------

fn ask_urls(state: &State, books: Vec<Book>, index: usize, mut acc: Vec<(Book, Option<String>)>) {
    if index >= books.len() {
        if !acc.is_empty() {
            let settings = state.settings.borrow().scraper.clone();
            run_scrape_job(state, t!("Fetching the info from Bedetheque links…"), acc, settings);
        }
        return;
    }
    let book = books[index].clone();
    let (win, content, buttons) = widgets::dialog(&state.window, t!("Fetch the info from a Bedetheque link"), 680, 170);
    let caption = gtk::Label::builder().xalign(0.0).wrap(true).build();
    caption.set_markup(&format!(
        "<b>{}</b>  <span alpha=\"60%\">({}/{})</span>",
        glib::markup_escape_text(&book.with_proposed().caption()),
        index + 1,
        books.len()
    ));
    content.append(&caption);
    content.append(
        &gtk::Label::builder()
            .label(t!("Paste the web link of the album (or of the series, or of the magazine) from www.bedetheque.com:"))
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    let prefill = if book.info.web.contains("bedetheque.com") { book.info.web.clone() } else { String::new() };
    let entry = widgets::entry(&prefill);
    entry.set_placeholder_text(Some("https://www.bedetheque.com/BD-…html"));
    content.append(&entry);

    let stop = widgets::button(t!("Stop"), false);
    stop.set_tooltip_text(Some(t!("Stop asking for links (books already given a link are processed)")));
    let skip = widgets::button(t!("Skip this book"), false);
    let ok = widgets::button(t!("Scrape"), true);
    buttons.append(&stop);
    buttons.append(&skip);
    buttons.append(&ok);

    // Chosen action: None = close/stop, Some(None) = skip, Some(Some(url)).
    let choice: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
    let books = Rc::new(books);
    let acc = Rc::new(RefCell::new(std::mem::take(&mut acc)));

    let submit = Rc::new(glib::clone!(
        #[weak]
        win,
        #[weak]
        entry,
        #[strong]
        choice,
        move || {
            let url = entry.text().trim().to_string();
            choice.replace(Some((!url.is_empty()).then_some(url)));
            win.close();
        }
    ));
    ok.connect_clicked(glib::clone!(
        #[strong]
        submit,
        move |_| submit()
    ));
    entry.connect_activate(move |_| submit());
    skip.connect_clicked(glib::clone!(
        #[weak]
        win,
        #[strong]
        choice,
        move |_| {
            choice.replace(Some(None));
            win.close();
        }
    ));
    stop.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    win.connect_close_request(glib::clone!(
        #[weak]
        state,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_| {
            let books = (*books).clone();
            let mut acc = std::mem::take(&mut *acc.borrow_mut());
            match choice.borrow_mut().take() {
                Some(url) => {
                    if let Some(u) = url {
                        acc.push((book.clone(), Some(u)));
                    }
                    // The next dialog is opened after this one is closed.
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        state,
                        move || ask_urls(&state, books, index + 1, acc)
                    ));
                }
                None => {
                    if !acc.is_empty() {
                        let settings = state.settings.borrow().scraper.clone();
                        glib::idle_add_local_once(glib::clone!(
                            #[weak]
                            state,
                            move || run_scrape_job(&state, t!("Fetching the info from Bedetheque links…"), acc, settings)
                        ));
                    }
                }
            }
            glib::Propagation::Proceed
        }
    ));
    win.present();
    entry.grab_focus();
}

// ---------------------------------------------------------------------------
// Choice dialog (series / album / edition)
// ---------------------------------------------------------------------------

type Reply = Box<dyn FnOnce(Option<usize>) + Send>;

fn choice_dialog(
    st: &State,
    kind: ChoiceKind,
    caption: &str,
    candidates: Vec<Candidate>,
    timeout_secs: Option<u32>,
    reply: Reply,
) {
    let title = match kind {
        ChoiceKind::Series => t!("Choose a series"),
        ChoiceKind::Album => t!("Choose an album"),
        ChoiceKind::Edition => t!("Choose an edition"),
    };
    let (win, content, buttons) = widgets::dialog(&st.window, &t!("{0} for {1}", title, caption), 620, 460);
    let head = gtk::Label::builder().xalign(0.0).wrap(true).build();
    head.set_markup(&t!(
        "{0} for <b>{1}</b> ({2} candidate(s)). Double-click: open the page in the browser.",
        glib::markup_escape_text(title),
        glib::markup_escape_text(caption),
        candidates.len()
    ));
    content.append(&head);
    let filter = gtk::SearchEntry::builder().placeholder_text(t!("Filter…")).build();
    content.append(&filter);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    content.append(&widgets::scrolled(&list));
    let countdown = gtk::Label::builder().xalign(0.0).visible(timeout_secs.is_some()).build();
    countdown.add_css_class("dim-label");
    content.append(&countdown);

    let skip_label = if kind == ChoiceKind::Edition { t!("First edition") } else { t!("Skip this book") };
    let cancel = widgets::button(skip_label, false);
    let ok = widgets::button(t!("Choose"), true);
    buttons.append(&cancel);
    buttons.append(&ok);

    let candidates = Rc::new(candidates);
    // Index of the candidate shown on each row (after filtering).
    let shown: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let reply: Rc<RefCell<Option<Reply>>> = Rc::new(RefCell::new(Some(reply)));
    let timer_on = Rc::new(Cell::new(timeout_secs.is_some()));

    // Replies only once, then closes the window.
    let finish: Rc<dyn Fn(Option<usize>)> = Rc::new(glib::clone!(
        #[weak]
        win,
        #[strong]
        reply,
        move |v| {
            if let Some(r) = reply.borrow_mut().take() {
                r(v);
            }
            win.close();
        }
    ));

    let fill = Rc::new(glib::clone!(
        #[weak]
        list,
        #[strong]
        candidates,
        #[strong]
        shown,
        move |text: &str| {
            while let Some(row) = list.row_at_index(0) {
                list.remove(&row);
            }
            let text = text.to_lowercase();
            let mut idx = Vec::new();
            for (i, c) in candidates.iter().enumerate() {
                let display = format!("({:03}) - {} {}", i + 1, c.label, c.detail);
                if !text.is_empty() && !display.to_lowercase().contains(&text) {
                    continue;
                }
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.set_margin_top(3);
                row.set_margin_bottom(3);
                let num = gtk::Label::new(Some(&format!("{:03}", i + 1)));
                num.add_css_class("dim-label");
                let label = gtk::Label::builder()
                    .label(&c.label)
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .tooltip_text(&c.url)
                    .build();
                row.append(&num);
                row.append(&label);
                if !c.detail.is_empty() {
                    let detail = gtk::Label::new(Some(&c.detail));
                    detail.add_css_class("dim-label");
                    row.append(&detail);
                }
                list.append(&row);
                idx.push(i);
            }
            shown.replace(idx);
            if let Some(r) = list.row_at_index(0) {
                list.select_row(Some(&r));
            }
        }
    ));
    fill("");

    filter.connect_search_changed(glib::clone!(
        #[strong]
        fill,
        #[strong]
        timer_on,
        move |e| {
            timer_on.set(false);
            fill(&e.text());
        }
    ));
    let selected_index = glib::clone!(
        #[weak]
        list,
        #[strong]
        shown,
        #[upgrade_or]
        None,
        move || {
            list.selected_row()
                .and_then(|r| shown.borrow().get(r.index() as usize).copied())
        }
    );
    ok.connect_clicked(glib::clone!(
        #[strong]
        finish,
        move |_| {
            if let Some(i) = selected_index() {
                finish(Some(i));
            }
        }
    ));
    cancel.connect_clicked(glib::clone!(
        #[strong]
        finish,
        move |_| finish(None)
    ));
    list.connect_row_activated(glib::clone!(
        #[weak]
        win,
        #[strong]
        candidates,
        #[strong]
        shown,
        #[strong]
        timer_on,
        move |_, row| {
            timer_on.set(false);
            if let Some(c) = shown.borrow().get(row.index() as usize).and_then(|&i| candidates.get(i)) {
                gtk::UriLauncher::new(&c.url).launch(Some(&win), gio::Cancellable::NONE, |_| {});
            }
        }
    ));
    let click = gtk::GestureClick::new();
    click.connect_pressed(glib::clone!(
        #[strong]
        timer_on,
        move |_, _, _, _| timer_on.set(false)
    ));
    list.add_controller(click);
    // Closing with the window button or Escape: same as "Cancel".
    win.connect_close_request(glib::clone!(
        #[strong]
        reply,
        move |_| {
            if let Some(r) = reply.borrow_mut().take() {
                r(None);
            }
            glib::Propagation::Proceed
        }
    ));

    // Automatic closing: 1st item for an album or an edition, give up for a
    // series (like the plugin).
    if let Some(secs) = timeout_secs {
        let remaining = Rc::new(Cell::new(secs));
        let auto = if kind == ChoiceKind::Series { t!("skip this book") } else { t!("choose the 1st item") };
        let show = glib::clone!(
            #[weak]
            countdown,
            #[strong]
            remaining,
            move || countdown.set_text(&t!("Closing automatically in {0} s ({1})", remaining.get(), auto))
        );
        show();
        glib::timeout_add_seconds_local(
            1,
            glib::clone!(
                #[weak]
                countdown,
                #[strong]
                reply,
                #[strong]
                finish,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    if reply.borrow().is_none() {
                        return glib::ControlFlow::Break;
                    }
                    if !timer_on.get() {
                        countdown.set_text(t!("Automatic closing interrupted"));
                        return glib::ControlFlow::Break;
                    }
                    let left = remaining.get().saturating_sub(1);
                    remaining.set(left);
                    if left == 0 {
                        finish(if kind == ChoiceKind::Series { None } else { Some(0) });
                        return glib::ControlFlow::Break;
                    }
                    show();
                    glib::ControlFlow::Continue
                }
            ),
        );
    }
    win.present();
    list.grab_focus();
}
