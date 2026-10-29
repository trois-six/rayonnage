//! Book model: ComicInfo.xml metadata plus the ComicRack-only properties,
//! and the `Field` table that drives columns, editors, templates and rules.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::util::{human_size, natural_cmp, number_cmp};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum YesNo {
    #[default]
    Unknown,
    No,
    Yes,
}

impl YesNo {
    pub fn as_str(self) -> &'static str {
        match self {
            YesNo::Unknown => "",
            YesNo::No => "No",
            YesNo::Yes => "Yes",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            YesNo::Unknown => "",
            YesNo::No => t!("No"),
            YesNo::Yes => t!("Yes"),
        }
    }
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "yes" | "oui" | "true" | "1" => YesNo::Yes,
            "no" | "non" | "false" | "0" => YesNo::No,
            _ => YesNo::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Manga {
    #[default]
    Unknown,
    No,
    Yes,
    YesAndRightToLeft,
}

impl Manga {
    pub fn as_str(self) -> &'static str {
        match self {
            Manga::Unknown => "",
            Manga::No => "No",
            Manga::Yes => "Yes",
            Manga::YesAndRightToLeft => "YesAndRightToLeft",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Manga::Unknown => "",
            Manga::No => t!("No"),
            Manga::Yes => t!("Yes"),
            Manga::YesAndRightToLeft => t!("Yes (right to left)"),
        }
    }
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "yes" | "oui" | "true" => Manga::Yes,
            "yesandrighttoleft" | "yes (right to left)" | "oui (droite à gauche)" => {
                Manga::YesAndRightToLeft
            }
            "no" | "non" | "false" => Manga::No,
            _ => Manga::Unknown,
        }
    }
}

/// One `<Page>` element of ComicInfo.xml.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageInfo {
    pub image: i32,
    /// FrontCover, InnerCover, Roundup, Story, Advertisement, Editorial,
    /// Letters, Preview, BackCover, Other, Deleted
    pub page_type: String,
    pub double_page: bool,
    pub image_size: i64,
    pub key: String,
    pub bookmark: String,
    pub image_width: i32,
    pub image_height: i32,
    /// None, Rotate90, Rotate180, Rotate270
    pub rotation: String,
}

pub const PAGE_TYPES: &[&str] = &[
    "FrontCover",
    "InnerCover",
    "Roundup",
    "Story",
    "Advertisement",
    "Editorial",
    "Letters",
    "Preview",
    "BackCover",
    "Other",
    "Deleted",
];

/// Everything that lives in ComicInfo.xml. Integer fields use -1 for "unset"
/// like ComicRack does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComicInfo {
    pub title: String,
    pub series: String,
    pub number: String,
    pub count: i32,
    pub volume: i32,
    pub alternate_series: String,
    pub alternate_number: String,
    pub alternate_count: i32,
    pub summary: String,
    pub notes: String,
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub writer: String,
    pub penciller: String,
    pub inker: String,
    pub colorist: String,
    pub letterer: String,
    pub cover_artist: String,
    pub editor: String,
    pub translator: String,
    pub publisher: String,
    pub imprint: String,
    pub genre: String,
    pub tags: String,
    pub web: String,
    pub page_count: i32,
    pub language_iso: String,
    pub format: String,
    pub black_and_white: YesNo,
    pub manga: Manga,
    pub characters: String,
    pub teams: String,
    pub locations: String,
    pub scan_information: String,
    pub story_arc: String,
    pub story_arc_number: String,
    pub series_group: String,
    pub age_rating: String,
    pub community_rating: f32,
    pub main_character_or_team: String,
    pub review: String,
    pub gtin: String,
    pub pages: Vec<PageInfo>,
    /// Unknown simple elements, preserved when rewriting the file.
    pub extra: Vec<(String, String)>,
}

impl Default for ComicInfo {
    fn default() -> Self {
        ComicInfo {
            title: String::new(),
            series: String::new(),
            number: String::new(),
            count: -1,
            volume: -1,
            alternate_series: String::new(),
            alternate_number: String::new(),
            alternate_count: -1,
            summary: String::new(),
            notes: String::new(),
            year: -1,
            month: -1,
            day: -1,
            writer: String::new(),
            penciller: String::new(),
            inker: String::new(),
            colorist: String::new(),
            letterer: String::new(),
            cover_artist: String::new(),
            editor: String::new(),
            translator: String::new(),
            publisher: String::new(),
            imprint: String::new(),
            genre: String::new(),
            tags: String::new(),
            web: String::new(),
            page_count: 0,
            language_iso: String::new(),
            format: String::new(),
            black_and_white: YesNo::Unknown,
            manga: Manga::Unknown,
            characters: String::new(),
            teams: String::new(),
            locations: String::new(),
            scan_information: String::new(),
            story_arc: String::new(),
            story_arc_number: String::new(),
            series_group: String::new(),
            age_rating: String::new(),
            community_rating: 0.0,
            main_character_or_team: String::new(),
            review: String::new(),
            gtin: String::new(),
            pages: Vec::new(),
            extra: Vec::new(),
        }
    }
}

/// A book in the library.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub id: u64,
    pub file_path: PathBuf,
    pub file_size: u64,
    pub file_created: Option<DateTime<Local>>,
    pub file_modified: Option<DateTime<Local>>,
    pub added: DateTime<Local>,
    pub info: ComicInfo,
    // ComicRack-only properties (stored in the library database, not in the file).
    pub rating: f32,
    pub series_complete: YesNo,
    pub opened: Option<DateTime<Local>>,
    pub open_count: i32,
    pub last_page_read: i32,
    pub book_price: String,
    pub book_age: String,
    pub book_condition: String,
    pub book_store: String,
    pub book_owner: String,
    pub book_collection_status: String,
    pub book_notes: String,
    pub book_location: String,
    pub isbn: String,
    pub custom_values: BTreeMap<String, String>,
    pub checked: bool,
    /// Metadata changed in the library but not yet written to the file.
    pub info_dirty: bool,
    /// Proposed values computed from the file name (sorting asks often).
    #[serde(skip)]
    pub proposed_cache: ProposedCache,
}

#[derive(Clone, Debug, Default)]
pub struct ProposedCache(std::sync::OnceLock<(PathBuf, Proposed)>);

impl Default for Book {
    fn default() -> Self {
        Book {
            id: 0,
            file_path: PathBuf::new(),
            file_size: 0,
            file_created: None,
            file_modified: None,
            added: Local::now(),
            info: ComicInfo::default(),
            rating: 0.0,
            series_complete: YesNo::Unknown,
            opened: None,
            open_count: 0,
            last_page_read: 0,
            book_price: String::new(),
            book_age: String::new(),
            book_condition: String::new(),
            book_store: String::new(),
            book_owner: String::new(),
            book_collection_status: String::new(),
            book_notes: String::new(),
            book_location: String::new(),
            isbn: String::new(),
            custom_values: BTreeMap::new(),
            checked: true,
            info_dirty: false,
            proposed_cache: ProposedCache::default(),
        }
    }
}

impl Book {
    pub fn file_name(&self) -> String {
        self.file_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn file_stem(&self) -> String {
        self.file_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn file_directory(&self) -> String {
        self.file_path
            .parent()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn file_format(&self) -> String {
        crate::archive::BookFormat::from_path(&self.file_path)
            .map(|f| f.label().to_string())
            .unwrap_or_default()
    }

    pub fn read_percentage(&self) -> i32 {
        if self.info.page_count <= 0 {
            return 0;
        }
        ((self.last_page_read.max(0) as f64 + 1.0) * 100.0 / self.info.page_count as f64)
            .min(100.0) as i32
            * (self.last_page_read > 0) as i32
    }

    pub fn released(&self) -> Option<NaiveDate> {
        if self.info.year <= 0 {
            return None;
        }
        NaiveDate::from_ymd_opt(
            self.info.year,
            self.info.month.clamp(1, 12) as u32,
            self.info.day.clamp(1, 28) as u32,
        )
    }

    /// The ComicRack "caption": "Series Vol.N #Number - Title (Month Year)".
    pub fn caption(&self) -> String {
        let i = &self.info;
        if i.series.is_empty() && i.title.is_empty() {
            return self.file_stem();
        }
        let mut s = i.series.clone();
        if i.volume >= 0 {
            s.push_str(&format!(" Vol.{}", i.volume));
        }
        if !i.number.is_empty() {
            s.push_str(&format!(" #{}", i.number));
        }
        if i.count > 0 {
            s.push_str(&t!(" (of {0})", i.count));
        }
        if !i.title.is_empty() {
            if !s.is_empty() {
                s.push_str(" - ");
            }
            s.push_str(&i.title);
        }
        if i.year > 0 {
            if i.month > 0 {
                s.push_str(&format!(" ({} {})", month_name(i.month), i.year));
            } else {
                s.push_str(&format!(" ({})", i.year));
            }
        }
        s.trim().to_string()
    }

    pub fn proposed(&self) -> Proposed {
        let cache = &self.proposed_cache.0;
        if let Some((path, p)) = cache.get()
            && *path == self.file_path
        {
            return p.clone();
        }
        let p = propose_from_filename(&self.file_stem());
        let _ = cache.set((self.file_path.clone(), p.clone()));
        p
    }

    /// Copy with empty Series/Number/Volume/Year filled from the proposed values
    /// (ComicRack's "Shadow" properties, used by the plugins).
    pub fn with_proposed(&self) -> Book {
        let mut b = self.clone();
        if b.info.series.is_empty() && b.info.number.is_empty() {
            let p = self.proposed();
            b.info.series = p.series;
            b.info.number = p.number;
            if b.info.volume < 0 {
                b.info.volume = p.volume;
            }
            if b.info.year < 0 {
                b.info.year = p.year;
            }
        }
        b
    }

    /// Builds a book from a file on disk (without reading the archive content).
    pub fn from_file(path: PathBuf) -> Book {
        let mut b = Book {
            file_path: path,
            ..Default::default()
        };
        b.refresh_file_stats();
        b
    }

    pub fn refresh_file_stats(&mut self) {
        if let Ok(md) = std::fs::metadata(&self.file_path) {
            self.file_size = if md.is_dir() {
                walkdir::WalkDir::new(&self.file_path)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            } else {
                md.len()
            };
            self.file_modified = md.modified().ok().map(DateTime::<Local>::from);
            self.file_created = md.created().ok().map(DateTime::<Local>::from);
        }
    }
}

/// Values guessed from the file name, used when the real ones are empty
/// (ComicRack's "proposed values", shown greyed out).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proposed {
    pub series: String,
    pub number: String,
    pub volume: i32,
    pub year: i32,
    pub title: String,
}

pub fn propose_from_filename(stem: &str) -> Proposed {
    use std::sync::LazyLock;
    static YEAR: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"[(\[]((?:19|20)\d{2})[)\]]").unwrap());
    static BRACKETS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\([^)]*\)|\[[^\]]*\]|\{[^}]*\}").unwrap());
    static VOLUME: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)\b(?:v|vol\.?|volume)\s*(\d{1,4})\b").unwrap());
    static NUMBER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)^(?P<series>.*?)[\s\-.,]*(?:\b(?:t|tome|n°|no\.?|nr\.?|issue|ch\.?|chapitre|chapter)\s*|#\s*)?(?P<num>\d+(?:\.\d+)?)(?:\s+(?:-\s*)?(?P<title>\D.*))?$",
        )
        .unwrap()
    });
    let mut p = Proposed { volume: -1, year: -1, ..Default::default() };
    let mut s = stem.replace('_', " ");
    if let Some(c) = YEAR.captures(&s) {
        p.year = c[1].parse().unwrap_or(-1);
    }
    s = BRACKETS.replace_all(&s, " ").into_owned();
    if let Some(c) = VOLUME.captures(&s) {
        p.volume = c[1].parse().unwrap_or(-1);
        s = VOLUME.replace(&s, " ").into_owned();
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match NUMBER.captures(&s) {
        Some(c) => {
            p.series = c["series"].trim().trim_end_matches(['-', ',', '.']).trim().to_string();
            let n = c["num"].trim_start_matches('0');
            p.number = if n.is_empty() || n.starts_with('.') { format!("0{n}") } else { n.to_string() };
            p.title = c.name("title").map(|t| t.as_str().trim().to_string()).unwrap_or_default();
        }
        None => p.series = s.trim().to_string(),
    }
    p
}

/// English month names, translated by `month_name`.
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// French month names (data for French-language formats, whatever the
/// interface language).
pub const MONTHS_FR: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

pub fn month_name(m: i32) -> &'static str {
    if (1..=12).contains(&m) {
        crate::i18n::tr(MONTHS[(m - 1) as usize])
    } else {
        ""
    }
}

fn fmt_int(v: i32) -> String {
    if v < 0 { String::new() } else { v.to_string() }
}

fn parse_int(s: &str) -> i32 {
    let s = s.trim();
    if s.is_empty() {
        -1
    } else {
        s.parse().unwrap_or(-1)
    }
}

fn fmt_float(v: f32) -> String {
    if v <= 0.0 {
        String::new()
    } else if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn parse_float(s: &str) -> f32 {
    s.trim().replace(',', ".").parse().unwrap_or(0.0)
}

fn fmt_date(d: &Option<DateTime<Local>>) -> String {
    d.map(|d| d.format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_default()
}

/// How a field is displayed/edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    /// Comma separated list with auto-completion (writers, tags, ...).
    List,
    LongText,
    Int,
    /// Issue number (string compared numerically).
    Number,
    YesNo,
    Manga,
    Rating,
    /// Computed/read-only value.
    ReadOnly,
}

macro_rules! fields {
    ($( $v:ident => $key:literal, $label:literal, $kind:ident; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum Field { $($v),* }

        impl Field {
            pub const ALL: &'static [Field] = &[$(Field::$v),*];
            /// Stable identifier (ComicRack/ComicInfo property name).
            pub fn key(self) -> &'static str { match self { $(Field::$v => $key),* } }
            /// Name shown in the interface (translated).
            pub fn label(self) -> &'static str { match self { $(Field::$v => crate::i18n::tr($label)),* } }
            pub fn kind(self) -> FieldKind { match self { $(Field::$v => FieldKind::$kind),* } }
        }
    };
}

fields! {
    Series => "Series", "Series", Text;
    Number => "Number", "Number", Number;
    Count => "Count", "Count", Int;
    Volume => "Volume", "Volume", Int;
    Title => "Title", "Title", Text;
    AlternateSeries => "AlternateSeries", "Alternate Series", Text;
    AlternateNumber => "AlternateNumber", "Alternate Number", Number;
    AlternateCount => "AlternateCount", "Alternate Count", Int;
    StoryArc => "StoryArc", "Story Arc", List;
    StoryArcNumber => "StoryArcNumber", "Story Arc Number", Text;
    SeriesGroup => "SeriesGroup", "Series Group", List;
    Year => "Year", "Year", Int;
    Month => "Month", "Month", Int;
    Day => "Day", "Day", Int;
    Writer => "Writer", "Writer", List;
    Penciller => "Penciller", "Penciller", List;
    Inker => "Inker", "Inker", List;
    Colorist => "Colorist", "Colorist", List;
    Letterer => "Letterer", "Letterer", List;
    CoverArtist => "CoverArtist", "Cover Artist", List;
    Editor => "Editor", "Editor", List;
    Translator => "Translator", "Translator", List;
    Publisher => "Publisher", "Publisher", Text;
    Imprint => "Imprint", "Imprint", Text;
    Genre => "Genre", "Genre", List;
    Tags => "Tags", "Tags", List;
    Characters => "Characters", "Characters", List;
    Teams => "Teams", "Teams", List;
    MainCharacterOrTeam => "MainCharacterOrTeam", "Main Character/Team", Text;
    Locations => "Locations", "Locations", List;
    Format => "Format", "Format", Text;
    AgeRating => "AgeRating", "Age Rating", Text;
    LanguageISO => "LanguageISO", "Language", Text;
    Manga => "Manga", "Manga", Manga;
    BlackAndWhite => "BlackAndWhite", "Black and White", YesNo;
    SeriesComplete => "SeriesComplete", "Series complete", YesNo;
    Web => "Web", "Web", Text;
    Summary => "Summary", "Summary", LongText;
    Notes => "Notes", "Notes", LongText;
    Review => "Review", "Review", LongText;
    ScanInformation => "ScanInformation", "Scan Information", Text;
    PageCount => "PageCount", "Pages", Int;
    Rating => "Rating", "My Rating", Rating;
    CommunityRating => "CommunityRating", "Community Rating", Rating;
    Gtin => "GTIN", "GTIN", Text;
    Isbn => "ISBN", "ISBN", Text;
    BookPrice => "BookPrice", "Book Price", Text;
    BookAge => "BookAge", "Book Age", Text;
    BookCondition => "BookCondition", "Book Condition", Text;
    BookStore => "BookStore", "Book Store", Text;
    BookOwner => "BookOwner", "Book Owner", Text;
    BookCollectionStatus => "BookCollectionStatus", "Collection Status", Text;
    BookNotes => "BookNotes", "Book Notes", LongText;
    BookLocation => "BookLocation", "Book Location", Text;
    ReadPercentage => "ReadPercentage", "Read Percentage", ReadOnly;
    Opened => "Opened", "Opened", ReadOnly;
    Added => "Added", "Added", ReadOnly;
    Released => "Released", "Released", ReadOnly;
    FileName => "FileName", "File Name", ReadOnly;
    FilePath => "FilePath", "File Path", ReadOnly;
    FileDirectory => "FileDirectory", "File Directory", ReadOnly;
    FileSize => "FileSize", "File Size", ReadOnly;
    FileFormat => "FileFormat", "File Format", ReadOnly;
    FileCreated => "FileCreated", "File Created", ReadOnly;
    FileModified => "FileModified", "File Modified", ReadOnly;
    Caption => "Caption", "Caption", ReadOnly;
    Modified => "InfoModified", "Info Modified", ReadOnly;
}

impl Field {
    pub fn from_key(key: &str) -> Option<Field> {
        Field::ALL
            .iter()
            .copied()
            .find(|f| f.key().eq_ignore_ascii_case(key))
    }

    /// Fields editable by the user.
    pub fn is_editable(self) -> bool {
        self.kind() != FieldKind::ReadOnly
    }

    /// Fields stored in ComicInfo.xml (and thus written into book files).
    pub fn in_comicinfo(self) -> bool {
        !matches!(
            self,
            Field::Rating
                | Field::SeriesComplete
                | Field::Isbn
                | Field::BookPrice
                | Field::BookAge
                | Field::BookCondition
                | Field::BookStore
                | Field::BookOwner
                | Field::BookCollectionStatus
                | Field::BookNotes
                | Field::BookLocation
        ) && self.is_editable()
    }

    /// Value as displayed in the list and used by templates.
    pub fn get(self, b: &Book) -> String {
        let i = &b.info;
        match self {
            Field::Series => i.series.clone(),
            Field::Number => i.number.clone(),
            Field::Count => fmt_int(i.count),
            Field::Volume => fmt_int(i.volume),
            Field::Title => i.title.clone(),
            Field::AlternateSeries => i.alternate_series.clone(),
            Field::AlternateNumber => i.alternate_number.clone(),
            Field::AlternateCount => fmt_int(i.alternate_count),
            Field::StoryArc => i.story_arc.clone(),
            Field::StoryArcNumber => i.story_arc_number.clone(),
            Field::SeriesGroup => i.series_group.clone(),
            Field::Year => fmt_int(i.year),
            Field::Month => fmt_int(i.month),
            Field::Day => fmt_int(i.day),
            Field::Writer => i.writer.clone(),
            Field::Penciller => i.penciller.clone(),
            Field::Inker => i.inker.clone(),
            Field::Colorist => i.colorist.clone(),
            Field::Letterer => i.letterer.clone(),
            Field::CoverArtist => i.cover_artist.clone(),
            Field::Editor => i.editor.clone(),
            Field::Translator => i.translator.clone(),
            Field::Publisher => i.publisher.clone(),
            Field::Imprint => i.imprint.clone(),
            Field::Genre => i.genre.clone(),
            Field::Tags => i.tags.clone(),
            Field::Characters => i.characters.clone(),
            Field::Teams => i.teams.clone(),
            Field::MainCharacterOrTeam => i.main_character_or_team.clone(),
            Field::Locations => i.locations.clone(),
            Field::Format => i.format.clone(),
            Field::AgeRating => i.age_rating.clone(),
            Field::LanguageISO => i.language_iso.clone(),
            Field::Manga => i.manga.as_str().to_string(),
            Field::BlackAndWhite => i.black_and_white.as_str().to_string(),
            Field::SeriesComplete => b.series_complete.as_str().to_string(),
            Field::Web => i.web.clone(),
            Field::Summary => i.summary.clone(),
            Field::Notes => i.notes.clone(),
            Field::Review => i.review.clone(),
            Field::ScanInformation => i.scan_information.clone(),
            Field::PageCount => fmt_int(if i.page_count > 0 { i.page_count } else { -1 }),
            Field::Rating => fmt_float(b.rating),
            Field::CommunityRating => fmt_float(i.community_rating),
            Field::Gtin => i.gtin.clone(),
            Field::Isbn => b.isbn.clone(),
            Field::BookPrice => b.book_price.clone(),
            Field::BookAge => b.book_age.clone(),
            Field::BookCondition => b.book_condition.clone(),
            Field::BookStore => b.book_store.clone(),
            Field::BookOwner => b.book_owner.clone(),
            Field::BookCollectionStatus => b.book_collection_status.clone(),
            Field::BookNotes => b.book_notes.clone(),
            Field::BookLocation => b.book_location.clone(),
            Field::ReadPercentage => b.read_percentage().to_string(),
            Field::Opened => fmt_date(&b.opened),
            Field::Added => fmt_date(&Some(b.added)),
            Field::Released => b
                .released()
                .map(|d| {
                    if i.day > 0 {
                        d.format("%d/%m/%Y").to_string()
                    } else if i.month > 0 {
                        format!("{:02}/{}", d.month(), d.year())
                    } else {
                        d.year().to_string()
                    }
                })
                .unwrap_or_default(),
            Field::FileName => b.file_name(),
            Field::FilePath => b.file_path.to_string_lossy().into_owned(),
            Field::FileDirectory => b.file_directory(),
            Field::FileSize => b.file_size.to_string(),
            Field::FileFormat => b.file_format(),
            Field::FileCreated => fmt_date(&b.file_created),
            Field::FileModified => fmt_date(&b.file_modified),
            Field::Caption => b.caption(),
            Field::Modified => if b.info_dirty { "Yes" } else { "" }.to_string(),
        }
    }

    /// Proposed value shown greyed out when the field is empty.
    pub fn proposed(self, b: &Book) -> Option<String> {
        if !b.info.series.is_empty() || !b.info.number.is_empty() {
            return None;
        }
        let p = b.proposed();
        let v = match self {
            Field::Series => p.series,
            Field::Number => p.number,
            Field::Volume if b.info.volume < 0 => fmt_int(p.volume),
            Field::Year if b.info.year < 0 => fmt_int(p.year),
            _ => return None,
        };
        (!v.is_empty()).then_some(v)
    }

    /// Value formatted for the list view (localized yes/no, human sizes...).
    pub fn display(self, b: &Book) -> String {
        match self {
            Field::Manga => b.info.manga.label().to_string(),
            Field::BlackAndWhite => b.info.black_and_white.label().to_string(),
            Field::SeriesComplete => b.series_complete.label().to_string(),
            Field::FileSize => human_size(b.file_size),
            Field::ReadPercentage => {
                let p = b.read_percentage();
                if p > 0 { t!("{0}%", p) } else { String::new() }
            }
            Field::Rating | Field::CommunityRating => {
                let v = if self == Field::Rating { b.rating } else { b.info.community_rating };
                if v <= 0.0 {
                    String::new()
                } else {
                    let full = v.round().clamp(0.0, 5.0) as usize;
                    format!("{}{}", "★".repeat(full), "☆".repeat(5 - full))
                }
            }
            Field::Month => month_name(b.info.month).to_string(),
            Field::Summary | Field::Notes | Field::Review | Field::BookNotes => {
                self.get(b).lines().next().unwrap_or("").to_string()
            }
            Field::Modified => if b.info_dirty { "✎" } else { "" }.to_string(),
            _ => self.get(b),
        }
    }

    pub fn set(self, b: &mut Book, v: &str) {
        let i = &mut b.info;
        let s = v.to_string();
        match self {
            Field::Series => i.series = s,
            Field::Number => i.number = s,
            Field::Count => i.count = parse_int(v),
            Field::Volume => i.volume = parse_int(v),
            Field::Title => i.title = s,
            Field::AlternateSeries => i.alternate_series = s,
            Field::AlternateNumber => i.alternate_number = s,
            Field::AlternateCount => i.alternate_count = parse_int(v),
            Field::StoryArc => i.story_arc = s,
            Field::StoryArcNumber => i.story_arc_number = s,
            Field::SeriesGroup => i.series_group = s,
            Field::Year => i.year = parse_int(v),
            Field::Month => i.month = parse_int(v),
            Field::Day => i.day = parse_int(v),
            Field::Writer => i.writer = s,
            Field::Penciller => i.penciller = s,
            Field::Inker => i.inker = s,
            Field::Colorist => i.colorist = s,
            Field::Letterer => i.letterer = s,
            Field::CoverArtist => i.cover_artist = s,
            Field::Editor => i.editor = s,
            Field::Translator => i.translator = s,
            Field::Publisher => i.publisher = s,
            Field::Imprint => i.imprint = s,
            Field::Genre => i.genre = s,
            Field::Tags => i.tags = s,
            Field::Characters => i.characters = s,
            Field::Teams => i.teams = s,
            Field::MainCharacterOrTeam => i.main_character_or_team = s,
            Field::Locations => i.locations = s,
            Field::Format => i.format = s,
            Field::AgeRating => i.age_rating = s,
            Field::LanguageISO => i.language_iso = s,
            Field::Manga => i.manga = Manga::parse(v),
            Field::BlackAndWhite => i.black_and_white = YesNo::parse(v),
            Field::SeriesComplete => b.series_complete = YesNo::parse(v),
            Field::Web => i.web = s,
            Field::Summary => i.summary = s,
            Field::Notes => i.notes = s,
            Field::Review => i.review = s,
            Field::ScanInformation => i.scan_information = s,
            Field::PageCount => i.page_count = parse_int(v).max(0),
            Field::Rating => b.rating = parse_float(v).clamp(0.0, 5.0),
            Field::CommunityRating => i.community_rating = parse_float(v).clamp(0.0, 5.0),
            Field::Gtin => i.gtin = s,
            Field::Isbn => b.isbn = s,
            Field::BookPrice => b.book_price = s,
            Field::BookAge => b.book_age = s,
            Field::BookCondition => b.book_condition = s,
            Field::BookStore => b.book_store = s,
            Field::BookOwner => b.book_owner = s,
            Field::BookCollectionStatus => b.book_collection_status = s,
            Field::BookNotes => b.book_notes = s,
            Field::BookLocation => b.book_location = s,
            _ => {}
        }
    }

    pub fn compare(self, a: &Book, b: &Book) -> Ordering {
        if matches!(self, Field::Series | Field::Number) {
            let va = self.proposed(a).unwrap_or_else(|| self.get(a));
            let vb = self.proposed(b).unwrap_or_else(|| self.get(b));
            return if self == Field::Number { number_cmp(&va, &vb) } else { natural_cmp(&va, &vb) };
        }
        match self {
            Field::Number | Field::AlternateNumber | Field::StoryArcNumber => {
                number_cmp(&self.get(a), &self.get(b))
            }
            Field::Count
            | Field::Volume
            | Field::AlternateCount
            | Field::Year
            | Field::Month
            | Field::Day
            | Field::PageCount
            | Field::ReadPercentage => {
                let pa: i64 = self.get(a).parse().unwrap_or(i64::MIN);
                let pb: i64 = self.get(b).parse().unwrap_or(i64::MIN);
                pa.cmp(&pb)
            }
            Field::FileSize => a.file_size.cmp(&b.file_size),
            Field::Rating => a.rating.total_cmp(&b.rating),
            Field::CommunityRating => a.info.community_rating.total_cmp(&b.info.community_rating),
            Field::Added => a.added.cmp(&b.added),
            Field::Opened => a.opened.cmp(&b.opened),
            Field::FileCreated => a.file_created.cmp(&b.file_created),
            Field::FileModified => a.file_modified.cmp(&b.file_modified),
            Field::Released => a.released().cmp(&b.released()),
            _ => natural_cmp(&self.get(a), &self.get(b)),
        }
    }
}

/// The default ComicRack sort: Series, Volume, Number, Year, File name.
pub fn default_book_cmp(a: &Book, b: &Book) -> Ordering {
    Field::Series
        .compare(a, b)
        .then_with(|| Field::Volume.compare(a, b))
        .then_with(|| Field::Number.compare(a, b))
        .then_with(|| Field::Year.compare(a, b))
        .then_with(|| natural_cmp(&a.file_name(), &b.file_name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposed() {
        let p = propose_from_filename("Blacksad - T01 - Quelque part (2000)");
        assert_eq!((p.series.as_str(), p.number.as_str(), p.year, p.title.as_str()), ("Blacksad", "1", 2000, "Quelque part"));
        let p = propose_from_filename("one_piece_001");
        assert_eq!((p.series.as_str(), p.number.as_str()), ("one piece", "1"));
        let p = propose_from_filename("Batman v2 #12 [scan]");
        assert_eq!((p.series.as_str(), p.number.as_str(), p.volume), ("Batman", "12", 2));
        let p = propose_from_filename("Asterix 05");
        assert_eq!((p.series.as_str(), p.number.as_str()), ("Asterix", "5"));
        let p = propose_from_filename("Les Schtroumpfs");
        assert_eq!((p.series.as_str(), p.number.as_str()), ("Les Schtroumpfs", ""));
    }

    #[test]
    fn get_set_roundtrip() {
        let mut b = Book::default();
        for f in Field::ALL.iter().filter(|f| f.is_editable()) {
            let v = match f.kind() {
                FieldKind::Int => "12",
                FieldKind::YesNo => "Yes",
                FieldKind::Manga => "YesAndRightToLeft",
                FieldKind::Rating => "4",
                _ => "abc",
            };
            f.set(&mut b, v);
            assert_eq!(f.get(&b), v, "{f:?}");
        }
    }
}
