//! Scraping engine for www.bedetheque.com.
//!
//! Faithful port of the ComicRack plugin "Bedetheque Scraper 2" (v6.08,
//! <https://github.com/maforget/Bedetheque-Scrapper-2>): series lookup
//! (alphabetical index, then the site's search engine), choice of the series,
//! album and edition, field extraction with regular expressions and
//! post-processing ("Last, First" inversion, articles, "Pascal" case, series
//! + album summary, etc.).
//!
//! The regular expressions are the plugin's. As the `regex` engine does not
//! support lookahead/lookbehind assertions, the few patterns that used them
//! are rewritten equivalently (see `find_series_in_index`, `clean_articles`,
//! `format_articles`).

use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Datelike, Local};
use regex::{Regex, RegexBuilder};
use reqwest::blocking::Client;
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::model::{Book, MONTHS_FR, YesNo};
use crate::util::find_program;


const BASE_URL: &str = "https://www.bedetheque.com/";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
const ACCEPT_LANGUAGE: &str = "fr-FR,fr;q=0.9,en;q=0.8";
const TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// Decision when a series or album choice is ambiguous.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ambiguity {
    /// Skip and move on.
    Skip,
    /// Ask the user to choose.
    Ask,
    /// Ask the user, closing the choice automatically after
    /// `popup_timeout_secs` seconds.
    #[default]
    AskWithTimeout,
}

/// All the plugin options (same defaults as the plugin).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ScraperSettings {
    /// Series name.
    pub series: bool,
    /// Album title.
    pub title: bool,
    /// Album writer(s) (Scénario + Storyboard).
    pub writer: bool,
    /// Album penciller(s).
    pub penciller: bool,
    /// Album colorist(s) (also sets "Black and white").
    pub colorist: bool,
    /// Album letterer(s).
    pub letterer: bool,
    /// Album inker(s).
    pub inker: bool,
    /// Album cover artist(s).
    pub cover_artist: bool,
    /// Publisher.
    pub publisher: bool,
    /// Series language.
    pub language: bool,
    /// Album format.
    pub format: bool,
    /// Album rating (community rating).
    pub rating: bool,
    /// Publication date (legal deposit, otherwise printing date).
    pub printed: bool,
    /// Album imprint (Collection).
    pub imprint: bool,
    /// Series and album summary.
    pub synopsis: bool,
    /// Total number of albums in the series.
    pub count: bool,
    /// Album notes ("Bedetheque.com - date").
    pub notes: bool,
    /// Series complete (finished / ongoing).
    pub status: bool,
    /// Album web link.
    pub web: bool,
    /// Series genre.
    pub genre: bool,
    /// Album ISBN/EAN number.
    pub isbn: bool,
    /// Album tags: empty = unchanged, "DEL" = cleared, otherwise replaced by
    /// this value.
    pub tags: String,
    /// Replace the album format with "One Shot" when applicable (this then
    /// disables format retrieval).
    pub one_shot_format: bool,
    /// Use short web links (www.bedetheque.com/BD--id.html).
    pub short_web_link: bool,
    /// Total album count based on volume numbering.
    pub count_of: bool,
    /// Total album count only if the series is finished.
    pub count_only_if_complete: bool,
    /// Series summary on every album (otherwise on volume 1 only).
    pub serie_resume_everywhere: bool,
    /// Accept generic artist values (<Collectif>, <Indéterminé>…).
    pub accept_generic_artists: bool,
    /// Series name ending to remove before searching.
    pub subpatt: String,
    /// Album number length (left-padded with zeros); 0 = unchanged.
    pub pad_number: u32,
    /// Language of the added texts (notes, "Info édition: "): French if true,
    /// English otherwise.
    pub lang_fr: bool,
    /// Main articles, comma-separated.
    pub articles: String,
    /// "Pascal Case" convention for series and album names.
    pub titleit: bool,
    /// Move the main articles of series names to the end
    /// ("Les 5 Terres" → "5 Terres (Les)").
    pub format_articles: bool,
    /// Automatically choose the album edition (otherwise, ask).
    pub auto_choose_edition: bool,
    /// Always choose series manually.
    pub always_choose_serie: bool,
    /// Maximum number of albums to process (handled by the caller).
    pub max_books: u32,
    /// Pause between 2 albums, in seconds (handled by the caller).
    pub delay_secs: u32,
    /// Decision when a choice is ambiguous.
    pub ambiguity: Ambiguity,
    /// Automatically close the choice after this many seconds
    /// (if `ambiguity == AskWithTimeout`).
    pub popup_timeout_secs: u32,
    /// Run a rescrape from the albums' web links (albums without a link are
    /// skipped).
    pub rescrape: bool,
    /// Detailed log (requests, extracted values).
    pub debug_log: bool,
}

impl Default for ScraperSettings {
    fn default() -> Self {
        ScraperSettings {
            series: true,
            title: true,
            writer: true,
            penciller: true,
            colorist: true,
            letterer: true,
            inker: true,
            cover_artist: true,
            publisher: true,
            language: true,
            format: true,
            rating: true,
            printed: true,
            imprint: true,
            synopsis: true,
            count: true,
            notes: true,
            status: true,
            web: true,
            genre: true,
            isbn: true,
            tags: String::new(),
            one_shot_format: false,
            short_web_link: false,
            count_of: true,
            count_only_if_complete: true,
            serie_resume_everywhere: true,
            accept_generic_artists: false,
            subpatt: " - - ".into(),
            pad_number: 0,
            lang_fr: true,
            articles: "Le,La,Les,L',The".into(),
            titleit: true,
            format_articles: true,
            auto_choose_edition: true,
            always_choose_serie: false,
            max_books: 1000,
            delay_secs: 7,
            ambiguity: Ambiguity::AskWithTimeout,
            popup_timeout_secs: 30,
            rescrape: false,
            debug_log: false,
        }
    }
}

impl ScraperSettings {
    /// The plugin's `titlize` (articles moved if requested, then Pascal Case).
    fn tz(&self, s: &str, format_articles_too: bool) -> String {
        let s = if format_articles_too && self.format_articles {
            format_articles(s, &self.articles)
        } else {
            s.to_string()
        };
        titlize(&s, self.titleit)
    }

    /// The plugin disables format retrieval when "One Shot" is enabled.
    fn format_enabled(&self) -> bool {
        self.format && !self.one_shot_format
    }

    fn timeout(&self) -> Option<u32> {
        (self.ambiguity == Ambiguity::AskWithTimeout).then_some(self.popup_timeout_secs)
    }

    fn info_edition_prefix(&self) -> &'static str {
        if self.lang_fr {
            "Info édition: "
        } else {
            "Edition info: "
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A choice offered to the user.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// Main label (series name, "1. Title", edition title).
    pub label: String,
    /// Matching bedetheque link (can be opened in a browser).
    pub url: String,
    /// Additional information (type, alternate number of the edition…).
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceKind {
    Series,
    Album,
    Edition,
}

pub trait Chooser {
    /// Returns index chosen, or None = cancel/skip. Called from the worker thread; may block.
    ///
    /// `timeout_secs` is set in `AskWithTimeout` mode. When it expires, the
    /// original plugin takes the 1st item for an album or an edition (return
    /// `Some(0)`) and gives up for a series (return `None`). For an edition,
    /// `None` amounts to taking the 1st edition.
    fn choose(
        &self,
        kind: ChoiceKind,
        book_caption: &str,
        candidates: &[Candidate],
        timeout_secs: Option<u32>,
    ) -> Option<usize>;
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ScrapeOutcome {
    /// Updated copy of the book (only enabled fields are changed).
    Updated(Book),
    /// Book skipped, with the reason.
    Skipped(String),
}

struct SeriesPick {
    key: String,
    name: String,
    link: String,
}

/// Scraping engine (HTTP client with cookies, memory of the last series
/// choice, log).
pub struct Scraper {
    settings: ScraperSettings,
    client: Client,
    warmed_up: bool,
    use_curl: bool,
    curl: Option<PathBuf>,
    curl_jar: PathBuf,
    curl_warmed: bool,
    last_pick: Option<SeriesPick>,
    single_book: bool,
    log: Vec<String>,
    #[cfg(test)]
    fake_pages: std::collections::HashMap<String, String>,
}

impl Drop for Scraper {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.curl_jar);
    }
}

/// The plugin's "dlg*" state for the current book.
#[derive(Default)]
struct Ctx {
    /// dlgNumber
    number: String,
    /// dlgAltNumber
    alt_number: String,
    /// dlgName (series formatted for comparison)
    name: String,
    /// dlgNameClean (series cleaned for searching)
    name_clean: String,
    /// AlbumNumNum
    numeric: bool,
    /// RenameSeries
    rename_series: Option<String>,
    /// Serie_Resume
    serie_resume: String,
    /// SkipAlbum
    skip_album: bool,
    /// Rescrape from the Web field: the link is not rewritten.
    rescrape: bool,
}

impl Ctx {
    fn caption(&self) -> String {
        let mut s = self.name.clone();
        if !self.number.is_empty() {
            s.push_str(" - ");
            s.push_str(&self.number);
        }
        if !self.alt_number.is_empty() {
            s.push_str(&t!(" (alt. no. {0})", self.alt_number));
        }
        s
    }
}

enum SerieResult {
    Album(String),
    Revue(bool),
    Nothing,
}

struct Edition {
    n: String,
    a: String,
    title: String,
    info: String,
    url: String,
    id: String,
}

impl Scraper {
    pub fn new(settings: ScraperSettings) -> Result<Self> {
        Ok(Scraper {
            settings,
            client: build_client()?,
            warmed_up: false,
            use_curl: false,
            curl: find_program("", &["curl"]),
            curl_jar: std::env::temp_dir().join(format!(
                "rayonnage-bedetheque-{}-cookies.txt",
                std::process::id()
            )),
            curl_warmed: false,
            last_pick: None,
            single_book: false,
            log: Vec::new(),
            #[cfg(test)]
            fake_pages: std::collections::HashMap::new(),
        })
    }

    /// Tells whether a single book is processed: like the plugin, the series
    /// choice is then offered even in `Ambiguity::Skip` mode.
    pub fn set_single_book(&mut self, single: bool) {
        self.single_book = single;
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Full scrape: returns the updated copy of the book (only enabled fields changed), or Skipped(reason).
    pub fn scrape(&mut self, book: &Book, chooser: &dyn Chooser) -> Result<ScrapeOutcome> {
        if self.settings.rescrape {
            let web = book.info.web.trim().to_string();
            if web.is_empty() {
                let reason = t!("no web link (rescrape)").to_string();
                self.info(t!("{0} ** Skipped ** ({1})", book_label(book), reason));
                return Ok(ScrapeOutcome::Skipped(reason));
            }
            let mut ctx = self.prepare(book, false);
            ctx.rescrape = true;
            return self.scrape_link(book, ctx, &web, chooser);
        }

        let mut ctx = self.prepare(book, false);
        if ctx.name.trim().is_empty() {
            let reason = t!("empty series name").to_string();
            self.info(t!("{0} ** Skipped ** ({1})", book_label(book), reason));
            return Ok(ScrapeOutcome::Skipped(reason));
        }
        self.debug(t!(
            "Series: [{0}] no. [{1}]{2} (search: \"{3}\")",
            ctx.name,
            ctx.number,
            if ctx.alt_number.is_empty() {
                String::new()
            } else {
                t!(" alt. no. [{0}]", ctx.alt_number)
            },
            ctx.name_clean
        ));

        let serie = match self.set_serie_id(&mut ctx, chooser)? {
            Ok(s) => s,
            Err(reason) => {
                self.info(t!(
                    "[{0}] {1} ** Skipped ** ({2})",
                    ctx.name, ctx.number, reason
                ));
                return Ok(ScrapeOutcome::Skipped(reason));
            }
        };
        let mut serie_url = full_url(&serie);
        if !serie_url.contains("/revue-") {
            serie_url = serie_url.to_lowercase().replace(".html", "__10000.html");
        }

        let mut b = book.clone();
        let mut ok = self.set_album_information(&mut b, &mut ctx, &serie_url, chooser)?;
        if !ctx.skip_album && !ok && !serie_url.contains("/revue-") {
            // No album list (single-album series…): read directly.
            ok = self.parse_album_info(&mut b, &mut ctx, &serie_url, false, chooser)?;
        }
        Ok(self.outcome(b, &ctx, ok))
    }

    /// QuickScrape: scrape from a given bedetheque URL (album, series or revue page).
    pub fn scrape_url(
        &mut self,
        book: &Book,
        url: &str,
        chooser: &dyn Chooser,
    ) -> Result<ScrapeOutcome> {
        let ctx = self.prepare(book, true);
        self.scrape_link(book, ctx, url, chooser)
    }

    // -- workflow ---------------------------------------------------------

    fn outcome(&mut self, b: Book, ctx: &Ctx, ok: bool) -> ScrapeOutcome {
        if ok {
            self.info(t!("{0} ** Processed **", book_label(&b)));
            ScrapeOutcome::Updated(b)
        } else {
            let reason = if ctx.skip_album {
                t!("album choice cancelled or ambiguous").to_string()
            } else {
                t!("album not found on bedetheque.com").to_string()
            };
            self.info(t!(
                "[{0}] {1} ** Skipped ** ({2})",
                ctx.name, ctx.number, reason
            ));
            ScrapeOutcome::Skipped(reason)
        }
    }

    fn scrape_link(
        &mut self,
        book: &Book,
        mut ctx: Ctx,
        url: &str,
        chooser: &dyn Chooser,
    ) -> Result<ScrapeOutcome> {
        let mut b = book.clone();
        let mut url = full_url(url.trim());
        self.debug(t!("Direct link: {0}", url));
        let ok = if url.contains("/serie-") || url.contains("/revue-") {
            if !url.contains("__10000.html") && !url.contains("/revue-") {
                url = url.to_lowercase().replace(".html", "__10000.html");
            }
            match self.parse_serie_info(&mut b, &mut ctx, &url, true, chooser)? {
                SerieResult::Album(a) => {
                    self.parse_album_info(&mut b, &mut ctx, &a, true, chooser)?
                }
                SerieResult::Revue(ok) => ok,
                SerieResult::Nothing => false,
            }
        } else {
            self.parse_album_info(&mut b, &mut ctx, &url, true, chooser)?
        };
        Ok(self.outcome(b, &ctx, ok))
    }

    /// Computes dlgNumber / dlgAltNumber / dlgName / dlgNameClean.
    fn prepare(&self, book: &Book, quick: bool) -> Ctx {
        let st = &self.settings;
        let mut ctx = Ctx {
            alt_number: book.info.alternate_number.clone(),
            ..Default::default()
        };
        let seps: &[char] = if quick {
            &['.', ',', '\\', '/', '-']
        } else {
            &['.', ',', '\\', '/']
        };
        let (number, alt, numeric) = split_number(&book.info.number, seps);
        ctx.number = number;
        if let Some(a) = alt {
            ctx.alt_number = a;
        }
        ctx.numeric = numeric;

        let name = titlize(&book.info.series, st.titleit);
        if quick {
            ctx.name_clean = name.clone();
            ctx.name = name;
        } else {
            ctx.name_clean = clean_articles(&name, &st.articles);
            let mut name = format_articles(&name, &st.articles);
            if let Some(p) = name.find(st.subpatt.as_str())
                && p > 0
            {
                name.truncate(p);
            }
            ctx.name = name;
        }
        ctx
    }

    /// SetSerieId: returns the series link, or the reason for giving up.
    fn set_serie_id(
        &mut self,
        ctx: &mut Ctx,
        chooser: &dyn Chooser,
    ) -> Result<Result<String, String>> {
        let st = self.settings.clone();
        let serie = ctx.name.clone();
        let Some(first) = serie.chars().next() else {
            return Ok(Err(t!("empty series name").into()));
        };
        let plain = remove_accents(&first.to_string());
        let letter = if plain
            .to_lowercase()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase())
        {
            plain
        } else {
            "0".to_string()
        };

        if !st.always_choose_serie {
            match self.fetch(&format!("bandes_dessinees_{letter}.html")) {
                Ok(page) => {
                    if let Some(mut u) = find_series_in_index(&page, serie.trim()) {
                        if !u.contains(".html") {
                            u.push_str(".html");
                        }
                        self.debug(t!("Series found in the index: {0}", u));
                        return Ok(Ok(u));
                    }
                }
                Err(e) => self.info(t!(
                    "Series index \"{0}\" unreachable: {1}",
                    letter,
                    format!("{e:#}")
                )),
            }
        }

        let key = remove_accents(&serie.to_lowercase());
        if let Some(p) = &self.last_pick
            && p.key == key
        {
            ctx.rename_series = Some(p.name.clone());
            return Ok(Ok(p.link.clone()));
        }
        self.last_pick = None;

        let query = remove_accents(&ctx.name_clean.to_lowercase())
            .trim()
            .to_string();
        self.debug(t!(
            "Search: {0}",
            format!("www.bedetheque.com/search/tout?RechTexte={query}&RechWhere=7")
        ));
        let page = self.fetch(&format!(
            "search/tout?RechTexte={}&RechWhere=7",
            encode_query(&query)
        ))?;

        if SERIE_LIST_CHECK.is_match(&page) || REVUE_LIST_CHECK.is_match(&page) {
            return Ok(Err(t!(
                "the search returns too many results for [{0}]",
                serie
            )));
        }

        let list = parse_search_results(&page);
        let n = list.len();
        if n == 1 && !st.always_choose_serie {
            self.debug(t!("Single series found: {0}", list[0].1));
            ctx.rename_series = Some(list[0].1.clone());
            return Ok(Ok(list[0].0.clone()));
        }
        if n > 1 || (st.always_choose_serie && n >= 1) {
            if st.ambiguity != Ambiguity::Skip || self.single_book {
                let target = remove_accents(&ctx.name.trim().to_lowercase());
                let mut unique = false;
                let mut idx = 0;
                for (i, (_, name)) in list.iter().enumerate() {
                    let lname = name.to_lowercase();
                    if remove_accents(&lname) == target {
                        unique = true;
                        idx = i;
                        if PAREN4.is_match(&lname) {
                            unique = false;
                        }
                    }
                    if st.always_choose_serie {
                        unique = false;
                    }
                }
                if unique {
                    self.debug(t!("Exact match: {0}", list[idx].1));
                    ctx.rename_series = Some(list[idx].1.clone());
                    return Ok(Ok(list[idx].0.clone()));
                }
                let cands: Vec<Candidate> = list
                    .iter()
                    .map(|(link, name)| Candidate {
                        label: name.clone(),
                        url: full_url(link),
                        detail: if link.starts_with("revue-") {
                            t!("Magazine").into()
                        } else {
                            t!("Series").into()
                        },
                    })
                    .collect();
                return Ok(
                    match chooser.choose(ChoiceKind::Series, &serie, &cands, st.timeout()) {
                        Some(i) if i < n => {
                            let (link, name) = list[i].clone();
                            self.debug(t!("Series chosen: {0}", name));
                            ctx.rename_series = Some(name.clone());
                            self.last_pick = Some(SeriesPick {
                                key,
                                name,
                                link: link.clone(),
                            });
                            Ok(link)
                        }
                        _ => Err(t!("series choice cancelled").into()),
                    },
                );
            }
            return Ok(Err(t!("album skipped, ambiguous").into()));
        }
        Ok(Err(t!("no series found for [{0}]", serie)))
    }

    fn set_album_information(
        &mut self,
        b: &mut Book,
        ctx: &mut Ctx,
        serie_url: &str,
        chooser: &dyn Chooser,
    ) -> Result<bool> {
        match self.parse_serie_info(b, ctx, serie_url, false, chooser)? {
            SerieResult::Album(url) => {
                self.debug(t!("Album: {0}", url));
                self.parse_album_info(b, ctx, &url, false, chooser)
            }
            SerieResult::Revue(ok) => Ok(ok),
            SerieResult::Nothing => {
                self.debug(t!(
                    "Album [{0}] {1} not found ({2})",
                    ctx.name, ctx.number, serie_url
                ));
                Ok(false)
            }
        }
    }

    /// parseSerieInfo: series information, then album choice.
    fn parse_serie_info(
        &mut self,
        b: &mut Book,
        ctx: &mut Ctx,
        url: &str,
        direct: bool,
        chooser: &dyn Chooser,
    ) -> Result<SerieResult> {
        let st = self.settings.clone();
        ctx.skip_album = false;
        let page = self.fetch(url)?;
        if url.contains("/revue-") {
            return self.parse_revue_series(b, ctx, url, &page, direct, chooser);
        }
        if page.is_empty() {
            return Ok(SerieResult::Nothing);
        }

        if st.series
            && let Some(r) = &ctx.rename_series
        {
            b.info.series = st.tz(r, false);
        }

        if direct && st.series {
            match SERIE_QSERIE.captures(&page) {
                Some(c) => {
                    let q = check_web_char(c[1].trim());
                    b.info.series = st.tz(&q, false);
                    self.debug(t!("Series: {0}", q));
                }
                None => return Ok(SerieResult::Nothing),
            }
        }

        if st.genre {
            let genre = SERIE_GENRE
                .captures(&page)
                .map(|c| check_web_char(c[1].trim()))
                .unwrap_or_default();
            if !genre.is_empty() {
                if genre.to_lowercase().contains("rotique") {
                    b.info.age_rating = "PG".into();
                }
                b.info.genre = genre;
            }
            self.debug(t!("Genre: {0}", b.info.genre));
        }

        if st.synopsis {
            ctx.serie_resume = serie_resume(&page);
            self.debug(if ctx.serie_resume.is_empty() {
                t!("No series summary")
            } else {
                t!("Series summary found")
            });
        }

        if st.status {
            let fin = SERIE_STATUS
                .captures(&page)
                .map(|c| check_web_char(c[1].trim()))
                .unwrap_or_default();
            self.debug(t!("Publication status: {0}", fin));
            let one_shot_num = ctx.number.to_lowercase() == "one shot";
            b.series_complete = if fin.contains("finie") || one_shot_num {
                YesNo::Yes
            } else if fin.to_lowercase().contains("one shot") {
                if st.one_shot_format {
                    b.info.format = "One Shot".into();
                }
                YesNo::Yes
            } else if fin.contains("cours") {
                YesNo::No
            } else {
                YesNo::Unknown
            };
        }

        if st.language
            && let Some(c) = SERIE_LANGUE.captures(&page)
        {
            let langue = c[1].trim().to_string();
            self.debug(t!("Language: {0}", langue));
            if let Some(iso) = language_iso(&langue) {
                b.info.language_iso = iso.into();
            }
        }

        if st.count && !direct {
            b.info.count = series_count(&page, &st, b.series_complete, ctx.numeric);
            self.debug(t!("Number of albums: {0}", b.info.count));
        }

        // List of the series' albums.
        let mut wanted = Vec::new();
        let mut all = Vec::new();
        for c in ALBUM_LIST.captures_iter(&page) {
            let tome = c.name("tome").map_or("", |m| m.as_str()).trim();
            let alt = c.name("alt").map_or("", |m| m.as_str()).trim();
            let num = if !tome.is_empty() { tome } else { alt };
            let item = (
                c["url"].to_string(),
                format!("{num}. {}", check_web_char(&c["title"])),
            );
            if !ctx.number.is_empty() && num == ctx.number && !direct {
                wanted.push(item.clone());
            }
            all.push(item);
        }
        let list = if wanted.is_empty() { all } else { wanted };

        let caption = ctx.caption();
        let mut album = self.album_chooser(ctx, &caption, &list, chooser);
        if album.is_none() && !ctx.skip_album {
            // No side list: probably a single album.
            album = EDITION_HEAD.captures(&page).map(|c| c[1].to_string());
            if album.is_some() {
                self.debug(t!("Number missing from the list, choosing the 1st album"));
            }
        }
        Ok(album.map_or(SerieResult::Nothing, SerieResult::Album))
    }

    /// AlbumChooser.
    fn album_chooser(
        &mut self,
        ctx: &mut Ctx,
        caption: &str,
        list: &[(String, String)],
        chooser: &dyn Chooser,
    ) -> Option<String> {
        self.debug(t!("Number of albums in the list: {0}", list.len()));
        match list.len() {
            0 => None,
            1 => Some(list[0].0.clone()),
            _ => {
                if self.settings.ambiguity == Ambiguity::Skip {
                    ctx.skip_album = true;
                    self.debug(t!("More than one album and user choice disabled"));
                    return None;
                }
                let cands: Vec<Candidate> = list
                    .iter()
                    .map(|(u, l)| Candidate {
                        label: l.clone(),
                        url: full_url(u),
                        detail: String::new(),
                    })
                    .collect();
                match chooser.choose(ChoiceKind::Album, caption, &cands, self.settings.timeout()) {
                    Some(i) if i < list.len() => Some(list[i].0.clone()),
                    _ => {
                        ctx.skip_album = true;
                        self.debug(t!("Choice cancelled, album skipped"));
                        None
                    }
                }
            }
        }
    }

    /// parseAlbumInfo: album page, edition choice and extraction.
    fn parse_album_info(
        &mut self,
        b: &mut Book,
        ctx: &mut Ctx,
        page_url: &str,
        direct: bool,
        chooser: &dyn Chooser,
    ) -> Result<bool> {
        let st = self.settings.clone();
        let html = self.fetch(page_url)?;
        let info_album = INFOS_ALBUMS.find(&html).map_or("", |m| m.as_str());

        // Bedetheque album identifier (for the full summary).
        let album_id = if let Some(c) = ALBUM_URL_ID.captures(page_url) {
            c[1].to_string()
        } else if page_url.contains("serie-") {
            SERIE_URL_ANCHOR
                .captures(page_url)
                .map(|c| c[1].to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let t = TOME
            .captures(&html)
            .map(|c| {
                let g1 = c[1].trim();
                if g1.is_empty() {
                    check_web_char(c[2].trim())
                } else {
                    g1.to_string()
                }
            })
            .unwrap_or_default();

        let mut editions = Vec::new();
        for (i, c) in EDITION.captures_iter(&html).enumerate() {
            let id = c["id"].trim().to_string();
            let a = match c.name("alt") {
                Some(m) => {
                    let alt = m.as_str().trim();
                    if is_numeric(&t) {
                        check_web_char(alt)
                    } else {
                        check_web_char(alt.replacen(t.as_str(), "", 1).trim())
                    }
                }
                None => String::new(),
            };
            let url = if i == 0 {
                format!("{page_url}#reed")
            } else {
                format!("{page_url}#{id}")
            };
            self.debug(t!(
                "Edition: volume [{0}] alt [{1}] title [{2}] id [{3}]",
                t,
                a,
                c["titre"].trim(),
                id
            ));
            editions.push(Edition {
                n: t.clone(),
                a,
                title: check_web_char(c["titre"].trim()),
                info: c["infos"].to_string(),
                url,
                id,
            });
        }
        if editions.is_empty() {
            self.debug(t!("No edition found on {0}", page_url));
            return Ok(false);
        }

        let mut picked = 0;
        if editions.len() > 1 {
            let by_alt = (!ctx.alt_number.is_empty())
                .then(|| editions.iter().position(|e| e.a == ctx.alt_number))
                .flatten();
            if !st.auto_choose_edition && st.ambiguity != Ambiguity::Skip {
                let cands: Vec<Candidate> = editions
                    .iter()
                    .map(|e| Candidate {
                        label: e.title.clone(),
                        url: e.url.replace("#reed", ""),
                        detail: if e.a.is_empty() {
                            format!("ID {}", e.id)
                        } else {
                            format!("{} · ID {}", e.a, e.id)
                        },
                    })
                    .collect();
                let caption = ctx.caption();
                picked = chooser
                    .choose(ChoiceKind::Edition, &caption, &cands, st.timeout())
                    .filter(|&i| i < editions.len())
                    .unwrap_or(0);
            } else {
                picked = by_alt.unwrap_or(0);
            }
        }
        let ed = &editions[picked];
        let info = ed.info.as_str();
        self.debug(t!(
            "Edition chosen: alt [{0}] title [{1}] id [{2}]",
            ed.a, ed.title, ed.id
        ));

        // Web
        if st.web && !ctx.rescrape {
            if !st.short_web_link {
                b.info.web = ed.url.replace("#reed", "");
            } else if let Some(c) = ALBUM_URL_ID.captures(page_url) {
                b.info.web = format!("www.bedetheque.com/BD--{}.html", &c[1]);
            }
            self.debug(t!("Web: {0}", b.info.web));
        }

        // Number and alternate number
        let (qnum, anum) = (ed.n.clone(), ed.a.clone());
        let from_alt = qnum.is_empty() && !anum.is_empty();
        b.info.number = if from_alt { anum.clone() } else { qnum };
        b.info.alternate_number = if from_alt {
            ctx.alt_number.clone()
        } else {
            anum
        };
        if st.pad_number != 0 {
            let w = st.pad_number as usize;
            if is_positive_int(&b.info.number) {
                b.info.number = zfill(&b.info.number, w);
            }
            if is_positive_int(&b.info.alternate_number) {
                b.info.alternate_number = zfill(&b.info.alternate_number, w);
            }
        }

        // Series name (header "Les 5 Terres" / sheet "5 Terres (Les)")
        let mut series = b.info.series.clone();
        let mut series_format = String::new();
        if let Some(c) = ALBUM_H1.captures(&html) {
            series = check_web_char(c[1].trim());
            series_format = SERIE_IN_SHEET
                .captures(info_album)
                .map(|c| check_web_char(c[1].trim()))
                .unwrap_or_else(|| series.clone());
            self.debug(t!("Series: {0} // formatted: {1}", series, series_format));
        }

        if st.title {
            let mut new_title = st.tz(&ed.title, false);
            if new_title.to_lowercase() == series.to_lowercase() {
                new_title.clear();
            }
            b.info.title = new_title;
        }

        if st.tags == "DEL" {
            b.info.tags.clear();
        } else if !st.tags.is_empty() {
            b.info.tags = st.tags.clone();
        }

        let generic = st.accept_generic_artists;
        if st.writer {
            b.info.writer = generic_artists(&[&ALBUM_SCENAR, &ALBUM_STORYBOARD], info, generic);
        }
        if st.penciller {
            b.info.penciller = generic_artists(&[&ALBUM_DESSIN], info, generic);
        }
        if st.colorist {
            b.info.colorist = generic_artists(&[&ALBUM_COLOR], info, generic);
            if b.info.colorist.contains("<N&B>") {
                b.info.black_and_white = YesNo::Yes;
            } else if !b.info.colorist.is_empty() {
                b.info.black_and_white = YesNo::No;
            }
        }
        if st.cover_artist {
            b.info.cover_artist = generic_artists(&[&ALBUM_COUVERT], info, generic);
        }
        if st.letterer {
            b.info.letterer = generic_artists(&[&ALBUM_LETTRAGE], info, generic);
        }
        if st.inker {
            b.info.inker = generic_artists(&[&ALBUM_INKER], info, generic);
        }

        if st.printed {
            (b.info.month, b.info.year) = depot_legal(info);
            if b.info.month < 1
                && let Some((m, y)) = acheve_imprimer(info)
            {
                b.info.month = m;
                b.info.year = y;
            }
            self.debug(t!("Date: {0}/{1}", b.info.month, b.info.year));
        }

        if st.publisher {
            b.info.publisher = ALBUM_EDITEUR
                .captures(info)
                .map(|c| parse_name(&c[1], generic).trim().to_string())
                .unwrap_or_default();
            self.debug(t!("Publisher: {0}", b.info.publisher));
        }

        if st.isbn {
            let isbn = ALBUM_ISBN
                .captures(info)
                .map(|c| c[1].to_string())
                .filter(|s| s != "</span>")
                .map(|s| check_web_char(&s).trim().to_string())
                .unwrap_or_default();
            let digits: String = isbn.chars().filter(|c| !matches!(c, '-' | ' ')).collect();
            if digits.len() == 13 && digits.chars().all(|c| c.is_ascii_digit()) {
                b.info.gtin = digits;
            }
            b.isbn = isbn;
            self.debug(t!("ISBN: {0}", b.isbn));
        }

        if st.rating
            && let Some(c) = ALBUM_EVAL.captures(&html)
            && let Ok(v) = c[1].trim().replace(',', ".").parse::<f32>()
        {
            b.info.community_rating = v.clamp(0.0, 5.0);
            self.debug(t!("Rating: {0}", v));
        }

        if st.imprint {
            let c = ALBUM_COLLECTION
                .captures(info)
                .or_else(|| ALBUM_COLLECTION.captures(info_album));
            b.info.imprint = c.map(|c| check_web_char(&c[1])).unwrap_or_default();
            self.debug(t!("Imprint: {0}", b.info.imprint));
        }

        if st.format_enabled() {
            b.info.format = ALBUM_TAILLE
                .captures(info)
                .map(|c| check_web_char(c[1].trim()))
                .unwrap_or_default();
            self.debug(t!("Format: {0}", b.info.format));
        }

        if st.synopsis {
            let mut resume = String::new();
            if !album_id.is_empty() {
                match self.fetch(&format!("ajax/resume/album/{album_id}")) {
                    Ok(r) => resume = r,
                    Err(e) => self.debug(t!("Album summary unreachable: {0}", format!("{e:#}"))),
                }
            }
            let mut summary = String::new();
            if !resume.trim().is_empty() {
                let resume = strip_tags(&check_web_char(&resume)).trim().to_string();
                let print_serie = st.serie_resume_everywhere || b.info.number == "1";
                let title_line = if b.info.title.is_empty() {
                    String::new()
                } else {
                    format!(">{}< \n", b.info.title)
                };
                if !ctx.serie_resume.is_empty()
                    && print_serie
                    && remove_accents(&ctx.serie_resume) != remove_accents(&resume)
                {
                    summary = ctx.serie_resume.clone();
                    if !resume.is_empty() {
                        summary.push_str(&format!("\n\n{title_line}{resume}"));
                    }
                } else if !resume.is_empty() {
                    summary = format!("{title_line}{resume}");
                }
                self.debug(t!("Album summary found"));
            } else {
                self.debug(t!("No album summary"));
            }
            if let Some(c) = ALBUM_INFOEDITION.captures(info)
                && &c[1] != " &nbsp;"
            {
                let ie = strip_tags(&check_web_char(&c[1])).trim().to_string();
                if !ie.is_empty() {
                    let prefix = st.info_edition_prefix();
                    summary = if summary.is_empty() {
                        format!("{prefix}{ie}")
                    } else {
                        format!("{summary}\n\n{prefix}{ie}")
                    };
                }
            }
            if !summary.is_empty() {
                b.info.summary = summary;
            }
        }

        if st.series {
            let formatted = if !series_format.is_empty() {
                st.tz(&series_format, false)
            } else {
                st.tz(&series, true)
            };
            b.info.series = if st.format_articles {
                formatted
            } else {
                st.tz(&series, false)
            };
            self.debug(t!("Series: {0}", b.info.series));
        }

        // QuickScrape: the language is not read (no series page), French by default.
        if st.language && direct && b.info.language_iso.is_empty() {
            b.info.language_iso = "fr".into();
        }

        // Pages: only for books without a file (like the plugin).
        if b.file_path.as_os_str().is_empty()
            && let Some(c) = ALBUM_PLANCHES.captures(info)
        {
            b.info.page_count = c[1].trim().parse().unwrap_or(0);
        }

        if st.notes {
            b.info.notes = self.notes_text();
        }
        Ok(true)
    }

    /// "Revue" (magazine) branch of parseSerieInfo: issue choice, then reading.
    fn parse_revue_series(
        &mut self,
        b: &mut Book,
        ctx: &mut Ctx,
        url: &str,
        page: &str,
        direct: bool,
        chooser: &dyn Chooser,
    ) -> Result<SerieResult> {
        let mut list: Vec<(String, String)> = REVUE_LIST_ALL
            .captures_iter(page)
            .map(|c| (c[1].to_string(), format!("Num: {}", c[2].trim())))
            .collect();
        if list.is_empty() {
            // Single page: named anchors.
            for c in REVUE_LIST_ALL_ALT.captures_iter(page) {
                let full = if url.to_lowercase().contains("www.bedetheque.com") {
                    format!("{url}#{}", &c[1])
                } else {
                    format!("{BASE_URL}{url}#{}", &c[1])
                };
                list.push((full, format!("Num: {}", c[2].trim())));
            }
        }
        let wanted = format!("Num: {}", ctx.number);
        let matched = list.iter().find(|x| x.1 == wanted).map(|x| x.0.clone());
        let album_url = match matched {
            Some(m) if !direct => Some(m),
            _ if direct && url.contains('#') => Some(url.to_string()),
            _ => {
                let caption = ctx.caption();
                self.album_chooser(ctx, &caption, &list, chooser)
            }
        };
        let Some(album_url) = album_url else {
            return Ok(SerieResult::Nothing);
        };

        let page2 = self.fetch(&album_url)?;
        let id = album_url.split_once('#').map_or("", |x| x.1);
        let header = if !id.is_empty() {
            rx(&REVUE_HEADER_PATTERN_ALT.replace("%s", &regex::escape(id)))
        } else {
            rx(&REVUE_HEADER_PATTERN.replace("%s", &regex::escape(&ctx.number)))
        };
        let ok = match header.captures(&page2) {
            Some(c) => {
                let entete = c.get(1).map_or("", |m| m.as_str()).to_string();
                let numero = c.get(3).map_or("", |m| m.as_str()).to_string();
                self.parse_revue_info(b, ctx, &entete, &numero, &album_url);
                true
            }
            None => false,
        };
        Ok(SerieResult::Revue(ok))
    }

    /// parseRevueInfo.
    fn parse_revue_info(&mut self, b: &mut Book, ctx: &Ctx, entete: &str, numero: &str, url: &str) {
        let st = self.settings.clone();
        if st.series {
            b.info.series = match &ctx.rename_series {
                Some(r) => st.tz(r, false),
                None => st.tz(&b.info.series, false),
            };
        }
        if !numero.is_empty() {
            b.info.number = numero.to_string();
        }
        if st.title
            && let Some(c) = REVUE_TITLE.captures(entete)
        {
            b.info.title = st.tz(c[1].trim(), false);
        }
        if st.genre {
            b.info.genre = "Revue".into();
        }
        if st.synopsis {
            let resume = REVUE_RESUME
                .captures(entete)
                .map(|c| strip_tags(&c[1]).trim().to_string())
                .unwrap_or_default();
            b.info.summary = check_web_char(&resume).trim().to_string();
        }
        if st.printed {
            (b.info.month, b.info.year) = REVUE_DEPOT
                .captures(entete)
                .and_then(|c| {
                    let d: Vec<char> = c[1].chars().collect();
                    let part = |a: usize, z: usize| {
                        d.get(a..z.min(d.len()))
                            .map(|s| s.iter().collect::<String>())
                    };
                    let m = part(3, 5)?.trim().parse().ok()?;
                    let y = part(6, 10)?.trim().parse().ok()?;
                    Some((m, y))
                })
                .unwrap_or((-1, -1));
        }
        if st.publisher {
            b.info.publisher = REVUE_EDITEUR
                .captures(entete)
                .map(|c| parse_name(&c[1], st.accept_generic_artists))
                .unwrap_or_default();
        }
        if b.file_path.as_os_str().is_empty()
            && let Some(c) = REVUE_PLANCHES.captures(entete)
        {
            b.info.page_count = c[1].trim().parse().unwrap_or(0);
        }
        if st.format_enabled()
            && let Some(c) = REVUE_PERIOD.captures(entete)
        {
            b.info.format = c[1].trim().to_string();
        }
        if st.language && b.info.language_iso.is_empty() {
            b.info.language_iso = "fr".into();
        }
        if st.web && !ctx.rescrape {
            b.info.web = url.to_string();
        }
        // The plugin resets these fields for a magazine; here only those whose
        // retrieval is enabled.
        if st.status {
            b.series_complete = YesNo::Unknown;
        }
        if st.colorist {
            b.info.black_and_white = YesNo::Unknown;
        }
        b.info.alternate_number.clear();
        if st.tags == "DEL" {
            b.info.tags.clear();
        } else if !st.tags.is_empty() {
            b.info.tags = st.tags.clone();
        }
        if st.writer {
            b.info.writer.clear();
        }
        if st.penciller {
            b.info.penciller.clear();
        }
        if st.cover_artist {
            b.info.cover_artist.clear();
        }
        if st.letterer {
            b.info.letterer.clear();
        }
        if st.inker {
            b.info.inker.clear();
        }
        if st.isbn {
            b.isbn.clear();
        }
        if st.imprint {
            b.info.imprint.clear();
        }
        if st.rating {
            b.info.community_rating = 0.0;
        }
        if st.notes {
            b.info.notes = self.notes_text();
        }
    }

    fn notes_text(&self) -> String {
        const WEEKDAYS_FR: [&str; 7] = [
            "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
        ];
        let now = Local::now();
        let date = if self.settings.lang_fr {
            format!(
                "{} {:02} {} {} {}",
                WEEKDAYS_FR[now.weekday().num_days_from_monday() as usize],
                now.day(),
                MONTHS_FR[now.month0() as usize],
                now.year(),
                now.format("%H:%M:%S")
            )
        } else {
            now.format("%A %d %B %Y %H:%M:%S").to_string()
        };
        format!("Bedetheque.com - {date}\nRayonnage {}", env!("CARGO_PKG_VERSION"))
    }

    // -- log -----------------------------------------------------------------

    fn info(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
    }

    fn debug(&mut self, msg: impl Into<String>) {
        if self.settings.debug_log {
            self.log.push(msg.into());
        }
    }

    // -- HTTP -----------------------------------------------------------------

    fn fetch(&mut self, url: &str) -> Result<String> {
        let url = full_url(url);
        self.debug(format!("GET {url}"));
        #[cfg(test)]
        if !self.fake_pages.is_empty() {
            return self
                .fake_pages
                .get(&url)
                .cloned()
                .ok_or_else(|| anyhow!("missing test page: {url}"));
        }

        if !self.use_curl {
            if !self.warmed_up {
                self.warmed_up = true;
                // Home page first, to get the cookies. If the built-in HTTP
                // stack cannot even connect, switch to curl directly.
                if let Err(e) = self.client.get(BASE_URL).send().and_then(|r| r.text())
                    && (e.is_timeout() || e.is_connect())
                    && self.curl.is_some()
                {
                    self.info(t!(
                        "Unable to connect ({0}): using curl for bedetheque.com",
                        e
                    ));
                    self.use_curl = true;
                    return self.curl_get(&url);
                }
            }
            match http_get(&self.client, &url) {
                Ok(t) => return Ok(t),
                Err(HttpError::Status(403)) => {
                    self.info(t!(
                        "bedetheque.com refused the request (403): renewing cookies…"
                    ));
                    self.client = build_client()?;
                    let _ = self.client.get(BASE_URL).send().and_then(|r| r.text());
                    match http_get(&self.client, &url) {
                        Ok(t) => return Ok(t),
                        Err(HttpError::Status(403)) => {
                            self.info(t!("Refused again (403): using curl"));
                            self.use_curl = true;
                        }
                        Err(HttpError::Status(c)) => bail!(t!("HTTP {0} for {1}", c, url)),
                        Err(HttpError::Transport(e)) => {
                            if self.curl.is_none() {
                                return Err(e);
                            }
                        }
                    }
                }
                Err(HttpError::Status(c)) => bail!(t!("HTTP {0} for {1}", c, url)),
                Err(HttpError::Transport(e)) => {
                    if self.curl.is_none() {
                        return Err(e);
                    }
                    self.info(t!("Network error ({0}), trying with curl", format!("{e:#}")));
                    let body = self.curl_get(&url)?;
                    // curl works where the built-in stack fails: keep it.
                    self.use_curl = true;
                    return Ok(body);
                }
            }
        }
        self.curl_get(&url)
    }

    fn curl_get(&mut self, url: &str) -> Result<String> {
        let curl = self
            .curl
            .clone()
            .ok_or_else(|| anyhow!(t!("access refused by bedetheque.com and curl not found")))?;
        if !self.curl_warmed {
            self.curl_warmed = true;
            let _ = run_curl(&curl, &self.curl_jar, BASE_URL);
        }
        let (code, body) = run_curl(&curl, &self.curl_jar, url)?;
        if !(200..300).contains(&code) {
            bail!(t!("HTTP {0} (curl) for {1}", code, url));
        }
        Ok(body)
    }
}

fn book_label(b: &Book) -> String {
    let i = &b.info;
    let mut s = format!("[{}] {}", i.series, i.number);
    if !i.alternate_number.is_empty() {
        s.push_str(&t!(" alt. no. [{0}]", i.alternate_number));
    }
    if !i.title.is_empty() {
        s.push_str(&format!(" - {}", i.title));
    }
    s
}

fn build_client() -> Result<Client> {
    let mut h = HeaderMap::new();
    h.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    h.insert(header::ACCEPT, HeaderValue::from_static(ACCEPT));
    h.insert(
        header::ACCEPT_LANGUAGE,
        HeaderValue::from_static(ACCEPT_LANGUAGE),
    );
    h.insert(header::REFERER, HeaderValue::from_static(BASE_URL));
    h.insert(
        header::UPGRADE_INSECURE_REQUESTS,
        HeaderValue::from_static("1"),
    );
    Client::builder()
        .default_headers(h)
        .cookie_store(true)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context(t!("creating the HTTP client"))
}

enum HttpError {
    Status(u16),
    Transport(anyhow::Error),
}

fn http_get(client: &Client, url: &str) -> std::result::Result<String, HttpError> {
    let resp = client
        .get(url)
        .send()
        .map_err(|e| HttpError::Transport(e.into()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(HttpError::Status(status.as_u16()));
    }
    resp.text().map_err(|e| HttpError::Transport(e.into()))
}

fn run_curl(curl: &std::path::Path, jar: &std::path::Path, url: &str) -> Result<(u16, String)> {
    let out = Command::new(curl)
        .args([
            "-sS",
            "-L",
            "--compressed",
            "--max-time",
            &TIMEOUT_SECS.to_string(),
        ])
        .args(["-A", USER_AGENT])
        .args(["-H", &format!("Accept: {ACCEPT}")])
        .args(["-H", &format!("Accept-Language: {ACCEPT_LANGUAGE}")])
        .args(["-H", &format!("Referer: {BASE_URL}")])
        .args(["-H", "Upgrade-Insecure-Requests: 1"])
        .arg("-b")
        .arg(jar)
        .arg("-c")
        .arg(jar)
        .args(["-w", "\n%{http_code}", url])
        .output()
        .context(t!("running curl"))?;
    if !out.status.success() {
        bail!(t!(
            "curl failed: {0}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_str()));
    Ok((code.trim().parse().unwrap_or(0), body.to_string()))
}

/// GetFullURL (also accepts links without a scheme, e.g. short links).
fn full_url(u: &str) -> String {
    let lower = u.to_lowercase();
    if lower.starts_with("https://www.bedetheque.com/") {
        u.to_string()
    } else if lower.starts_with("http://www.bedetheque.com/") {
        format!("https://{}", &u["http://".len()..])
    } else if lower.starts_with("www.bedetheque.com/") {
        format!("https://{u}")
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        u.to_string()
    } else {
        format!("{BASE_URL}{}", u.trim_start_matches('/'))
    }
}

/// "quote_plus" encoding of a query value.
fn encode_query(s: &str) -> String {
    urlencoding::encode(s).replace("%20", "+")
}

// ---------------------------------------------------------------------------
// Regular expressions (taken from the plugin)
// ---------------------------------------------------------------------------

fn rx(p: &str) -> Regex {
    RegexBuilder::new(p)
        .size_limit(1 << 26)
        .dfa_size_limit(1 << 26)
        .build()
        .unwrap_or_else(|e| panic!("invalid regular expression {p:?}: {e}"))
}

macro_rules! regexes {
    ($($name:ident = $p:expr;)*) => {
        $(static $name: LazyLock<Regex> = LazyLock::new(|| rx($p));)*
    };
}

const REVUE_HEADER_PATTERN: &str = r#"(?is)class="couv"(.{1,100}?couvertures"\shref="(https.{1,150}?)">.{1,600}?class="titre".{1,100}?#(%s)\..+?class="autres".+?)</li>"#;
const REVUE_HEADER_PATTERN_ALT: &str = r#"(?is)<a name="%s">.+?class="couv"(.{1,100}?couvertures"\shref="(https.{1,150}?)">.+?class="titre".{1,100}?#(.+?)\..+?class="autres".+?)</li>"#;

regexes! {
    LAST_FIRST_NAMES = r"(?P<name>[^,]*?), (?P<firstname>[^,]*?)$";
    SERIE_LIST = r#"(?is)<a\shref="https://www.bedetheque.com/serie-([^<>"]*?)">.+?class="bdt-liste-libelle">.*?<b>(.+?)</b>.+?</a>"#;
    SERIE_LIST_CHECK = r"(?is)séries\strouvées.{40,80}?La\srecherche\srenvoie\splus\sde";
    INDEX_A_HREF = r#"(?is)<a href="([^"]+?)">"#;
    CLOSE_A = r"(?i)</a>";
    INFOS_ALBUMS = r#"(?is)<dl class="bdt-sheet">.+?</div>"#;
    // (?<="Langue de parution") rewritten without a lookbehind assertion.
    SERIE_LANGUE = r#"(?i)<li[^<>]*?"Langue de parution".+?class="bdt-sh-flag"[^<>]+?>([^<>]+?)<"#;
    SERIE_GENRE = r#"(?is)class="bdt-tag bdt-sh-genre"[^<>]+?>([^<>]+?)<"#;
    SERIE_RESUME = r#"(?is)class="bdt-sh-resume">([^<>]*?)<"#;
    SERIE_STATUS = r#"(?is)class="bdt-sh-parution">([^<>]+?)<"#;
    SERIE_COUNT = r#"(?is)title="Nombre de tomes de la série">(\d+)"#;
    SERIE_COUNT_REAL = r#"(?is)class="bdt-box-albums-liste">(.+?)</ul>"#;
    SERIE_COUNTOF = r#"(?is)class="bdt-box-albums-num"[^<>]+?>([^<>]+?)<span"#;
    SERIE_QSERIE = r#"(?is)<h1>.+?<a href="[^<>]+/serie-[^\.]+\.html">([^"<>]+)</a>.*?</h1>"#;
    TOUT_SUR_LA_SERIE = r"Tout sur la série.*?:\s?";
    ALBUM_LIST = r#"(?is)<li[^>]*>\s+?<a href="(?P<url>[^"]+?)" title="(?P<title>[^"]+?)">.+?class="bdt-box-albums-num"[^<>]+?>(?P<tome>[^<>]*?)<span class="bdt-numa">(?P<alt>[^<>]*?)<"#;
    EDITION_HEAD = r#"(?is)class="bdt-edition-head">\s*<h3>\s*<a[^<>]+?href="([^"]+?)""#;
    ALBUM_URL_ID = r"-(\d+).html";
    SERIE_URL_ANCHOR = r"serie.*?\.html#(\d+)$";
    TOME = r#"(?is)<h2 class="bdt-ah-sub">\s*([^<>]+?)<span class="bdt-numa">(.*?)</span>"#;
    EDITION = r#"(?is)<article class="bdt-edition bdt-edition--detail" id="ed-(?P<id>[^"]+)">.+?<h3>.+?<span class="bdt-numa">(?P<alt>[^\s<]+)?.+?</h3>.+?<a class="bdt-img bdt-ecov[^"]+"\shref="(?P<couv>[^"]+)".+?<dt>Titre</dt><dd><b>(?P<titre>[^<]+).+?<dt>Identifiant</dt>(?P<infos>.+?)</article>"#;
    ALBUM_H1 = r#"(?is)bdt-ah-top.+?<h1>.+?>([^"]+?)[<>]"#;
    SERIE_IN_SHEET = r"(?is)<dt>S.rie</dt><dd><a[^>]+?>([^<]+?)</a>";
    ALBUM_EVAL = r#"(?is)ratingValue">(.*?)<"#;
    ALBUM_MULTI_AUTHOR_NAMES = r#"(?is)">([^<>]*?)</"#;
    ALBUM_SCENAR = r"(?is)<dt>Sc.nario</dt>\s*<dd>(.+?)<dt>[^<>]+?</dt>";
    ALBUM_STORYBOARD = r"(?is)<dt>storyboard</dt>\s*<dd>(.+?)<dt>[^<>]+?</dt>";
    ALBUM_DESSIN = r"(?is)<dt>dessin</dt>\s*<dd>(.+?)<dt>[^<>]+?</dt>";
    ALBUM_COLOR = r"(?is)<dt>Couleurs</dt>\s*<dd>(.+?)<dt>[^<>]+?</dt>";
    ALBUM_COUVERT = r"(?is)<dt>Couverture</dt>\s*<dd>(.+?)<dt>[^<>]+?</dt>";
    ALBUM_LETTRAGE = r"(?is)<dt>Lettrage</dt>\s*<dd>(.+?)<dt>[^<>]+?</dt>";
    ALBUM_INKER = r"(?is)<dt>Encrage</dt>\s*<dd>(.+?)<dt>[^<>]+?</dt>";
    ALBUM_DEPOT = r"(?is)<dt>D.p.t L.gal</dt>\s+<dd>(?P<month>[\d|-]{0,2})/?(?P<year>[\d]{2,4})?";
    ALBUM_ACHEVE = r"(?is)<dt>Achev.*?\s:\s</dt>\s+<dd>(?P<month>[\d|-]{0,2})/?(?P<year>[\d]{2,4})?<";
    ALBUM_EDITEUR = r"(?is)<dt>.diteur</dt>\s+<dd>(.*?)</dd>";
    ALBUM_COLLECTION = r"(?is)<dt>Collection</dt><dd>(?:<a href.+?>)*([^><]+?)<";
    ALBUM_TAILLE = r"(?is)<dt>Format</dt>\s*<dd>(.*?)</dd>";
    ALBUM_ISBN = r"(?is)<dt>EAN/ISBN</dt>\s*<dd>(.*?)</dd>";
    ALBUM_PLANCHES = r"(?is)<dt>Planches</dt>\s*<dd>(.*?)</dd>";
    ALBUM_INFOEDITION = r"(?is)<b>Info\s.*?dition</b>\s?(.*?)<";
    REVUE_LIST = r#"(?is)<a\shref="https://www.bedetheque.com/revue-([^<>"]*?)">.+?class="bdt-liste-libelle">.*?<b>(.+?)</b>.+?</a>"#;
    REVUE_LIST_EXISTS = r"(?is)<h2>\d{1,3} revue\w?? trouvée\w??</h2>";
    REVUE_LIST_CHECK = r"(?is)revues\strouvées.{40,80}?La\srecherche\srenvoie\splus\sde";
    REVUE_LIST_ALL = r#"(?is)<option\svalue="(https://www\.bedetheque\.com/revue-[^>]+?)">(.+?)</option>"#;
    REVUE_LIST_ALL_ALT = r#"(?is)<a name="(.+?)">.+?class="titre".{1,100}?#(.+?)\..+?"#;
    REVUE_TITLE = r#"(?is)<h3 class="titre".+?</span>(.+?)</h3>"#;
    REVUE_RESUME = r"(?is)<em>Sommaire.*?</em>(.*?)</p";
    REVUE_PLANCHES = r"(?is)>Nb\sPages\s:\s??</label>(.*?)<";
    REVUE_DEPOT = r"(?is)<label>Parution\s:s??</label>(.*?)</";
    REVUE_PERIOD = r"(?is)<label>P.riodicit.\s:\s??</label>(.*?)</";
    REVUE_EDITEUR = r"(?is)<label>Editeur\s:\s?</label>(.*?)</";
    STRIP_TAGS = r"(?is)<[^<>]+?>";
    PAREN4 = r"\(.{4,}?\)";
}

// ---------------------------------------------------------------------------
// Page parsing (pure functions)
// ---------------------------------------------------------------------------

/// SERIE_URL_PATTERN of the alphabetical index:
/// `<a href="([^"]+?)">(?:(?!</a>).)+?<span class="bdt-liste-libelle"><b>NAME</b></span>`
/// (the "tempered token" is replaced by a search bounded by `</a>`).
fn find_series_in_index(page: &str, serie: &str) -> Option<String> {
    // checkRegExp: "(" and ")" become ".", "&" → &amp;, '"' → &quot;
    let mut name = String::new();
    for c in serie.chars() {
        match c {
            '(' | ')' => name.push('.'),
            '&' => name.push_str("&amp;"),
            '"' => name.push_str("&quot;"),
            _ => name.push_str(&regex::escape(&c.to_string())),
        }
    }
    let target = rx(&format!(
        r#"(?is)<span class="bdt-liste-libelle"><b>{name}</b></span>"#
    ));
    for c in INDEX_A_HREF.captures_iter(page) {
        let start = c.get(0)?.end();
        let end_a = CLOSE_A
            .find_at(page, start)
            .map_or(page.len(), |m| m.start());
        if let Some(t) = target.find_at(page, start)
            && t.start() > start
            && t.start() <= end_a
        {
            return Some(c[1].to_string());
        }
    }
    None
}

/// Search results: (relative link "serie-…" / "revue-…", name).
fn parse_search_results(page: &str) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = SERIE_LIST
        .captures_iter(page)
        .map(|c| {
            (
                format!("serie-{}", &c[1]),
                check_web_char(&strip_tags(&c[2])).trim().to_string(),
            )
        })
        .collect();
    if REVUE_LIST_EXISTS.is_match(page) {
        list.extend(REVUE_LIST.captures_iter(page).map(|c| {
            (
                format!("revue-{}", &c[1]),
                check_web_char(&strip_tags(&c[2])).trim().to_string(),
            )
        }));
    }
    list
}

fn serie_resume(page: &str) -> String {
    let page = check_web_char(&page.replace("\r\n", ""));
    let resume = SERIE_RESUME
        .captures(&page)
        .map(|c| strip_tags(&c[1]).trim().to_string())
        .unwrap_or_default();
    let resume = TOUT_SUR_LA_SERIE.replace_all(&resume, "");
    check_web_char(&resume).trim().to_string()
}

/// Number of albums in the series ("volume numbering" and "only if finished"
/// options).
fn series_count(page: &str, st: &ScraperSettings, complete: YesNo, numeric: bool) -> i32 {
    if st.count_only_if_complete && complete == YesNo::No {
        -1
    } else if !st.count_of {
        match SERIE_COUNT.captures(page) {
            Some(c) if numeric => c[1].parse().unwrap_or(-1),
            _ => -1,
        }
    } else if let Some(c) = SERIE_COUNT_REAL.captures(page) {
        let max = SERIE_COUNTOF
            .captures_iter(&c[1])
            .filter_map(|n| n[1].trim().parse::<i32>().ok())
            .fold(0, i32::max);
        if !numeric || max <= 0 { -1 } else { max }
    } else {
        -1
    }
}

fn language_iso(langue: &str) -> Option<&'static str> {
    let p: String = langue.chars().take(2).collect();
    Some(match p.as_str() {
        "Fr" => "fr",
        "Al" => "de",
        "An" => "en",
        "It" => "it",
        "Es" => "es",
        "Ne" => "nl",
        "Po" => "pt",
        "Ja" => "ja",
        _ => return None,
    })
}

/// getGenericBookArtists: artists of one or more roles, without duplicates.
fn generic_artists(patterns: &[&Regex], info: &str, accept_generic: bool) -> String {
    let mut list: Vec<String> = Vec::new();
    for p in patterns {
        let Some(c) = p.captures(info) else { continue };
        let block = &c[1];
        if block.contains("&lt;Indéterminé&gt;") {
            if accept_generic && !list.iter().any(|a| a == "<Indéterminé>") {
                list.push("<Indéterminé>".into());
            }
        } else {
            for m in ALBUM_MULTI_AUTHOR_NAMES.captures_iter(block) {
                let a = parse_name(m[1].trim(), accept_generic);
                if !a.is_empty() && !list.contains(&a) {
                    list.push(a);
                }
            }
        }
    }
    list.join(", ")
}

/// parseName: "Last, First" → "First Last", entities decoded.
fn parse_name(extracted: &str, accept_generic: bool) -> String {
    if !accept_generic && extracted.starts_with("&lt;") {
        return String::new();
    }
    let name = match LAST_FIRST_NAMES.captures(extracted) {
        Some(c) => format!("{} {}", &c["firstname"], &c["name"]),
        None => extracted.trim().to_string(),
    };
    check_web_char(&name).trim().to_string()
}

/// Legal deposit "MM/YYYY" → (month, year), (-1, -1) if missing or "-".
fn depot_legal(info: &str) -> (i32, i32) {
    let Some(c) = ALBUM_DEPOT.captures(info) else {
        return (-1, -1);
    };
    let month = c.name("month").map_or("", |m| m.as_str());
    if month == "-" || month.is_empty() {
        return (-1, -1);
    }
    let m = month.parse::<i32>().ok();
    let y = c.name("year").and_then(|y| y.as_str().parse::<i32>().ok());
    match (m, y) {
        (Some(m), Some(y)) => (m, y),
        _ => (-1, -1),
    }
}

fn acheve_imprimer(info: &str) -> Option<(i32, i32)> {
    let c = ALBUM_ACHEVE.captures(info)?;
    let m = c.name("month")?.as_str().parse().ok()?;
    let y = c.name("year")?.as_str().parse().ok()?;
    Some((m, y))
}

// ---------------------------------------------------------------------------
// Text utilities (faithful to the plugin)
// ---------------------------------------------------------------------------

/// checkWebChar: HTML entity decoding.
pub fn check_web_char(s: &str) -> String {
    let s = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("<br />", "")
        .replace("&quot;", "\"")
        .replace('\u{92}', "'")
        .replace("&#039;", "'");
    html_escape::decode_html_entities(&s).into_owned()
}

/// Removes HTML tags.
pub fn strip_tags(s: &str) -> String {
    STRIP_TAGS.replace_all(s, "").into_owned()
}

/// The plugin's remove_accents (accented capitals become lowercase).
pub fn remove_accents(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ä' | 'Å' | 'Ã' => {
                out.push('a')
            }
            'è' | 'é' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => out.push('e'),
            'ç' | 'Ç' => out.push('c'),
            'ì' | 'í' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => out.push('i'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => out.push('u'),
            'œ' | 'Œ' => out.push_str("oe"),
            _ => out.push(c),
        }
    }
    out
}

/// titlize: capital at the start of the string and after any
/// non-alphanumeric character (except "'s"). Nothing is lowercased.
pub fn titlize(s: &str, titleit: bool) -> String {
    if !titleit {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let n = s.trim().chars().count();
    let mut out = String::with_capacity(s.len());
    let mut up = false;
    for (i, &c) in chars.iter().enumerate().take(n) {
        if up || i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        let apos_s = c == '\'' && chars.get(i + 1).is_some_and(|d| d.to_lowercase().eq(['s']));
        up = !c.is_alphanumeric() && !apos_s;
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut it = s.chars();
    match it.next() {
        Some(c) => c.to_uppercase().chain(it).collect(),
        None => String::new(),
    }
}

fn article_list(articles: &str) -> Vec<&str> {
    articles.split(',').filter(|a| !a.is_empty()).collect()
}

/// Length (in bytes) of the prefix of `s` equal to the article `art`.
fn match_article(s: &str, art: &str, ignore_case: bool) -> Option<usize> {
    let mut it = s.char_indices();
    let mut end = 0;
    for ac in art.chars() {
        let (i, c) = it.next()?;
        let eq = if ignore_case {
            c.to_lowercase().eq(ac.to_lowercase())
        } else {
            c == ac
        };
        if !eq {
            return None;
        }
        end = i + c.len_utf8();
    }
    Some(end)
}

/// Possible positions after `\s*` (from shortest to longest).
fn ws_positions(s: &str, from: usize) -> Vec<usize> {
    let mut v = vec![from];
    for c in s[from..].chars() {
        if !c.is_whitespace() {
            break;
        }
        v.push(v.last().unwrap() + c.len_utf8());
    }
    v
}

/// `(?<=['\s])`
fn lookbehind_ok(s: &str, pos: usize) -> bool {
    s[..pos]
        .chars()
        .next_back()
        .is_some_and(|c| c == '\'' || c.is_whitespace())
}

/// `(?=[^STOPS]*(?:\s[SEPS]\s))`
fn sep_ahead(r: &str, stops: &[char], seps: &[char]) -> bool {
    let chars: Vec<char> = r.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_whitespace()
            && chars.get(i + 1).is_some_and(|d| seps.contains(d))
            && chars.get(i + 2).is_some_and(|d| d.is_whitespace())
        {
            return true;
        }
        if stops.contains(c) {
            return false;
        }
    }
    false
}

fn prefix_until<'a>(r: &'a str, stops: &[char]) -> &'a str {
    &r[..r.find(stops).unwrap_or(r.len())]
}

const CLEAN_STOPS: &[char] = &['/', '\r', '\n', '(', ':', ',', '!'];
const CLEAN_DASH_STOPS: &[char] = &['-', '–', '\r', '\n'];
const DASHES: &[char] = &['-', '–'];

/// cleanARTICLES: removes the leading article and cuts the name before
/// "-", "(", ":", ",", "!" or "/" (name used for searching).
pub fn clean_articles(s: &str, articles: &str) -> String {
    let mut s = s.to_string();
    // ^(ARTICLES)\s*(?<=['\s])((?=[^/\r\n\(:\,!]*(?:\s[-–]\s))[^-–\r\n]*|.[^/\r\n\(:\,!]*)  (IGNORECASE)
    'outer: for art in article_list(articles) {
        let Some(aend) = match_article(&s, art, true) else {
            continue;
        };
        for &p in ws_positions(&s, aend).iter().rev() {
            if !lookbehind_ok(&s, p) {
                continue;
            }
            let r = &s[p..];
            let g2 = if sep_ahead(r, CLEAN_STOPS, DASHES) {
                Some(prefix_until(r, CLEAN_DASH_STOPS).to_string())
            } else {
                r.chars()
                    .next()
                    .filter(|&c| c != '\n')
                    .map(|c| format!("{c}{}", prefix_until(&r[c.len_utf8()..], CLEAN_STOPS)))
            };
            if let Some(g2) = g2 {
                s = g2.trim().to_string();
                break 'outer;
            }
        }
    }
    // ^[#]*(.(?=[^/\r\n\(:\,!]*(?:\s[-–]\s))[^-–\r\n]*|.[^/\r\n\(:\,!]*)
    let hashes = s.chars().take_while(|&c| c == '#').count();
    for k in (0..=hashes).rev() {
        let r = &s[k..];
        let Some(c) = r.chars().next().filter(|&c| c != '\n') else {
            continue;
        };
        let rest = &r[c.len_utf8()..];
        let g = if sep_ahead(rest, CLEAN_STOPS, DASHES) {
            format!("{c}{}", prefix_until(rest, CLEAN_DASH_STOPS))
        } else {
            format!("{c}{}", prefix_until(rest, CLEAN_STOPS))
        };
        return g.trim().to_string();
    }
    s
}

/// formatARTICLES: "Les 5 Terres" → "5 Terres (Les)" (case-sensitive, like
/// the plugin, whose IGNORECASE flag was ignored).
pub fn format_articles(s: &str, articles: &str) -> String {
    let replaced = format_articles_sub(s, articles).unwrap_or_else(|| s.to_string());
    capitalize(replaced.trim())
}

// ^(ARTICLES)\s*(?<=['\s])((?=[^(]*(?:\s[-–:]\s))[^-–:\r\n]*|[^\(/\r\n]*)(?![-–:/\(])\s*([^\r\n]*)
// → \2 (\1) \3
fn format_articles_sub(s: &str, articles: &str) -> Option<String> {
    const NEG: &[char] = &['-', '–', ':', '/', '('];
    for art in article_list(articles) {
        let Some(aend) = match_article(s, art, false) else {
            continue;
        };
        for &p in ws_positions(s, aend).iter().rev() {
            if !lookbehind_ok(s, p) {
                continue;
            }
            let r = &s[p..];
            let mut alts = Vec::new();
            if sep_ahead(r, &['('], &['-', '–', ':']) {
                alts.push(prefix_until(r, &['-', '–', ':', '\r', '\n']));
            }
            alts.push(prefix_until(r, &['(', '/', '\r', '\n']));
            for g in alts {
                let bounds = g
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(g.len()));
                let bounds: Vec<usize> = bounds.collect();
                for &l in bounds.iter().rev() {
                    if r[l..].chars().next().is_some_and(|c| NEG.contains(&c)) {
                        continue;
                    }
                    let after = r[l..].trim_start();
                    let g3_end = after.find(['\r', '\n']).unwrap_or(after.len());
                    return Some(format!(
                        "{} ({}) {}{}",
                        &r[..l],
                        &s[..aend],
                        &after[..g3_end],
                        &after[g3_end..]
                    ));
                }
            }
        }
    }
    None
}

fn is_numeric(s: &str) -> bool {
    s.trim().parse::<f64>().is_ok()
}

fn is_positive_int(s: &str) -> bool {
    s.trim().parse::<i64>().is_ok_and(|v| v >= 0)
}

fn zfill(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{}{s}", "0".repeat(width - n))
    }
}

/// Parses the book number: (number, forced alternate number, numeric?).
/// "1.5" → ("1", ".5"), "07" → "7".
fn split_number(raw: &str, seps: &[char]) -> (String, Option<String>, bool) {
    if !is_numeric(raw) {
        return (raw.to_string(), None, false);
    }
    match raw.find(seps) {
        None => match raw.trim().parse::<i64>() {
            Ok(n) => (n.to_string(), None, true),
            Err(_) => (raw.to_string(), None, false),
        },
        Some(p) => (raw[..p].to_string(), Some(raw[p..].to_string()), true),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FixedChooser {
        answer: Option<usize>,
        calls: RefCell<Vec<(ChoiceKind, Vec<String>)>>,
    }

    impl FixedChooser {
        fn new(answer: Option<usize>) -> Self {
            FixedChooser {
                answer,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Chooser for FixedChooser {
        fn choose(
            &self,
            kind: ChoiceKind,
            _caption: &str,
            c: &[Candidate],
            _t: Option<u32>,
        ) -> Option<usize> {
            self.calls
                .borrow_mut()
                .push((kind, c.iter().map(|c| c.label.clone()).collect()));
            self.answer
        }
    }

    const LETTER_B: &str = r#"
        <li><a href="https://www.bedetheque.com/serie-1-BD-Bidule.html">
            <span class="bdt-liste-flag"><img src="x.png"></span>
            <span class="bdt-liste-libelle"><b>Bidule</b></span></a></li>
        <li><a href="https://www.bedetheque.com/serie-500-BD-Blacksad.html">
            <span class="bdt-liste-flag"><img src="x.png" alt=""></span>
            <span class="bdt-liste-libelle"><b>Blacksad</b></span></a></li>"#;

    const SERIES: &str = r#"
        <div class="bdt-ah-by"><span class="bdt-sh-genres">
        <a class="bdt-tag bdt-sh-genre" href="x" title="Les séries du genre Policier">Policier</a></span>
        · <b class="bdt-sh-parution">Série en cours </b></div>
        <ul class="bdt-ah-pastilles">
        <li title="Nombre de tomes de la série">7 tomes</li>
        <li title="Langue de parution"><img src="France.png" class="bdt-sh-flag" alt=""> Français</li></ul>
        <p class="bdt-sh-resume">Tout sur la série Blacksad : Un détective &#039;chat&#039;.</p>
        <ul class="bdt-box-albums-liste">
            <li>
                <a href="https://www.bedetheque.com/BD-Blacksad-Tome-1-Quelque-part-entre-les-ombres-15161.html" title="Quelque part entre les ombres">
                <span class="bdt-box-albums-num" title="1">1<span class="bdt-numa"></span></span></a></li>
            <li>
                <a href="https://www.bedetheque.com/BD-Blacksad-Tome-2-Arctic-Nation-24600.html" title="Arctic-Nation">
                <span class="bdt-box-albums-num" title="2">2<span class="bdt-numa"></span></span></a></li>
            <li>
                <a href="https://www.bedetheque.com/BD-Blacksad-HS1-L-histoire-des-aquarelles-51853.html" title="L&#039;histoire des aquarelles">
                <span class="bdt-box-albums-num" title="HS1"><span class="bdt-numa">HS1</span></span></a></li>
        </ul>"#;

    fn edition(id: &str, alt: &str, extra: &str) -> String {
        format!(
            r#"<article class="bdt-edition bdt-edition--detail" id="ed-{id}">
            <a name="{id}"></a><div class="bdt-edition-head"><h3>
            Tome 1<span class="bdt-numa">{alt}</span></h3></div>
            <div class="bdt-edition-body"><div>
            <a class="bdt-img bdt-ecov browse-couvertures" href="https://www.bedetheque.com/media/Couvertures/Couv_{id}.jpg" title="x">
            </a></div><dl>
            <dt>Titre</dt><dd><b>Quelque part entre les ombres</b></dd>
            <dt>Identifiant</dt><dd><span class="bdt-id">{id}</span></dd>
            <dt>Scénario</dt>
            <dd><a href="a.html" title="Voir">Díaz Canalès, Juan</a></dd>
            <dt>Dessin</dt>
            <dd><a href="b.html" title="Voir">Guarnido, Juanjo</a></dd>
            <dt>Couleurs</dt>
            <dd><a href="b.html" title="Voir">Guarnido, Juanjo</a></dd>
            <dt>Dépôt légal</dt>
            <dd>11/2000</dd>
            <dt>Éditeur</dt>
            <dd>Dargaud</dd>
            {extra}
            <dt>Format</dt><dd>Grand format</dd>
            <dt>EAN/ISBN</dt><dd>978-2-205-04965-8</dd>
            <dt>Planches</dt><dd>46</dd>
            <dt>Créé le</dt><dd>24/05/2002</dd></dl>
            <div class="bdt-note"><div><b>Info édition</b>L&#039;EO avec DL.</div></div>
            </div></article>"#
        )
    }

    fn album_page() -> String {
        format!(
            r#"<div class="bdt-ah-top"><div>
            <h1><a href="https://www.bedetheque.com/serie-500-BD-Blacksad.html" title="Blacksad">Blacksad</a></h1>
            <h2 class="bdt-ah-sub">
            1<span class="bdt-numa"></span>. Quelque part entre les ombres</h2></div></div>
            <dl class="bdt-sheet">
            <dt>Série</dt><dd><a href="https://www.bedetheque.com/serie-500-BD-Blacksad.html">Blacksad</a></dd>
            </dl></div>
            Note: <span itemprop="ratingValue">4.4</span>/<span itemprop="bestRating">5</span>
            {}{}"#,
            edition("15161", "", ""),
            edition(
                "391756",
                "a2002",
                "<dt>Collection</dt><dd><a href=\"c.html\">Long Courrier</a></dd>"
            )
        )
    }

    const ALBUM1: &str =
        "https://www.bedetheque.com/BD-Blacksad-Tome-1-Quelque-part-entre-les-ombres-15161.html";

    fn scraper(settings: ScraperSettings, pages: &[(&str, String)]) -> Scraper {
        let mut s = Scraper::new(settings).unwrap();
        s.fake_pages = pages
            .iter()
            .map(|(u, p)| (full_url(u), p.clone()))
            .collect();
        s
    }

    fn blacksad_pages() -> Vec<(&'static str, String)> {
        vec![
            ("bandes_dessinees_B.html", LETTER_B.to_string()),
            (
                "https://www.bedetheque.com/serie-500-bd-blacksad__10000.html",
                SERIES.to_string(),
            ),
            (ALBUM1, album_page()),
            (
                "ajax/resume/album/15161",
                "Par un moche matin couleur s&eacute;pia.".to_string(),
            ),
        ]
    }

    fn book(series: &str, number: &str) -> Book {
        let mut b = Book::default();
        b.info.series = series.into();
        b.info.number = number.into();
        b
    }

    #[test]
    fn text_helpers() {
        assert_eq!(titlize("l'homme qui rit", true), "L'Homme Qui Rit");
        assert_eq!(titlize("tom's diner", true), "Tom's Diner");
        assert_eq!(titlize("abc ", true), "Abc");
        assert_eq!(remove_accents("Âme Rouge œuvre"), "ame Rouge oeuvre");
        assert_eq!(
            check_web_char("L&#039;Enfer &amp; &lt;N&amp;B&gt; &eacute;"),
            "L'Enfer & <N&B> é"
        );
        assert_eq!(
            strip_tags("<span class=\"x\">Blacksad</span> (en allemand)"),
            "Blacksad (en allemand)"
        );
        assert_eq!(parse_name("Díaz Canalès, Juan", false), "Juan Díaz Canalès");
        assert_eq!(parse_name("&lt;Collectif&gt;", false), "");
        assert_eq!(parse_name("&lt;Collectif&gt;", true), "<Collectif>");
        assert_eq!(zfill("7", 3), "007");
    }

    #[test]
    fn articles() {
        let a = "Le,La,Les,L',The";
        assert_eq!(format_articles("Les 5 Terres", a), "5 Terres (Les)");
        assert_eq!(format_articles("L'Ombre", a), "Ombre (L')");
        assert_eq!(
            format_articles("Les Mondes - Cycle 2", a),
            "Mondes (Les) - Cycle 2"
        );
        assert_eq!(format_articles("Leonard", a), "Leonard");
        assert_eq!(format_articles("les 5 terres", a), "Les 5 terres");
        assert_eq!(clean_articles("Les 5 Terres", a), "5 Terres");
        assert_eq!(clean_articles("Les Mondes - Cycle 2", a), "Mondes");
        assert_eq!(clean_articles("Blacksad (en allemand)", a), "Blacksad");
        assert_eq!(clean_articles("Thorgal: Kriss", a), "Thorgal");
        assert_eq!(clean_articles("##Test", a), "Test");
        assert_eq!(clean_articles("Leonard", a), "Leonard");
    }

    #[test]
    fn numbers() {
        let s = &['.', ',', '\\', '/'];
        assert_eq!(split_number("07", s), ("7".into(), None, true));
        assert_eq!(
            split_number("1.5", s),
            ("1".into(), Some(".5".into()), true)
        );
        assert_eq!(split_number("HS1", s), ("HS1".into(), None, false));
        assert_eq!(split_number("", s), ("".into(), None, false));
    }

    #[test]
    fn index_and_search() {
        assert_eq!(
            find_series_in_index(LETTER_B, "blacksad").as_deref(),
            Some("https://www.bedetheque.com/serie-500-BD-Blacksad.html")
        );
        assert_eq!(find_series_in_index(LETTER_B, "Black"), None);
        let page = r#"<h2>2 séries trouvées</h2><ul class="bdt-liste">
            <li><a href="https://www.bedetheque.com/serie-500-BD-Blacksad.html">
              <span class="bdt-liste-libelle"><span class="bdt-liste-l1"><b><span class="highlight">Blacksad</span></b></span></span>
              <span class="bdt-liste-meta">Policier</span></a></li>
            <li><a href="https://www.bedetheque.com/serie-86490-BD-Blacksad-en-allemand.html">
              <span class="bdt-liste-libelle"><span class="bdt-liste-l1"><b><span class="highlight">Blacksad</span> (en allemand)</b></span></span>
              <span class="bdt-liste-meta">Policier</span></a></li></ul>"#;
        let l = parse_search_results(page);
        assert_eq!(l.len(), 2);
        assert_eq!(
            l[0],
            ("serie-500-BD-Blacksad.html".into(), "Blacksad".into())
        );
        assert_eq!(l[1].1, "Blacksad (en allemand)");
    }

    #[test]
    fn series_fields() {
        assert_eq!(serie_resume(SERIES), "Un détective 'chat'.");
        assert_eq!(SERIE_LANGUE.captures(SERIES).unwrap()[1].trim(), "Français");
        let st = ScraperSettings::default();
        assert_eq!(series_count(SERIES, &st, YesNo::Unknown, true), 2);
        assert_eq!(series_count(SERIES, &st, YesNo::No, true), -1);
        let st2 = ScraperSettings {
            count_of: false,
            ..Default::default()
        };
        assert_eq!(series_count(SERIES, &st2, YesNo::Unknown, true), 7);
        assert_eq!(series_count(SERIES, &st2, YesNo::Unknown, false), -1);
    }

    #[test]
    fn artists_and_dates() {
        let info = edition(
            "1",
            "",
            "<dt>Lettrage</dt><dd>&lt;Indéterminé&gt;</dd><dt>Achevé impr. : </dt>\n<dd>03/2001<",
        );
        assert_eq!(
            generic_artists(&[&ALBUM_SCENAR, &ALBUM_STORYBOARD], &info, false),
            "Juan Díaz Canalès"
        );
        assert_eq!(generic_artists(&[&ALBUM_LETTRAGE], &info, false), "");
        assert_eq!(
            generic_artists(&[&ALBUM_LETTRAGE], &info, true),
            "<Indéterminé>"
        );
        assert_eq!(depot_legal(&info), (11, 2000));
        assert_eq!(acheve_imprimer(&info), Some((3, 2001)));
        assert_eq!(
            depot_legal("<dt>Dépôt légal</dt> <dd>-/2001</dd>"),
            (-1, -1)
        );
    }

    #[test]
    fn full_scrape_from_index() {
        let mut s = scraper(ScraperSettings::default(), &blacksad_pages());
        let ch = FixedChooser::new(None);
        let b = book("blacksad", "1");
        let ScrapeOutcome::Updated(r) = s.scrape(&b, &ch).unwrap() else {
            panic!("{:?}", s.log())
        };
        let i = &r.info;
        assert_eq!(i.series, "Blacksad");
        assert_eq!(i.number, "1");
        assert_eq!(i.alternate_number, "");
        assert_eq!(i.title, "Quelque Part Entre Les Ombres");
        assert_eq!(i.writer, "Juan Díaz Canalès");
        assert_eq!(i.penciller, "Juanjo Guarnido");
        assert_eq!(i.colorist, "Juanjo Guarnido");
        assert_eq!(i.black_and_white, YesNo::No);
        assert_eq!((i.month, i.year), (11, 2000));
        assert_eq!(i.publisher, "Dargaud");
        assert_eq!(i.format, "Grand format");
        assert_eq!(i.imprint, "");
        assert_eq!(r.isbn, "978-2-205-04965-8");
        assert_eq!(i.gtin, "9782205049658");
        assert_eq!(i.community_rating, 4.4);
        assert_eq!(i.genre, "Policier");
        assert_eq!(i.language_iso, "fr");
        assert_eq!(r.series_complete, YesNo::No);
        assert_eq!(i.count, -1);
        assert_eq!(i.web, ALBUM1);
        assert_eq!(
            i.summary,
            "Un détective 'chat'.\n\n>Quelque Part Entre Les Ombres< \nPar un moche matin couleur sépia.\n\nInfo édition: L'EO avec DL."
        );
        assert!(i.notes.starts_with("Bedetheque.com - "));
        assert_eq!(i.page_count, 46);
        assert!(ch.calls.borrow().is_empty());
    }

    #[test]
    fn edition_by_alt_number_and_disabled_fields() {
        let st = ScraperSettings {
            title: false,
            writer: false,
            short_web_link: true,
            pad_number: 2,
            tags: "BD".into(),
            ..Default::default()
        };
        let mut s = scraper(st, &blacksad_pages());
        let mut b = book("Blacksad", "1");
        b.info.alternate_number = "a2002".into();
        b.info.title = "garder".into();
        b.info.writer = "moi".into();
        let ScrapeOutcome::Updated(r) = s.scrape(&b, &FixedChooser::new(None)).unwrap() else {
            panic!()
        };
        assert_eq!(r.info.title, "garder");
        assert_eq!(r.info.writer, "moi");
        assert_eq!(r.info.number, "01");
        assert_eq!(r.info.alternate_number, "a2002");
        assert_eq!(r.info.imprint, "Long Courrier");
        assert_eq!(r.info.tags, "BD");
        assert_eq!(r.info.web, "www.bedetheque.com/BD--15161.html");
    }

    #[test]
    fn search_with_choice() {
        let search = r#"<h2>2 séries trouvées</h2>
            <li><a href="https://www.bedetheque.com/serie-500-BD-Blacksad.html"><span class="bdt-liste-libelle"><b>Blacksad (Intégrale)</b></span></a></li>
            <li><a href="https://www.bedetheque.com/serie-86490-BD-Blacksad-en-allemand.html"><span class="bdt-liste-libelle"><b>Blacksad (en allemand)</b></span></a></li>"#;
        let mut pages = blacksad_pages();
        pages[0].1 = "<html></html>".into();
        pages.push(("search/tout?RechTexte=blacksad&RechWhere=7", search.into()));
        // No unique exact match: ask.
        let mut s = scraper(ScraperSettings::default(), &pages);
        let ch = FixedChooser::new(Some(0));
        let ScrapeOutcome::Updated(r) = s.scrape(&book("Blacksad", "2x"), &ch).unwrap() else {
            panic!("{:?}", s.log())
        };
        assert_eq!(r.info.series, "Blacksad");
        let calls = ch.calls.borrow();
        assert_eq!(calls[0].0, ChoiceKind::Series);
        // "2x" missing from the list: all the albums are offered.
        assert_eq!(calls[1].0, ChoiceKind::Album);
        assert_eq!(
            calls[1].1,
            vec![
                "1. Quelque part entre les ombres",
                "2. Arctic-Nation",
                "HS1. L'histoire des aquarelles"
            ]
        );
        drop(calls);
        // Series choice cancelled.
        let mut s = scraper(ScraperSettings::default(), &pages);
        assert!(matches!(
            s.scrape(&book("Blacksad", "1"), &FixedChooser::new(None))
                .unwrap(),
            ScrapeOutcome::Skipped(_)
        ));
        // "Skip" mode: ambiguous → skipped without asking.
        let st = ScraperSettings {
            ambiguity: Ambiguity::Skip,
            ..Default::default()
        };
        let mut s = scraper(st, &pages);
        let ch = FixedChooser::new(Some(0));
        assert!(matches!(
            s.scrape(&book("Blacksad", "1"), &ch).unwrap(),
            ScrapeOutcome::Skipped(_)
        ));
        assert!(ch.calls.borrow().is_empty());
    }

    #[test]
    fn quick_scrape_and_rescrape() {
        let mut s = scraper(ScraperSettings::default(), &blacksad_pages());
        let ScrapeOutcome::Updated(r) = s
            .scrape_url(&Book::default(), ALBUM1, &FixedChooser::new(None))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(r.info.series, "Blacksad");
        assert_eq!(r.info.language_iso, "fr");

        let st = ScraperSettings {
            rescrape: true,
            ..Default::default()
        };
        let mut s = scraper(st, &blacksad_pages());
        let mut b = book("x", "9");
        assert!(matches!(
            s.scrape(&b, &FixedChooser::new(None)).unwrap(),
            ScrapeOutcome::Skipped(_)
        ));
        b.info.web =
            "www.bedetheque.com/BD-Blacksad-Tome-1-Quelque-part-entre-les-ombres-15161.html".into();
        let ScrapeOutcome::Updated(r) = s.scrape(&b, &FixedChooser::new(None)).unwrap() else {
            panic!("{:?}", s.log())
        };
        assert_eq!(r.info.number, "1");
        assert_eq!(r.info.web, b.info.web);

        // Series link: quick scrape with album choice.
        let mut s = scraper(ScraperSettings::default(), &blacksad_pages());
        let ch = FixedChooser::new(Some(0));
        let page = SERIES.replace("<ul class=\"bdt-ah-pastilles\">", "<h1><span>x</span><a href=\"https://www.bedetheque.com/serie-500-BD-Blacksad.html\">Blacksad</a></h1><ul class=\"bdt-ah-pastilles\">");
        s.fake_pages.insert(
            "https://www.bedetheque.com/serie-500-bd-blacksad__10000.html".into(),
            page,
        );
        let ScrapeOutcome::Updated(r) = s
            .scrape_url(
                &Book::default(),
                "https://www.bedetheque.com/serie-500-BD-Blacksad.html",
                &ch,
            )
            .unwrap()
        else {
            panic!("{:?}", s.log())
        };
        assert_eq!(r.info.title, "Quelque Part Entre Les Ombres");
        assert_eq!(r.info.count, -1);
        assert_eq!(ch.calls.borrow()[0].1.len(), 3);
    }

    #[test]
    fn revue() {
        let page = r#"<a name="1234"></a><li><div class="couv"><a class="browse-couvertures" href="https://www.bedetheque.com/media/Revues/r.jpg"><img></a></div>
            <h3 class="titre"><span class="numa">#12. </span>Le grand numéro</h3>
            <ul><li><label>Editeur : </label>Dupuis</li><li><label>Parution :</label>15/03/1985</li>
            <li><label>Périodicité : </label>Hebdomadaire</li></ul>
            <p><em>Sommaire :</em> Spirou &amp; Fantasio</p><div class="autres">x</div></li>"#;
        let url = "https://www.bedetheque.com/revue-12-BD-Spirou.html";
        let url2 = format!("{url}#1234");
        let mut s = scraper(
            ScraperSettings::default(),
            &[(url, page.to_string()), (&url2, page.to_string())],
        );
        let ScrapeOutcome::Updated(r) = s
            .scrape_url(&book("Spirou", "12"), url, &FixedChooser::new(Some(0)))
            .unwrap()
        else {
            panic!("{:?}", s.log())
        };
        assert_eq!(r.info.number, "12");
        assert_eq!(r.info.genre, "Revue");
        assert_eq!(r.info.publisher, "Dupuis");
        assert_eq!((r.info.month, r.info.year), (3, 1985));
        assert_eq!(r.info.format, "Hebdomadaire");
        assert_eq!(r.info.summary, "Spirou & Fantasio");
        assert_eq!(r.info.title, "Le Grand Numéro");
        assert_eq!(r.info.web, format!("{url}#1234"));
    }

    struct FirstChooser;
    impl Chooser for FirstChooser {
        fn choose(&self, _: ChoiceKind, _: &str, _: &[Candidate], _: Option<u32>) -> Option<usize> {
            Some(0)
        }
    }

    /// Live test against www.bedetheque.com: `cargo test -- --ignored live`.
    #[test]
    #[ignore]
    fn live_blacksad() {
        let mut s = Scraper::new(ScraperSettings {
            debug_log: true,
            ..Default::default()
        })
        .unwrap();
        let out = s.scrape(&book("Blacksad", "1"), &FirstChooser).unwrap();
        for l in s.log() {
            eprintln!("{l}");
        }
        let ScrapeOutcome::Updated(r) = out else {
            panic!("skipped")
        };
        eprintln!("{:#?}", r.info);
        assert_eq!(r.info.series, "Blacksad");
        assert!(r.info.writer.contains("Juan"));
        assert_eq!(r.info.publisher, "Dargaud");
        assert!(!r.info.summary.is_empty());
    }
}
