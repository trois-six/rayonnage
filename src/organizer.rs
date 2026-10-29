//! Library Organizer: a port of the ComicRack "Library Organizer" plugin
//! (Stonepaw, v2.1.13) that moves / copies / renames book files from
//! templates such as `{<publisher>}\{<series>}{ (<startyear>)}`.
//!
//! Template syntax: every field is written `{prefix<name args>postfix}`. The
//! block is replaced by `prefix + value + postfix` when the value is not empty,
//! by the profile's empty-data substitution (without prefix/postfix) otherwise.
//! Blocks may be nested (`{ ({<month>, }<year>)}`), they are resolved from the
//! inside out. Arguments are either a padding number (`<number3>`, `0` = auto)
//! or parenthesised values (`<writer(, )(issue)>`, `<manga(Manga)(!)>`,
//! `<read(Lu)(>)(90)>`, `<counter(1)(1)(3)>`, `<first(Series)>`,
//! `<AddedDate(dd/MM/yy)>`, `<Custom(key)>`). `<?name(text)>` inserts the
//! prefix/postfix when the value equals `text` (`(!regex)` for a regex);
//! `<!name>` / `<!name(text)>` / `<!name(!regex)>` is the inverse.
//! In folder templates both `\` and `/` separate folders.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{Datelike, NaiveDateTime, Timelike};
use regex::{Captures, Regex};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::{Book, Field, FieldKind, MONTHS, MONTHS_FR, Manga, YesNo};
use crate::i18n::tr;
use crate::util::{natural_cmp, number_cmp, split_list};

// ---------------------------------------------------------------------------
// Fields
// ---------------------------------------------------------------------------

/// A field usable in rules, empty-data substitutions and "fail on empty":
/// a regular book [`Field`] or one of the values computed over the series.
/// Serialized as its key string (`"Series"`, `"StartYear"`...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OrgField {
    Book(Field),
    StartYear,
    StartMonth,
    EndYear,
    EndMonth,
    FirstLetter,
    FirstIssueNumber,
    LastIssueNumber,
}

impl From<Field> for OrgField {
    fn from(f: Field) -> Self {
        OrgField::Book(f)
    }
}

impl OrgField {
    pub fn key(self) -> &'static str {
        match self {
            OrgField::Book(f) => f.key(),
            OrgField::StartYear => "StartYear",
            OrgField::StartMonth => "StartMonth",
            OrgField::EndYear => "EndYear",
            OrgField::EndMonth => "EndMonth",
            OrgField::FirstLetter => "FirstLetter",
            OrgField::FirstIssueNumber => "FirstIssueNumber",
            OrgField::LastIssueNumber => "LastIssueNumber",
        }
    }

    pub fn from_key(key: &str) -> Option<OrgField> {
        Some(match key {
            "StartYear" => OrgField::StartYear,
            "StartMonth" => OrgField::StartMonth,
            "EndYear" => OrgField::EndYear,
            "EndMonth" => OrgField::EndMonth,
            "FirstLetter" => OrgField::FirstLetter,
            "FirstIssueNumber" => OrgField::FirstIssueNumber,
            "LastIssueNumber" => OrgField::LastIssueNumber,
            _ => return Field::from_key(key).map(OrgField::Book),
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            OrgField::Book(f) => f.label(),
            OrgField::StartYear => t!("Start Year"),
            OrgField::StartMonth => t!("Start Month"),
            OrgField::EndYear => t!("End Year"),
            OrgField::EndMonth => t!("End Month"),
            OrgField::FirstLetter => t!("First Letter"),
            OrgField::FirstIssueNumber => t!("First Issue Number"),
            OrgField::LastIssueNumber => t!("Last Issue Number"),
        }
    }

    /// Yes/No fields only accept `Is`/`IsNot` and the [`YES_NO_VALUES`]
    /// (or [`MANGA_VALUES`]) values in rules.
    pub fn is_yes_no(self) -> bool {
        matches!(
            self,
            OrgField::Book(Field::Manga | Field::SeriesComplete | Field::BlackAndWhite)
        )
    }
}

impl Serialize for OrgField {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.key())
    }
}

impl<'de> Deserialize<'de> for OrgField {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        OrgField::from_key(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown field: {s}")))
    }
}

const fn bf(f: Field) -> OrgField {
    OrgField::Book(f)
}

/// Fields offered in metadata rules (plugin list + Manga).
pub const RULE_FIELDS: &[OrgField] = &[
    bf(Field::AgeRating),
    bf(Field::AlternateCount),
    bf(Field::AlternateNumber),
    bf(Field::AlternateSeries),
    bf(Field::BlackAndWhite),
    bf(Field::Characters),
    bf(Field::Count),
    bf(Field::FileName),
    bf(Field::FilePath),
    bf(Field::FileFormat),
    bf(Field::Manga),
    bf(Field::Notes),
    bf(Field::Publisher),
    bf(Field::Rating),
    bf(Field::ReadPercentage),
    bf(Field::Review),
    bf(Field::SeriesComplete),
    bf(Field::Tags),
    bf(Field::Teams),
    bf(Field::Title),
    bf(Field::ScanInformation),
    bf(Field::Series),
    bf(Field::SeriesGroup),
    OrgField::StartMonth,
    OrgField::StartYear,
    bf(Field::StoryArc),
    bf(Field::Volume),
    bf(Field::Web),
    bf(Field::Year),
];

/// Fields offered for "fail when empty".
pub const FAILED_FIELDS: &[OrgField] = &[
    bf(Field::AgeRating),
    bf(Field::AlternateCount),
    bf(Field::AlternateNumber),
    bf(Field::AlternateSeries),
    bf(Field::BlackAndWhite),
    bf(Field::Characters),
    bf(Field::Colorist),
    bf(Field::Count),
    bf(Field::CoverArtist),
    bf(Field::Editor),
    bf(Field::Format),
    bf(Field::Genre),
    bf(Field::Imprint),
    bf(Field::Inker),
    bf(Field::LanguageISO),
    bf(Field::Letterer),
    bf(Field::Locations),
    bf(Field::MainCharacterOrTeam),
    bf(Field::Manga),
    bf(Field::Month),
    bf(Field::Notes),
    bf(Field::Number),
    bf(Field::Penciller),
    bf(Field::Publisher),
    bf(Field::Rating),
    bf(Field::ReadPercentage),
    bf(Field::Review),
    bf(Field::ScanInformation),
    bf(Field::Series),
    bf(Field::SeriesComplete),
    bf(Field::SeriesGroup),
    OrgField::StartMonth,
    OrgField::StartYear,
    bf(Field::StoryArc),
    bf(Field::Tags),
    bf(Field::Teams),
    bf(Field::Title),
    bf(Field::Volume),
    bf(Field::Web),
    bf(Field::Writer),
    bf(Field::Year),
];

/// Fields offered for empty-value substitution.
pub const EMPTY_DATA_FIELDS: &[OrgField] = &[
    bf(Field::AgeRating),
    bf(Field::AlternateCount),
    bf(Field::AlternateNumber),
    bf(Field::AlternateSeries),
    bf(Field::BlackAndWhite),
    bf(Field::Characters),
    bf(Field::Colorist),
    bf(Field::Count),
    bf(Field::CoverArtist),
    bf(Field::Editor),
    bf(Field::Format),
    OrgField::FirstLetter,
    bf(Field::Genre),
    bf(Field::Imprint),
    bf(Field::Inker),
    bf(Field::LanguageISO),
    bf(Field::Letterer),
    bf(Field::Locations),
    bf(Field::MainCharacterOrTeam),
    bf(Field::Manga),
    bf(Field::Month),
    bf(Field::Number),
    bf(Field::Penciller),
    bf(Field::Publisher),
    bf(Field::Rating),
    bf(Field::ReadPercentage),
    bf(Field::ScanInformation),
    bf(Field::Series),
    bf(Field::SeriesComplete),
    bf(Field::SeriesGroup),
    OrgField::StartMonth,
    OrgField::StartYear,
    bf(Field::StoryArc),
    bf(Field::Tags),
    bf(Field::Teams),
    bf(Field::Title),
    bf(Field::Volume),
    bf(Field::Writer),
    bf(Field::Year),
];

/// Values for Yes/No rule fields (Black and white, Series complete).
pub const YES_NO_VALUES: &[&str] = &["Yes", "No", "Unknown"];
/// Values for the Manga rule field.
pub const MANGA_VALUES: &[&str] = &["Yes", "Yes (Right to Left)", "No", "Unknown"];

/// Template field names (as written between `<` `>`) with an English label
/// (translated at use time with [`crate::i18n::tr`]), for the template editor.
pub const TEMPLATE_FIELDS: &[(&str, &str)] = &[
    ("series", "Series"),
    ("number", "Number"),
    ("count", "Count"),
    ("volume", "Volume"),
    ("title", "Title"),
    ("year", "Year"),
    ("month", "Month (name)"),
    ("month#", "Month (number)"),
    ("Day", "Day"),
    ("publisher", "Publisher"),
    ("imprint", "Imprint"),
    ("altSeries", "Alternate Series"),
    ("altNumber", "Alternate Number"),
    ("altCount", "Alternate Count"),
    ("format", "Format"),
    ("ageRating", "Age Rating"),
    ("language", "Language"),
    ("maincharacter", "Main Character/Team"),
    ("seriesgroup", "Series Group"),
    ("storyarc", "Story Arc"),
    ("writer", "Writer"),
    ("penciller", "Penciller"),
    ("inker", "Inker"),
    ("colorist", "Colorist"),
    ("letterer", "Letterer"),
    ("coverartist", "Cover Artist"),
    ("editor", "Editor"),
    ("genre", "Genre"),
    ("tags", "Tags"),
    ("characters", "Characters"),
    ("teams", "Teams"),
    ("locations", "Locations"),
    ("scaninfo", "Scan Information"),
    ("manga", "Manga"),
    ("seriesComplete", "Series complete"),
    ("read", "Read Percentage"),
    ("Rating", "My Rating"),
    ("CommunityRating", "Community Rating"),
    ("startyear", "Start Year"),
    ("startmonth", "Start Month (name)"),
    ("startmonth#", "Start Month (number)"),
    ("EndYear", "End Year"),
    ("EndMonth", "End Month (name)"),
    ("EndMonth#", "End Month (number)"),
    ("firstissuenumber", "First Issue Number"),
    ("lastissuenumber", "Last Issue Number"),
    ("first", "First Letter"),
    ("counter", "Counter"),
    ("AddedDate", "Added Date"),
    ("ReleasedDate", "Released Date"),
    ("Custom", "Custom Value"),
];

// ---------------------------------------------------------------------------
// Profiles & settings
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Move,
    Copy,
    Simulate,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Move => t!("Move"),
            Mode::Copy => t!("Copy"),
            Mode::Simulate => t!("Simulate"),
        }
    }

    /// Log action of a successful operation (English, translated by
    /// [`Organizer::log`]).
    fn past(self) -> &'static str {
        match self {
            Mode::Move => "Moved",
            Mode::Copy => "Copied",
            Mode::Simulate => "Moved (simulated)",
        }
    }

    /// Summary line of a profile.
    fn report(self, name: &str, success: usize, skipped: usize, failed: usize) -> String {
        match self {
            Mode::Move => t!(
                "{0}:\nSuccessfully moved: {1}\tSkipped: {2}\tFailed: {3}",
                name,
                success,
                skipped,
                failed
            ),
            Mode::Copy => t!(
                "{0}:\nSuccessfully copied: {1}\tSkipped: {2}\tFailed: {3}",
                name,
                success,
                skipped,
                failed
            ),
            Mode::Simulate => t!(
                "{0}:\nSuccessfully moved (simulated): {1}\tSkipped: {2}\tFailed: {3}",
                name,
                success,
                skipped,
                failed
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExcludeMode {
    /// "Do not move books that match..."
    #[default]
    DoNot,
    /// "Only move books that match..."
    Only,
}

impl ExcludeMode {
    pub const ALL: &'static [ExcludeMode] = &[ExcludeMode::DoNot, ExcludeMode::Only];

    pub fn label(self) -> &'static str {
        match self {
            ExcludeMode::DoNot => t!("Do not"),
            ExcludeMode::Only => t!("Only"),
        }
    }
}

/// How the rules of a group are combined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupOp {
    #[default]
    Any,
    All,
}

impl GroupOp {
    pub const ALL: &'static [GroupOp] = &[GroupOp::Any, GroupOp::All];

    pub fn label(self) -> &'static str {
        match self {
            GroupOp::Any => t!("any"),
            GroupOp::All => t!("all"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleOp {
    #[default]
    Contains,
    DoesNotContain,
    GreaterThan,
    LessThan,
    Is,
    IsNot,
}

impl RuleOp {
    pub const ALL: &'static [RuleOp] = &[
        RuleOp::Contains,
        RuleOp::DoesNotContain,
        RuleOp::GreaterThan,
        RuleOp::LessThan,
        RuleOp::Is,
        RuleOp::IsNot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RuleOp::Contains => t!("contains"),
            RuleOp::DoesNotContain => t!("does not contain"),
            RuleOp::GreaterThan => t!("greater than"),
            RuleOp::LessThan => t!("less than"),
            RuleOp::Is => t!("is"),
            RuleOp::IsNot => t!("is not"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleGroup {
    pub operator: GroupOp,
    pub items: Vec<RuleItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuleItem {
    Rule {
        field: OrgField,
        operator: RuleOp,
        value: String,
    },
    Group(RuleGroup),
}

pub const DEFAULT_FILE_TEMPLATE: &str =
    "{<series>}{ Vol.<volume>}{ #<number2>}{ (of <count2>)}{ ({<month>, }<year>)}";
pub const DEFAULT_FOLDER_TEMPLATE: &str =
    "{<publisher>}/{<imprint>}/{<series>}{ (<startyear>{ <format>})}";

/// The nine replacements that always apply (they cannot be removed).
pub const DEFAULT_ILLEGAL_CHARACTERS: &[(&str, &str)] = &[
    ("?", ""),
    ("/", ""),
    ("\\", ""),
    ("*", ""),
    (":", " - "),
    ("<", "["),
    (">", "]"),
    ("|", "!"),
    ("\"", "'"),
];

/// Characters that may not appear in an illegal-character replacement.
pub const FORBIDDEN_IN_REPLACEMENT: &[char] = &['\\', '/', '|', '*', '<', '>', '?', '"', ':'];

/// Default month names (1-12) and seasons (13-16), in the interface language.
pub fn default_months() -> BTreeMap<u32, String> {
    let (months, seasons) = if crate::i18n::is_french() {
        (MONTHS_FR, ["Printemps", "Été", "Automne", "Hiver"])
    } else {
        (MONTHS, ["Spring", "Summer", "Fall", "Winter"])
    };
    let mut m: BTreeMap<u32, String> = months
        .iter()
        .enumerate()
        .map(|(i, n)| (i as u32 + 1, n.to_string()))
        .collect();
    for (i, s) in seasons.iter().enumerate() {
        m.insert(13 + i as u32, s.to_string());
    }
    m
}

pub fn default_illegal_characters() -> BTreeMap<String, String> {
    DEFAULT_ILLEGAL_CHARACTERS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub base_folder: String,
    pub folder_template: String,
    pub file_template: String,
    pub mode: Mode,
    /// Copy mode: add the copied book to the library.
    pub copy_add_to_library: bool,
    pub use_folder: bool,
    pub use_file_name: bool,
    /// Replacement for empty folder names (empty = drop that level).
    pub empty_folder: String,
    pub empty_data: BTreeMap<OrgField, String>,
    pub illegal_characters: BTreeMap<String, String>,
    pub months: BTreeMap<u32, String>,
    pub exclude_folders: Vec<String>,
    pub exclude_rules: RuleGroup,
    pub exclude_mode: ExcludeMode,
    pub remove_empty_folders: bool,
    pub excluded_empty_folders: Vec<String>,
    pub fail_empty_values: bool,
    pub failed_fields: Vec<OrgField>,
    pub move_failed: bool,
    pub failed_folder: String,
    pub dont_ask_when_multi_one: bool,
    pub replace_multiple_spaces: bool,
    pub copy_read_percentage: bool,
    /// Template editor: insert a space at the start of inserted fields.
    pub auto_space_fields: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            name: if crate::i18n::is_french() {
                "Défaut"
            } else {
                "Default"
            }
            .into(),
            base_folder: String::new(),
            folder_template: DEFAULT_FOLDER_TEMPLATE.into(),
            file_template: DEFAULT_FILE_TEMPLATE.into(),
            mode: Mode::Move,
            copy_add_to_library: true,
            use_folder: true,
            use_file_name: true,
            empty_folder: String::new(),
            empty_data: BTreeMap::new(),
            illegal_characters: default_illegal_characters(),
            months: default_months(),
            exclude_folders: Vec::new(),
            exclude_rules: RuleGroup::default(),
            exclude_mode: ExcludeMode::DoNot,
            remove_empty_folders: true,
            excluded_empty_folders: Vec::new(),
            fail_empty_values: false,
            failed_fields: Vec::new(),
            move_failed: false,
            failed_folder: String::new(),
            dont_ask_when_multi_one: true,
            replace_multiple_spaces: true,
            copy_read_percentage: true,
            auto_space_fields: true,
        }
    }
}

impl Profile {
    /// The plugin's settings checks; returns the error messages (empty = ok).
    pub fn validate(&self) -> Vec<String> {
        let mut e = Vec::new();
        if self.use_folder && self.base_folder.trim().is_empty() {
            e.push(
                t!("The base folder cannot be empty when folder organization is enabled.")
                    .into(),
            );
        }
        if self.use_file_name && self.file_template.trim().is_empty() {
            e.push(
                t!("The file template cannot be empty when file organization is enabled.")
                    .into(),
            );
        }
        if !self.use_folder && !self.use_file_name {
            e.push(
                t!("Enable folder or file organization so that the script does something.")
                    .into(),
            );
        }
        if self.move_failed && self.failed_folder.trim().is_empty() {
            e.push(
                t!("The failed folder cannot be empty when moving failed books is enabled.")
                    .into(),
            );
        }
        e
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OrganizerSettings {
    pub profiles: Vec<Profile>,
    pub last_used: Vec<String>,
}

impl Default for OrganizerSettings {
    fn default() -> Self {
        let p = Profile::default();
        OrganizerSettings {
            last_used: vec![p.name.clone()],
            profiles: vec![p],
        }
    }
}

impl OrganizerSettings {
    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

// ---------------------------------------------------------------------------
// Interaction
// ---------------------------------------------------------------------------

/// Answer of the multiple-value selection dialog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MultiChoice {
    /// Selected values, in the order they must be inserted.
    pub selected: Vec<String>,
    /// Use each value as its own folder.
    pub each_as_folder: bool,
    /// Issue mode: reuse this selection for every issue of this run having these values.
    pub always_use: bool,
    /// With `always_use`: do not ask again even if the issue has more values.
    pub dont_ask: bool,
    /// Series mode: insert every selected value even if the issue does not have it.
    pub every_issue: bool,
}

/// Full context of a multiple-value question (see [`Interaction::choose_multi_values_ex`]).
pub struct MultiValueRequest<'b> {
    pub book: &'b Book,
    pub field: Field,
    /// Values to choose from (issue values, or every value of the series).
    pub values: Vec<String>,
    /// Values to pre-select (from an "always use" selection).
    pub preselected: Vec<String>,
    /// True when the selection applies to the whole series (`(series)` mode).
    pub whole_series: bool,
    /// "Series vol. V #N" (issue mode) or "Series vol. V" (series mode).
    pub book_text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DuplicateAction {
    Overwrite,
    Skip,
    #[default]
    Rename,
}

/// UI callbacks; called from the worker thread and may block on the UI.
pub trait Interaction {
    /// `None` = cancelled (nothing inserted).
    fn choose_multi_values(
        &self,
        book: &Book,
        field: Field,
        values: &[String],
    ) -> Option<MultiChoice>;
    /// A file already exists at `dest`. Returns the action and "apply to all".
    fn on_duplicate(
        &self,
        book: &Book,
        dest: &Path,
        existing_size: Option<u64>,
    ) -> (DuplicateAction, bool);
    /// Richer variant of [`Interaction::choose_multi_values`] (pre-selection, series mode).
    fn choose_multi_values_ex(&self, req: &MultiValueRequest) -> Option<MultiChoice> {
        self.choose_multi_values(req.book, req.field, &req.values)
    }
    /// Polled before each operation.
    fn is_cancelled(&self) -> bool {
        false
    }
    /// Called after each operation.
    fn progress(&self, _done: usize, _total: usize) {}
}

/// Non-interactive answers: every value, and rename on conflicts.
pub struct NoInteraction;

impl Interaction for NoInteraction {
    fn choose_multi_values(
        &self,
        _book: &Book,
        _field: Field,
        values: &[String],
    ) -> Option<MultiChoice> {
        Some(MultiChoice {
            selected: values.to_vec(),
            ..Default::default()
        })
    }

    fn on_duplicate(
        &self,
        _book: &Book,
        _dest: &Path,
        _existing_size: Option<u64>,
    ) -> (DuplicateAction, bool) {
        (DuplicateAction::Rename, false)
    }
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub profile: String,
    pub action: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct RunResult {
    /// Books whose file moved (book id, new path).
    pub moved: Vec<(u64, PathBuf)>,
    /// Copies to add to the library (id 0).
    pub added: Vec<Book>,
    /// Library books whose file was overwritten (to remove from the library).
    pub removed: Vec<u64>,
    /// "Copy read percentage" on overwrite: (book id, last page read) to apply.
    pub read_progress: Vec<(u64, i32)>,
    pub log: Vec<LogEntry>,
    pub summary: String,
    /// Something failed or was skipped (the plugin then offers the detailed report).
    pub failed_or_skipped: bool,
    /// A simulate profile ran (the plugin then always offers the report).
    pub simulated: bool,
}

pub fn report_text(result: &RunResult) -> String {
    let mut s = t!("Library Organizer Report:\n\n{0}", result.summary);
    for e in &result.log {
        s.push_str("\n\n");
        s.push_str(&t!("{0}:\n{1}: {2}", e.profile, e.action, e.path));
        if !e.message.is_empty() {
            s.push('\n');
            s.push_str(&t!("Message: {0}", e.message));
        }
    }
    s
}

/// Log actions (English; translated by [`Organizer::log`]).
mod act {
    pub const FAILED: &str = "Failed";
    pub const SKIPPED: &str = "Skipped";
    pub const CANCELLED: &str = "Cancelled";
    pub const FOLDER_CREATED: &str = "Created Folder";
    pub const FOLDER_FAILED: &str = "Failed to create folder";
    pub const DELETED: &str = "Deleted file";
    pub const DELETED_SIM: &str = "Deleted (simulated)";
    pub const RENAMING_SIM: &str = "Renaming (simulated)";
    pub const RENAMED: &str = "Renamed";
    pub const RESTORED: &str = "Restored";
}

/// Log messages (English; translated with [`crate::i18n::tr`] where used).
mod msg {
    pub const FILE_MISSING: &str = "The file does not exist";
    pub const NO_FILE: &str = "The book has no file";
    pub const EXCLUDED_PATH: &str = "The book is located in an excluded path";
    pub const EXCLUDED_RULES: &str = "The book qualified under the exclude rules";
    pub const LATER_PROFILE: &str = "The book is moved by a later profile";
    pub const ALREADY_THERE: &str = "The book is already located at the calculated path";
    pub const EMPTY_NAME: &str = "The calculated file name is empty";
    pub const BASE_EMPTY: &str = "The base folder is empty";
    pub const CANCELLED: &str = "The user cancelled the operation";
}


// ---------------------------------------------------------------------------
// Regexes and small helpers
// ---------------------------------------------------------------------------

static TEMPLATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\{(?P<prefix>[^{}<]*)<(?P<name>[^\d\s(>]*)(?P<args>\d*|(?:\([^){}]*\))*)>(?P<postfix>[^{}]*)\}",
    )
    .unwrap()
});
static ARGS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(([^)]*)\)").unwrap());
static FIRST_LETTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:(?:the|a|an|de|het|een|die|der|das|des|dem|der|ein|eines|einer|einen|la|le|l'|les|un|une|el|las|los|las|un|una|unos|unas|o|os|um|uma|uns|umas|en|et|il|lo|uno|gli)\s+)?(?P<letter>.).+",
    )
    .unwrap()
});
static RENAME_SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" \([0-9]\)$").unwrap());
static SPACES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s\s+").unwrap());

/// Internal folder separator produced by "each value as a folder".
const SEP: char = '/';

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Applies the illegal-character table (longest keys first). The nine
/// default characters always apply, even if missing from the profile.
pub fn replace_illegal_characters(profile: &Profile, text: &str) -> String {
    let mut pairs: Vec<(&str, &str)> = profile
        .illegal_characters
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    for (k, v) in DEFAULT_ILLEGAL_CHARACTERS {
        if !profile.illegal_characters.contains_key(*k) {
            pairs.push((k, v));
        }
    }
    pairs.sort_by_key(|(k, _)| Reverse(k.chars().count()));
    let mut s = text.to_string();
    for (k, v) in pairs {
        if !k.is_empty() {
            s = s.replace(k, v);
        }
    }
    s
}

/// Zero-pads the integer part of a number ("7.1", 3 -> "007.1"; "-1", 2 -> "-01").
/// Non-numeric values are returned unchanged.
pub fn pad(value: &str, padding: usize) -> String {
    let Ok(n) = value.trim().parse::<f64>() else {
        return value.to_string();
    };
    let (neg, body): (bool, String) = if n >= 0.0 {
        (false, value.to_string())
    } else {
        (true, value.chars().skip(1).collect())
    };
    let (int, rem) = match body.split_once('.') {
        Some((a, b)) => (a.to_string(), format!(".{b}")),
        None => (body, String::new()),
    };
    format!(
        "{}{:0>w$}{}",
        if neg { "-" } else { "" },
        int,
        rem,
        w = padding
    )
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn too_long(p: &Path) -> bool {
    cfg!(windows) && path_str(p).chars().count() > 259
}

fn too_long_msg(p: &Path) -> String {
    t!(
        "The calculated path is too long ({0} characters, maximum 259): {1}",
        path_str(p).chars().count(),
        path_str(p)
    )
}

/// Extension with the dot (".cbz"), empty for folders and extension-less files.
fn extension_of(p: &Path) -> String {
    if p.is_dir() {
        return String::new();
    }
    p.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

fn same_series(a: &Book, b: &Book) -> bool {
    a.info.series == b.info.series
        && a.info.volume == b.info.volume
        && a.info.publisher == b.info.publisher
}

type SeriesKey = (String, String, i32);

fn series_key(b: &Book) -> SeriesKey {
    (
        b.info.publisher.clone(),
        b.info.series.clone(),
        b.info.volume,
    )
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.components().eq(b.components())
}

fn copy_path(src: &Path, dest: &Path) -> io::Result<()> {
    if src.is_dir() {
        for e in walkdir::WalkDir::new(src) {
            let e = e.map_err(io::Error::other)?;
            let rel = e.path().strip_prefix(src).map_err(io::Error::other)?;
            let target = dest.join(rel);
            if e.file_type().is_dir() {
                fs::create_dir_all(&target)?;
            } else {
                fs::copy(e.path(), &target)?;
            }
        }
        Ok(())
    } else {
        fs::copy(src, dest).map(|_| ())
    }
}

fn remove_path(p: &Path) -> io::Result<()> {
    if p.is_dir() {
        fs::remove_dir_all(p)
    } else {
        fs::remove_file(p)
    }
}

fn move_path(src: &Path, dest: &Path) -> io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18) => {
            copy_path(src, dest)?;
            remove_path(src)
        }
        Err(e) => Err(e),
    }
}

/// Name of an ISO 639-1 language code in the interface language (the code
/// itself if unknown).
pub fn language_name(iso: &str) -> String {
    let code = iso.trim().to_lowercase();
    let code = code.split(['-', '_']).next().unwrap_or("");
    let (en, fr) = match code {
        "" => return String::new(),
        "fr" => ("French", "Français"),
        "en" => ("English", "Anglais"),
        "de" => ("German", "Allemand"),
        "es" => ("Spanish", "Espagnol"),
        "it" => ("Italian", "Italien"),
        "pt" => ("Portuguese", "Portugais"),
        "nl" => ("Dutch", "Néerlandais"),
        "ja" => ("Japanese", "Japonais"),
        "zh" => ("Chinese", "Chinois"),
        "ko" => ("Korean", "Coréen"),
        "ru" => ("Russian", "Russe"),
        "pl" => ("Polish", "Polonais"),
        "sv" => ("Swedish", "Suédois"),
        "da" => ("Danish", "Danois"),
        "no" | "nb" | "nn" => ("Norwegian", "Norvégien"),
        "fi" => ("Finnish", "Finnois"),
        "is" => ("Icelandic", "Islandais"),
        "el" => ("Greek", "Grec"),
        "tr" => ("Turkish", "Turc"),
        "ar" => ("Arabic", "Arabe"),
        "he" => ("Hebrew", "Hébreu"),
        "fa" => ("Persian", "Persan"),
        "hi" => ("Hindi", "Hindi"),
        "hu" => ("Hungarian", "Hongrois"),
        "cs" => ("Czech", "Tchèque"),
        "sk" => ("Slovak", "Slovaque"),
        "sl" => ("Slovenian", "Slovène"),
        "hr" => ("Croatian", "Croate"),
        "sr" => ("Serbian", "Serbe"),
        "bg" => ("Bulgarian", "Bulgare"),
        "ro" => ("Romanian", "Roumain"),
        "uk" => ("Ukrainian", "Ukrainien"),
        "ca" => ("Catalan", "Catalan"),
        "eu" => ("Basque", "Basque"),
        "gl" => ("Galician", "Galicien"),
        "br" => ("Breton", "Breton"),
        "co" => ("Corsican", "Corse"),
        "oc" => ("Occitan", "Occitan"),
        "la" => ("Latin", "Latin"),
        "eo" => ("Esperanto", "Espéranto"),
        "id" => ("Indonesian", "Indonésien"),
        "ms" => ("Malay", "Malais"),
        "th" => ("Thai", "Thaï"),
        "vi" => ("Vietnamese", "Vietnamien"),
        "tl" => ("Tagalog", "Tagalog"),
        _ => return iso.trim().to_string(),
    };
    if crate::i18n::is_french() { fr } else { en }.to_string()
}

// ---------------------------------------------------------------------------
// .NET date formatting (fr-FR or en-US culture, following the interface)
// ---------------------------------------------------------------------------

const MONTHS_ABBR_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];
const DAYS_FR: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const DAYS_ABBR_FR: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
const MONTHS_ABBR_EN: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAYS_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const DAYS_ABBR_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Formats a date with a .NET standard ("d", "D", "M", "Y", "G"...) or
/// custom ("dd/MM/yy", "MMM d, yyy", "MMMM yyyy"...) format string, using
/// the culture of the interface (fr-FR or en-US). An empty format is .NET's
/// default `ToString()` ("G").
pub fn format_dotnet_date(dt: NaiveDateTime, format: &str) -> String {
    format_dotnet_date_in(dt, format, crate::i18n::is_french())
}

fn format_dotnet_date_in(dt: NaiveDateTime, format: &str, french: bool) -> String {
    let pattern = if french { fr_pattern(format) } else { en_pattern(format) };
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' => {
                i += 1;
                while i < chars.len() && chars[i] != c {
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                if let Some(n) = chars.get(i + 1) {
                    out.push(*n);
                }
                i += 2;
            }
            '%' => i += 1,
            'd' | 'M' | 'y' | 'h' | 'H' | 'm' | 's' | 'f' | 'F' | 't' | 'z' | 'g' | 'K' => {
                let mut n = 1;
                while chars.get(i + n) == Some(&c) {
                    n += 1;
                }
                out.push_str(&date_token(dt, c, n, french));
                i += n;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// .NET standard format strings of the en-US culture.
fn en_pattern(format: &str) -> &str {
    match format {
        "" | "G" => "M/d/yyyy h:mm:ss tt",
        "d" => "M/d/yyyy",
        "D" => "dddd, MMMM d, yyyy",
        "f" => "dddd, MMMM d, yyyy h:mm tt",
        "F" => "dddd, MMMM d, yyyy h:mm:ss tt",
        "g" => "M/d/yyyy h:mm tt",
        "m" | "M" => "MMMM d",
        "y" | "Y" => "MMMM yyyy",
        "t" => "h:mm tt",
        "T" => "h:mm:ss tt",
        "s" => "yyyy'-'MM'-'dd'T'HH':'mm':'ss",
        "u" => "yyyy'-'MM'-'dd HH':'mm':'ss'Z'",
        other => other,
    }
}

/// .NET standard format strings of the fr-FR culture.
fn fr_pattern(format: &str) -> &str {
    match format {
        "" | "G" => "dd/MM/yyyy HH:mm:ss",
        "d" => "dd/MM/yyyy",
        "D" => "dddd d MMMM yyyy",
        "f" => "dddd d MMMM yyyy HH:mm",
        "F" => "dddd d MMMM yyyy HH:mm:ss",
        "g" => "dd/MM/yyyy HH:mm",
        "m" | "M" => "d MMMM",
        "y" | "Y" => "MMMM yyyy",
        "t" => "HH:mm",
        "T" => "HH:mm:ss",
        "s" => "yyyy'-'MM'-'dd'T'HH':'mm':'ss",
        "u" => "yyyy'-'MM'-'dd HH':'mm':'ss'Z'",
        other => other,
    }
}

fn date_token(dt: NaiveDateTime, c: char, n: usize, french: bool) -> String {
    let two = |v: u32| {
        if n >= 2 {
            format!("{v:02}")
        } else {
            v.to_string()
        }
    };
    match c {
        'd' => match n {
            1 | 2 => two(dt.day()),
            3 => (if french { DAYS_ABBR_FR } else { DAYS_ABBR_EN })[dt.weekday().num_days_from_monday() as usize].into(),
            _ => (if french { DAYS_FR } else { DAYS_EN })[dt.weekday().num_days_from_monday() as usize].into(),
        },
        'M' => match n {
            1 | 2 => two(dt.month()),
            3 => (if french { MONTHS_ABBR_FR } else { MONTHS_ABBR_EN })[dt.month0() as usize].into(),
            _ => (if french { MONTHS_FR } else { MONTHS })[dt.month0() as usize].into(),
        },
        'y' => match n {
            1 => (dt.year().rem_euclid(100)).to_string(),
            2 => format!("{:02}", dt.year().rem_euclid(100)),
            _ => format!("{:0w$}", dt.year(), w = n),
        },
        'h' => two(match dt.hour() % 12 {
            0 => 12,
            h => h,
        }),
        'H' => two(dt.hour()),
        'm' => two(dt.minute()),
        's' => two(dt.second()),
        'f' | 'F' => {
            let n = n.min(9);
            let frac = format!("{:09}", dt.nanosecond() % 1_000_000_000);
            let s = &frac[..n];
            if c == 'F' {
                s.trim_end_matches('0').to_string()
            } else {
                s.to_string()
            }
        }
        // AM/PM designators (empty in fr-FR); time zones and eras are not supported.
        't' if !french => {
            let d = if dt.hour() < 12 { "AM" } else { "PM" };
            if n == 1 { d[..1].to_string() } else { d.to_string() }
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Template sources
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    Std(Field),
    Month,
    Language,
    ReadPct,
    StartYear,
    StartMonth,
    EndYear,
    EndMonth,
    FirstLetter,
    Counter,
    FirstIssue,
    LastIssue,
    Custom,
    Added,
    Released,
}

fn template_source(name: &str) -> Option<Src> {
    use Src::*;
    Some(match name {
        "series" => Std(Field::Series),
        "number" => Std(Field::Number),
        "count" => Std(Field::Count),
        "volume" => Std(Field::Volume),
        "title" => Std(Field::Title),
        "year" => Std(Field::Year),
        "Day" => Std(Field::Day),
        "month" | "month#" => Month,
        "imprint" => Std(Field::Imprint),
        "publisher" => Std(Field::Publisher),
        "altSeries" => Std(Field::AlternateSeries),
        "altNumber" => Std(Field::AlternateNumber),
        "altCount" => Std(Field::AlternateCount),
        "ageRating" => Std(Field::AgeRating),
        "language" => Language,
        "format" => Std(Field::Format),
        "writer" => Std(Field::Writer),
        "penciller" => Std(Field::Penciller),
        "inker" => Std(Field::Inker),
        "colorist" => Std(Field::Colorist),
        "letterer" => Std(Field::Letterer),
        "coverartist" => Std(Field::CoverArtist),
        "editor" => Std(Field::Editor),
        "tags" => Std(Field::Tags),
        "genre" => Std(Field::Genre),
        "characters" => Std(Field::Characters),
        "teams" => Std(Field::Teams),
        "locations" => Std(Field::Locations),
        "scaninfo" => Std(Field::ScanInformation),
        "storyarc" => Std(Field::StoryArc),
        "seriesgroup" => Std(Field::SeriesGroup),
        "maincharacter" => Std(Field::MainCharacterOrTeam),
        "manga" => Std(Field::Manga),
        "seriesComplete" => Std(Field::SeriesComplete),
        "Rating" => Std(Field::Rating),
        "CommunityRating" => Std(Field::CommunityRating),
        "read" => ReadPct,
        "startyear" => StartYear,
        "startmonth" | "startmonth#" => StartMonth,
        "EndYear" => EndYear,
        "EndMonth" | "EndMonth#" => EndMonth,
        "first" => FirstLetter,
        "counter" => Counter,
        "firstissuenumber" => FirstIssue,
        "lastissuenumber" => LastIssue,
        "Custom" => Custom,
        "AddedDate" => Added,
        "ReleasedDate" => Released,
        _ => return None,
    })
}

impl Src {
    /// Key used for empty-data substitution and "fail on empty".
    fn key(self) -> Option<OrgField> {
        Some(match self {
            Src::Std(f) => OrgField::Book(f),
            Src::Month => OrgField::Book(Field::Month),
            Src::Language => OrgField::Book(Field::LanguageISO),
            Src::ReadPct => OrgField::Book(Field::ReadPercentage),
            Src::StartYear => OrgField::StartYear,
            Src::StartMonth => OrgField::StartMonth,
            Src::EndYear => OrgField::EndYear,
            Src::EndMonth => OrgField::EndMonth,
            Src::FirstLetter => OrgField::FirstLetter,
            Src::FirstIssue => OrgField::FirstIssueNumber,
            Src::LastIssue => OrgField::LastIssueNumber,
            Src::Added => OrgField::Book(Field::Added),
            Src::Released => OrgField::Book(Field::Released),
            Src::Counter | Src::Custom => return None,
        })
    }
}

/// A property value as the plugin sees it (ints use -1 for "unset").
enum Raw {
    Int(i64),
    Float(f32),
    Str(String),
}

impl Raw {
    /// `unicode(value)` in the plugin.
    fn py(&self) -> String {
        match self {
            Raw::Int(i) => i.to_string(),
            Raw::Float(f) => format!("{f:?}"),
            Raw::Str(s) => s.clone(),
        }
    }
    /// `not value or value == -1`
    fn empty_text(&self) -> bool {
        match self {
            Raw::Int(i) => *i == -1 || *i == 0,
            Raw::Float(f) => *f == 0.0,
            Raw::Str(s) => s.is_empty(),
        }
    }
    /// `value == -1 or value == ""`
    fn unset(&self) -> bool {
        match self {
            Raw::Int(i) => *i == -1,
            Raw::Float(_) => false,
            Raw::Str(s) => s.is_empty(),
        }
    }
}

fn raw_field(book: &Book, f: Field) -> Raw {
    let i = &book.info;
    match f {
        Field::Count => Raw::Int(i.count as i64),
        Field::Volume => Raw::Int(i.volume as i64),
        Field::AlternateCount => Raw::Int(i.alternate_count as i64),
        Field::Year => Raw::Int(i.year as i64),
        Field::Month => Raw::Int(i.month as i64),
        Field::Day => Raw::Int(i.day as i64),
        Field::PageCount => Raw::Int(i.page_count as i64),
        Field::ReadPercentage => Raw::Int(book.read_percentage() as i64),
        Field::Rating => Raw::Float(book.rating),
        Field::CommunityRating => Raw::Float(i.community_rating),
        // ComicRack's FileName has no extension.
        Field::FileName => Raw::Str(book.file_stem()),
        _ => Raw::Str(f.get(book)),
    }
}

fn raw_src(book: &Book, src: Src) -> Option<Raw> {
    Some(match src {
        Src::Std(f) => raw_field(book, f),
        Src::Month => Raw::Int(book.info.month as i64),
        Src::Language => Raw::Str(language_name(&book.info.language_iso)),
        Src::ReadPct => Raw::Int(book.read_percentage() as i64),
        _ => return None,
    })
}

/// Resolves the argument of `<first(...)>`: plugin display names ("Series",
/// "Alternate Series"), ComicRack property names ("ShadowSeries",
/// "AlternateSeries") or interface labels ("Series", "Série").
fn resolve_field_name(name: &str) -> Option<Field> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let compact = compact.strip_prefix("Shadow").unwrap_or(&compact);
    match compact.to_lowercase().as_str() {
        "language" | "languageastext" => return Some(Field::LanguageISO),
        "addeddate" => return Some(Field::Added),
        "releaseddate" => return Some(Field::Released),
        _ => {}
    }
    Field::from_key(compact).or_else(|| {
        Field::ALL
            .iter()
            .copied()
            .find(|f| f.label().eq_ignore_ascii_case(name.trim()))
    })
}

/// Splits the last `(...)` group off the args ("(a)(b)" -> ("(a)", "b")).
fn split_last_paren(args: &str) -> Option<(String, String)> {
    if !args.ends_with(')') {
        return None;
    }
    let open = args.rfind('(')?;
    Some((
        args[..open].to_string(),
        args[open + 1..args.len() - 1].to_string(),
    ))
}

fn anchored_match(pattern: &str, text: &str) -> Option<bool> {
    Regex::new(&format!("^(?:{pattern})"))
        .ok()
        .map(|r| r.is_match(text))
}

struct AlwaysUsed {
    values: Vec<String>,
    dont_ask: bool,
    folder: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    Success,
    Failed,
    Skipped,
    Duplicate,
}

enum Resolved {
    Proceed,
    Skipped,
    Failed,
    SimulatedOverwrite,
}

struct Op {
    idx: usize,
    dest: PathBuf,
    profile: usize,
    failed_fields: Vec<OrgField>,
}

struct ProfileReport {
    success: usize,
    failed: usize,
    name: String,
    mode: Mode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct UndoEntry {
    profile: String,
    current: PathBuf,
    original: PathBuf,
}

// ---------------------------------------------------------------------------
// Organizer
// ---------------------------------------------------------------------------

/// The path maker and book mover. One instance = one run (computed series
/// values, counters and multiple-value selections are cached for the run).
pub struct Organizer<'a> {
    library: &'a [Book],
    interaction: &'a dyn Interaction,
    earliest: HashMap<SeriesKey, Book>,
    last: HashMap<SeriesKey, Book>,
    counter: Option<i64>,
    multi_issue: HashMap<(Field, String), MultiChoice>,
    multi_series: HashMap<(Field, SeriesKey), MultiChoice>,
    always_used: HashMap<Field, Vec<AlwaysUsed>>,
    failed: bool,
    failed_fields: Vec<OrgField>,
    // run state
    result: RunResult,
    profile_name: String,
    moved_books: HashSet<PathBuf>,
    created_paths: HashSet<PathBuf>,
    undo: Vec<UndoEntry>,
    current_paths: HashMap<usize, PathBuf>,
    moved_ids: HashMap<u64, PathBuf>,
    dup_always: bool,
    dup_action: DuplicateAction,
    pending_last_page: Option<i32>,
}

impl<'a> Organizer<'a> {
    pub fn new(library: &'a [Book], interaction: &'a dyn Interaction) -> Self {
        Organizer {
            library,
            interaction,
            earliest: HashMap::new(),
            last: HashMap::new(),
            counter: None,
            multi_issue: HashMap::new(),
            multi_series: HashMap::new(),
            always_used: HashMap::new(),
            failed: false,
            failed_fields: Vec::new(),
            result: RunResult::default(),
            profile_name: String::new(),
            moved_books: HashSet::new(),
            created_paths: HashSet::new(),
            undo: Vec::new(),
            current_paths: HashMap::new(),
            moved_ids: HashMap::new(),
            dup_always: false,
            dup_action: DuplicateAction::Rename,
            pending_last_page: None,
        }
    }

    // ----- computed series values -------------------------------------------

    /// The first published issue of the book's series in the library.
    fn earliest(&mut self, book: &Book) -> &Book {
        let key = series_key(book);
        if !self.earliest.contains_key(&key) {
            let mut start: &Book = book;
            for b in self.library.iter().filter(|b| same_series(b, book)) {
                let (s, bi) = (&start.info, &b.info);
                if s.year == -1 && bi.year != -1 {
                    start = b;
                }
                if bi.year != -1 && bi.year < start.info.year {
                    start = b;
                }
                let s = &start.info;
                if bi.year == s.year
                    && bi.month != -1
                    && (s.month == -1
                        || bi.month < s.month
                        || (bi.month == s.month && number_cmp(&bi.number, &s.number).is_lt()))
                {
                    start = b;
                }
            }
            let start = start.clone();
            self.earliest.insert(key.clone(), start);
        }
        &self.earliest[&key]
    }

    /// The issue with the highest (integer) number of the series in the library.
    fn last(&mut self, book: &Book) -> &Book {
        let key = series_key(book);
        if !self.last.contains_key(&key) {
            let mut end: &Book = book;
            for b in self.library.iter().filter(|b| same_series(b, book)) {
                if !is_digits(&end.info.number) {
                    end = b;
                    continue;
                }
                if is_digits(&b.info.number) && number_cmp(&end.info.number, &b.info.number).is_lt()
                {
                    end = b;
                }
            }
            let end = end.clone();
            self.last.insert(key.clone(), end);
        }
        &self.last[&key]
    }

    // ----- template engine --------------------------------------------------

    /// Replaces every `{...<field>...}` block of `template` (inside out).
    /// Invalid blocks are left untouched.
    pub fn expand(&mut self, book: &Book, profile: &Profile, template: &str) -> String {
        let mut t = template.to_string();
        for _ in 0..100 {
            let matches = TEMPLATE_RE.find_iter(&t).count();
            if matches == 0 {
                break;
            }
            let mut invalid = 0;
            let new = TEMPLATE_RE
                .replace_all(&t, |c: &Captures| {
                    self.insert_field(c, book, profile, &mut invalid)
                })
                .into_owned();
            let done = invalid == matches || new == t;
            t = new;
            if done {
                break;
            }
        }
        t
    }

    fn insert_field(
        &mut self,
        caps: &Captures,
        book: &Book,
        p: &Profile,
        invalid: &mut usize,
    ) -> String {
        let whole = caps[0].to_string();
        let prefix = &caps["prefix"];
        let postfix = &caps["postfix"];
        let mut name = &caps["name"];
        let mut args = caps["args"].to_string();
        let mut inversion: Option<String> = None;
        let mut conditional: Option<String> = None;

        if name.starts_with('!') {
            name = name.trim_start_matches('!');
            let mut inv = String::new();
            if let Some((rest, last)) = split_last_paren(&args) {
                args = rest;
                inv = last;
            }
            inversion = Some(inv);
        }
        if name.starts_with('?') {
            name = name.trim_start_matches('?');
            match split_last_paren(&args) {
                Some((rest, last)) => {
                    args = rest;
                    conditional = Some(last);
                }
                None => {
                    *invalid += 1;
                    return whole;
                }
            }
        }
        let Some(src) = template_source(name) else {
            *invalid += 1;
            return whole;
        };
        let Some(result) = self.field_text(book, p, src, name, &args) else {
            *invalid += 1;
            return whole;
        };
        let both = || format!("{prefix}{postfix}");

        if let Some(cond) = conditional {
            let hit = match cond.strip_prefix('!') {
                Some("") => false,
                Some(re) => match anchored_match(re, &result) {
                    Some(m) => m,
                    None => {
                        *invalid += 1;
                        return whole;
                    }
                },
                None => result == cond,
            };
            return if hit { both() } else { String::new() };
        }
        if let Some(inv) = inversion {
            let hit = if inv.is_empty() {
                result.is_empty()
            } else if let Some(re) = inv.strip_prefix('!') {
                match anchored_match(re, &result) {
                    Some(m) => !m,
                    None => {
                        *invalid += 1;
                        return whole;
                    }
                }
            } else {
                result != inv
            };
            return if hit { both() } else { String::new() };
        }
        if result.is_empty() {
            let key = src.key();
            if let Some(k) = key {
                if p.fail_empty_values && p.failed_fields.contains(&k) {
                    if !self.failed_fields.contains(&k) {
                        self.failed_fields.push(k);
                    }
                    self.failed = true;
                }
                if let Some(v) = p.empty_data.get(&k).filter(|v| !v.is_empty()) {
                    return v.clone();
                }
            }
            return String::new();
        }
        format!("{prefix}{result}{postfix}")
    }

    /// The text of one field, or `None` when the field/arguments are invalid.
    fn field_text(
        &mut self,
        book: &Book,
        p: &Profile,
        src: Src,
        name: &str,
        args_match: &str,
    ) -> Option<String> {
        let args: Vec<String> = ARGS_RE
            .captures_iter(args_match)
            .map(|c| c[1].to_string())
            .collect();
        let sanitize = |s: &str| replace_illegal_characters(p, s);
        match src {
            Src::StartYear | Src::EndYear => {
                let year = if src == Src::EndYear {
                    self.last(book).info.year
                } else {
                    self.earliest(book).info.year
                };
                return Some(if year == -1 {
                    String::new()
                } else {
                    sanitize(&year.to_string())
                });
            }
            Src::StartMonth | Src::EndMonth => {
                let month = if src == Src::EndMonth {
                    self.last(book).info.month
                } else {
                    self.earliest(book).info.month
                };
                if month == -1 {
                    return Some(String::new());
                }
                if name.ends_with('#') {
                    let mut m = month.to_string();
                    if is_digits(args_match) {
                        m = pad(&m, args_match.parse().ok()?);
                    }
                    return Some(sanitize(&m));
                }
                return Some(self.month_name(p, month));
            }
            Src::ReadPct if args.len() == 3 => {
                let percent: i32 = args[2].trim().parse().ok()?;
                let rp = book.read_percentage();
                let hit = match args[1].as_str() {
                    "=" => rp == percent,
                    ">" => rp > percent,
                    "<" => rp < percent,
                    _ => false,
                };
                return Some(if hit {
                    sanitize(&args[0])
                } else {
                    String::new()
                });
            }
            Src::FirstLetter => {
                let field = resolve_field_name(args.first()?)?;
                let value = raw_field(book, field).py();
                let letter = FIRST_LETTER_RE
                    .captures(&value)
                    .map(|c| c["letter"].to_uppercase())
                    .unwrap_or_default();
                return Some(sanitize(&letter));
            }
            Src::Counter => {
                if args.len() != 3 {
                    return None;
                }
                let start: i64 = args[0].trim().parse().ok()?;
                let inc: i64 = args[1].trim().parse().ok()?;
                let width: usize = if args[2].is_empty() {
                    0
                } else {
                    args[2].trim().parse().ok()?
                };
                let n = match self.counter {
                    None => start,
                    Some(c) => c + inc,
                };
                self.counter = Some(n);
                return Some(pad(&n.to_string(), width));
            }
            Src::FirstIssue | Src::LastIssue => {
                let number = if src == Src::LastIssue {
                    self.last(book).info.number.clone()
                } else {
                    self.earliest(book).info.number.clone()
                };
                let number = if is_digits(args_match) {
                    pad(&number, args_match.parse().ok()?)
                } else {
                    number
                };
                return Some(sanitize(&number));
            }
            Src::Custom => {
                let key = args.first()?;
                return Some(sanitize(
                    book.custom_values
                        .get(key)
                        .map(String::as_str)
                        .unwrap_or(""),
                ));
            }
            Src::Added | Src::Released => {
                let dt = if src == Src::Added {
                    Some(book.added.naive_local())
                } else {
                    book.released().and_then(|d| d.and_hms_opt(0, 0, 0))
                };
                let fmt = args.first().map(String::as_str).unwrap_or("");
                return Some(
                    dt.map(|d| sanitize(&format_dotnet_date(d, fmt)))
                        .unwrap_or_default(),
                );
            }
            _ => {}
        }

        if let Src::Std(f @ (Field::Manga | Field::SeriesComplete)) = src
            && (1..=2).contains(&args.len())
        {
            let no = args.len() == 2 && args[1] == "!";
            let (yes, is_no) = if f == Field::Manga {
                let m = book.info.manga;
                (
                    matches!(m, Manga::Yes | Manga::YesAndRightToLeft),
                    m == Manga::No,
                )
            } else {
                (
                    book.series_complete == YesNo::Yes,
                    book.series_complete == YesNo::No,
                )
            };
            let hit = if no { is_no } else { yes };
            return Some(if hit {
                sanitize(&args[0])
            } else {
                String::new()
            });
        }
        if args.len() == 2 {
            return self.multi_value(book, p, src, &args[0], &args[1]);
        }
        if args_match.is_empty() {
            if src == Src::Month && !name.ends_with('#') {
                return Some(self.month_name(p, book.info.month));
            }
            let raw = raw_src(book, src)?;
            return Some(if raw.empty_text() {
                String::new()
            } else {
                sanitize(&raw.py())
            });
        }
        if is_digits(args_match) {
            let mut padding: usize = args_match.parse().ok()?;
            let raw = raw_src(book, src)?;
            if raw.unset() {
                return Some(String::new());
            }
            if padding == 0 {
                let last = self.last(book).clone();
                padding = raw_src(&last, src)
                    .map(|r| r.py().chars().count())
                    .unwrap_or(0);
            }
            return Some(sanitize(&pad(&raw.py(), padding)));
        }
        None
    }

    fn month_name(&self, p: &Profile, month: i32) -> String {
        u32::try_from(month)
            .ok()
            .and_then(|m| p.months.get(&m))
            .map(|n| replace_illegal_characters(p, n))
            .unwrap_or_default()
    }

    // ----- multiple values --------------------------------------------------

    fn multi_value(
        &mut self,
        book: &Book,
        p: &Profile,
        src: Src,
        sep: &str,
        mode: &str,
    ) -> Option<String> {
        let field = match src {
            Src::Std(f)
                if matches!(
                    f.kind(),
                    FieldKind::Text | FieldKind::List | FieldKind::LongText | FieldKind::Number
                ) =>
            {
                f
            }
            _ => return None,
        };
        match mode {
            "issue" => Some(self.multi_value_issue(book, p, field, sep)),
            "series" => Some(self.multi_value_series(book, p, field, sep)),
            _ => None,
        }
    }

    fn multi_value_issue(&mut self, book: &Book, p: &Profile, field: Field, sep: &str) -> String {
        let i = &book.info;
        let index = (
            field,
            format!(
                "{}\u{1}{}\u{1}{}\u{1}{}",
                i.publisher, i.series, i.volume, i.number
            ),
        );
        if let Some(c) = self.multi_issue.get(&index) {
            return issue_string(p, &c.selected, sep, c.each_as_folder);
        }
        let values = split_list(&field.get(book));
        if values.is_empty() {
            self.multi_issue.insert(index, MultiChoice::default());
            return String::new();
        }
        if values.len() == 1 && p.dont_ask_when_multi_one {
            return replace_illegal_characters(p, &values[0]);
        }
        let mut preselected = Vec::new();
        let lists = self.always_used.entry(field).or_default();
        lists.sort_by_key(|l| Reverse(l.values.len()));
        for l in lists.iter() {
            if l.values.iter().all(|v| values.contains(v)) {
                if l.dont_ask {
                    return issue_string(p, &l.values, sep, l.folder);
                }
                preselected = l.values.clone();
                break;
            }
        }
        let req = MultiValueRequest {
            book,
            field,
            values,
            preselected,
            whole_series: false,
            book_text: format!("{} vol. {} #{}", i.series, i.volume, i.number),
        };
        let choice = self
            .interaction
            .choose_multi_values_ex(&req)
            .unwrap_or_default();
        let s = issue_string(p, &choice.selected, sep, choice.each_as_folder);
        if !choice.selected.is_empty() && choice.always_use {
            self.always_used.entry(field).or_default().push(AlwaysUsed {
                values: choice.selected.clone(),
                dont_ask: choice.dont_ask,
                folder: choice.each_as_folder,
            });
        }
        self.multi_issue.insert(index, choice);
        s
    }

    fn multi_value_series(&mut self, book: &Book, p: &Profile, field: Field, sep: &str) -> String {
        let index = (field, series_key(book));
        if !self.multi_series.contains_key(&index) {
            let mut values: Vec<String> = self
                .library
                .iter()
                .filter(|b| same_series(b, book))
                .chain(std::iter::once(book))
                .flat_map(|b| split_list(&field.get(b)))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            values.sort_by(|a, b| natural_cmp(a, b));
            let choice = if values.is_empty() {
                MultiChoice::default()
            } else {
                let req = MultiValueRequest {
                    book,
                    field,
                    values,
                    preselected: Vec::new(),
                    whole_series: true,
                    book_text: format!("{} vol. {}", book.info.series, book.info.volume),
                };
                self.interaction
                    .choose_multi_values_ex(&req)
                    .unwrap_or_default()
            };
            self.multi_series.insert(index.clone(), choice);
        }
        let c = &self.multi_series[&index];
        let book_values = split_list(&field.get(book));
        let (sep, book_values, selection) = if c.each_as_folder {
            (
                format!("{}{SEP}", replace_illegal_characters(p, sep)),
                book_values
                    .iter()
                    .map(|v| replace_illegal_characters(p, v))
                    .collect::<Vec<_>>(),
                c.selected
                    .iter()
                    .map(|v| replace_illegal_characters(p, v))
                    .collect::<Vec<_>>(),
            )
        } else {
            (sep.to_string(), book_values, c.selected.clone())
        };
        let items: Vec<&String> = if c.every_issue {
            selection.iter().collect()
        } else {
            selection
                .iter()
                .filter(|v| book_values.contains(v))
                .collect()
        };
        let joined = items
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(&sep);
        if c.each_as_folder {
            joined
        } else {
            replace_illegal_characters(p, &joined)
        }
    }

    // ----- paths ------------------------------------------------------------

    /// Computes (folder, file name, failed). When a "fail on empty" field is
    /// empty and the profile does not move failed books, the original folder
    /// and name are returned with `failed = true`.
    pub fn make_path(&mut self, book: &Book, profile: &Profile) -> (PathBuf, String, bool) {
        self.failed = false;
        self.failed_fields.clear();
        let original_dir = book
            .file_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        // File name first so that a failure switches the folder to the failed folder.
        let file = if profile.use_file_name {
            self.make_file_name(book, profile)
        } else {
            book.file_name()
        };
        let folder = if profile.use_folder {
            self.make_folder_path(book, profile)
        } else {
            original_dir.clone()
        };
        if self.failed && !profile.move_failed {
            return (original_dir, book.file_name(), true);
        }
        (folder, file, self.failed)
    }

    fn make_folder_path(&mut self, book: &Book, p: &Profile) -> PathBuf {
        let template = p.folder_template.trim().trim_matches(is_sep);
        let mut rel = PathBuf::new();
        if !template.is_empty() {
            let rough = self.expand(book, p, template);
            for line in rough.split(is_sep) {
                let line = if line.trim().is_empty() {
                    p.empty_folder.as_str()
                } else {
                    line
                };
                let line = replace_illegal_characters(p, line);
                let line = line.trim_matches(|c: char| c == '.' || c.is_whitespace());
                if !line.is_empty() {
                    rel.push(line);
                }
            }
        }
        let base = if self.failed && p.move_failed {
            &p.failed_folder
        } else {
            &p.base_folder
        };
        let path = Path::new(base).join(rel);
        if p.replace_multiple_spaces {
            PathBuf::from(SPACES_RE.replace_all(&path_str(&path), " ").into_owned())
        } else {
            path
        }
    }

    fn make_file_name(&mut self, book: &Book, p: &Profile) -> String {
        let name = self.expand(book, p, &p.file_template);
        let mut name = replace_illegal_characters(p, name.trim());
        if name.is_empty() {
            return String::new();
        }
        if p.replace_multiple_spaces {
            name = SPACES_RE.replace_all(&name, " ").into_owned();
        }
        name + &extension_of(&book.file_path)
    }

    // ----- rules ------------------------------------------------------------

    fn rule_value(&mut self, book: &Book, field: OrgField) -> String {
        match field {
            OrgField::Book(Field::Manga) => match book.info.manga {
                Manga::Unknown => "Unknown",
                Manga::No => "No",
                Manga::Yes => "Yes",
                Manga::YesAndRightToLeft => "Yes (Right to Left)",
            }
            .into(),
            OrgField::Book(f @ (Field::SeriesComplete | Field::BlackAndWhite)) => {
                let v = if f == Field::SeriesComplete {
                    book.series_complete
                } else {
                    book.info.black_and_white
                };
                match v {
                    YesNo::Unknown => "Unknown",
                    YesNo::No => "No",
                    YesNo::Yes => "Yes",
                }
                .into()
            }
            OrgField::Book(f) => raw_field(book, f).py(),
            OrgField::StartYear => self.earliest(book).info.year.to_string(),
            OrgField::StartMonth => self.earliest(book).info.month.to_string(),
            OrgField::EndYear => self.last(book).info.year.to_string(),
            OrgField::EndMonth => self.last(book).info.month.to_string(),
            OrgField::FirstIssueNumber => self.earliest(book).info.number.clone(),
            OrgField::LastIssueNumber => self.last(book).info.number.clone(),
            OrgField::FirstLetter => {
                let s = book.info.series.clone();
                FIRST_LETTER_RE
                    .captures(&s)
                    .map(|c| c["letter"].to_uppercase())
                    .unwrap_or_default()
            }
        }
    }

    fn rule_matches(&mut self, book: &Book, field: OrgField, op: RuleOp, value: &str) -> bool {
        let data = self.rule_value(book, field);
        let value = if field.is_yes_no() {
            canonical_yes_no(value)
        } else {
            value.to_string()
        };
        let (data, value) = (data.as_str(), value.as_str());
        match op {
            RuleOp::Is => data == value,
            RuleOp::IsNot => data != value,
            RuleOp::Contains => data.contains(value),
            RuleOp::DoesNotContain => !data.contains(value),
            RuleOp::GreaterThan => {
                match (value.trim().parse::<i64>(), data.trim().parse::<i64>()) {
                    (Ok(v), Ok(d)) => v < d,
                    _ => value < data,
                }
            }
            RuleOp::LessThan => match (value.trim().parse::<i64>(), data.trim().parse::<i64>()) {
                (Ok(v), Ok(d)) => v > d,
                _ => value > data,
            },
        }
    }

    /// `None` for an empty group (ignored by its parent).
    fn group_result(&mut self, book: &Book, g: &RuleGroup) -> Option<bool> {
        let (mut count, mut total) = (0, 0);
        for item in &g.items {
            let r = match item {
                RuleItem::Rule {
                    field,
                    operator,
                    value,
                } => Some(self.rule_matches(book, *field, *operator, value)),
                RuleItem::Group(sub) => self.group_result(book, sub),
            };
            if let Some(r) = r {
                total += 1;
                count += r as usize;
            }
        }
        (total > 0).then_some(match g.operator {
            GroupOp::Any => count > 0,
            GroupOp::All => count == total,
        })
    }

    /// True when the metadata rules allow the book to be moved.
    pub fn passes_rules(&mut self, book: &Book, p: &Profile) -> bool {
        match self.group_result(book, &p.exclude_rules) {
            None => true,
            Some(q) => match p.exclude_mode {
                ExcludeMode::Only => q,
                ExcludeMode::DoNot => !q,
            },
        }
    }

    // ----- preview ----------------------------------------------------------

    pub fn preview(&mut self, book: &Book, p: &Profile) -> String {
        let orig = path_str(&book.file_path);
        let why = |r: &str| format!("{orig} — {r}");
        if in_excluded_folder(book, p) {
            return why(tr(msg::EXCLUDED_PATH));
        }
        if !self.passes_rules(book, p) {
            return why(tr(msg::EXCLUDED_RULES));
        }
        if p.use_folder && p.base_folder.trim().is_empty() {
            return why(tr(msg::BASE_EMPTY));
        }
        let (folder, name, failed) = self.make_path(book, p);
        if failed && !p.move_failed {
            return why(&empty_fields_msg(&self.failed_fields));
        }
        if name.is_empty() {
            return why(tr(msg::EMPTY_NAME));
        }
        path_str(&folder.join(name))
    }

    // ----- run --------------------------------------------------------------

    /// Adds a log entry; `action` is an English action, translated here.
    fn log(&mut self, action: &'static str, path: impl AsRef<str>, message: impl Into<String>) {
        self.result.log.push(LogEntry {
            profile: self.profile_name.clone(),
            action: tr(action).to_string(),
            path: path.as_ref().to_string(),
            message: message.into(),
        });
    }

    fn current_path(&self, idx: usize, book: &Book) -> PathBuf {
        self.current_paths
            .get(&idx)
            .cloned()
            .unwrap_or_else(|| book.file_path.clone())
    }

    fn record_move(&mut self, id: u64, dest: &Path) {
        self.moved_ids.insert(id, dest.to_path_buf());
        if let Some(e) = self.result.moved.iter_mut().find(|(i, _)| *i == id) {
            e.1 = dest.to_path_buf();
        } else {
            self.result.moved.push((id, dest.to_path_buf()));
        }
    }

    /// Computes the destination of a book for one profile.
    fn create_book_path(
        &mut self,
        book: &Book,
        p: &Profile,
    ) -> Result<(PathBuf, Vec<OrgField>), Outcome> {
        let name = path_str(&book.file_path);
        if book.file_path.as_os_str().is_empty() {
            self.log(act::SKIPPED, book.caption(), tr(msg::NO_FILE));
            return Err(Outcome::Skipped);
        }
        if !book.file_path.exists() {
            self.log(act::FAILED, &name, tr(msg::FILE_MISSING));
            return Err(Outcome::Failed);
        }
        if in_excluded_folder(book, p) {
            self.log(act::SKIPPED, &name, tr(msg::EXCLUDED_PATH));
            return Err(Outcome::Skipped);
        }
        if !self.passes_rules(book, p) {
            self.log(act::SKIPPED, &name, tr(msg::EXCLUDED_RULES));
            return Err(Outcome::Skipped);
        }
        if p.use_folder && p.base_folder.trim().is_empty() {
            self.log(act::FAILED, &name, tr(msg::BASE_EMPTY));
            return Err(Outcome::Failed);
        }
        let (folder, file, failed) = self.make_path(book, p);
        if failed && !p.move_failed {
            let m = empty_fields_msg(&self.failed_fields);
            self.log(act::FAILED, &name, m);
            return Err(Outcome::Failed);
        }
        if file.is_empty() {
            self.log(act::FAILED, &name, tr(msg::EMPTY_NAME));
            return Err(Outcome::Failed);
        }
        Ok((folder.join(file), self.failed_fields.clone()))
    }

    /// Handles "already there" and case-only differences. Returns true when skipped.
    fn check_path_problems(&mut self, idx: usize, book: &Book, dest: &Path, p: &Profile) -> bool {
        let name = path_str(&book.file_path);
        if dest == book.file_path {
            self.log(act::SKIPPED, &name, tr(msg::ALREADY_THERE));
            return true;
        }
        let case_insensitive_fs = cfg!(any(windows, target_os = "macos"));
        if case_insensitive_fs && path_str(dest).to_lowercase() == name.to_lowercase() {
            if p.mode == Mode::Simulate {
                self.log(
                    act::RENAMING_SIM,
                    &name,
                    t!("to: {0}", path_str(dest)),
                );
            } else {
                match fs::rename(&book.file_path, dest) {
                    Ok(()) => {
                        self.current_paths.insert(idx, dest.to_path_buf());
                        if p.mode == Mode::Move {
                            self.record_move(book.id, dest);
                        }
                        self.log(act::RENAMED, &name, t!("to: {0}", path_str(dest)));
                    }
                    Err(e) => {
                        self.log(act::FAILED, &name, t!("An error occurred: {0}", e))
                    }
                }
            }
            self.log(act::SKIPPED, &name, tr(msg::ALREADY_THERE));
            return true;
        }
        false
    }

    pub fn run(&mut self, books: &[Book], profiles: &[Profile], undo_path: &Path) -> RunResult {
        let mut reports: Vec<ProfileReport> = profiles
            .iter()
            .map(|p| ProfileReport {
                success: 0,
                failed: 0,
                name: p.name.clone(),
                mode: p.mode,
            })
            .collect();
        self.result.simulated = profiles.iter().any(|p| p.mode == Mode::Simulate);

        // 1. Destination paths.
        let mut ops = Vec::new();
        for (idx, book) in books.iter().enumerate() {
            let mut chosen: Option<(PathBuf, usize, Vec<OrgField>)> = None;
            for (pi, p) in profiles.iter().enumerate() {
                self.profile_name = p.name.clone();
                match self.create_book_path(book, p) {
                    Err(Outcome::Failed) => {
                        reports[pi].failed += 1;
                        self.result.failed_or_skipped = true;
                    }
                    Err(_) => self.result.failed_or_skipped = true,
                    Ok((dest, failed_fields)) => {
                        if p.mode == Mode::Copy {
                            ops.push(Op {
                                idx,
                                dest,
                                profile: pi,
                                failed_fields,
                            });
                        } else {
                            if let Some((_, prev, _)) = &chosen {
                                self.profile_name = profiles[*prev].name.clone();
                                self.log(
                                    act::SKIPPED,
                                    path_str(&book.file_path),
                                    tr(msg::LATER_PROFILE),
                                );
                                self.result.failed_or_skipped = true;
                            }
                            chosen = Some((dest, pi, failed_fields));
                        }
                    }
                }
            }
            if let Some((dest, pi, failed_fields)) = chosen {
                self.profile_name = profiles[pi].name.clone();
                if self.check_path_problems(idx, book, &dest, &profiles[pi]) {
                    self.result.failed_or_skipped = true;
                } else {
                    ops.push(Op {
                        idx,
                        dest,
                        profile: pi,
                        failed_fields,
                    });
                }
            }
        }

        // 2. Operations, duplicates last.
        let total = ops.len();
        let mut done = 0;
        let mut held = Vec::new();
        let mut cancelled = false;
        for op in ops {
            if self.interaction.is_cancelled() {
                cancelled = true;
                self.log(
                    act::CANCELLED,
                    t!("{0} operations", total - done),
                    tr(msg::CANCELLED),
                );
                break;
            }
            self.profile_name = profiles[op.profile].name.clone();
            let r = self.process_op(books, &profiles[op.profile], &op);
            if r == Outcome::Duplicate {
                held.push(op);
                continue;
            }
            count(&mut reports[op.profile], r, &mut self.result);
            done += 1;
            self.interaction.progress(done, total);
        }
        if !cancelled {
            for op in held {
                if self.interaction.is_cancelled() {
                    self.log(
                        act::CANCELLED,
                        t!("{0} operations", total - done),
                        tr(msg::CANCELLED),
                    );
                    break;
                }
                self.profile_name = profiles[op.profile].name.clone();
                let r = self.process_duplicate(books, &profiles[op.profile], &op);
                count(&mut reports[op.profile], r, &mut self.result);
                done += 1;
                self.interaction.progress(done, total);
            }
        }

        if !self.undo.is_empty() {
            if let Some(d) = undo_path.parent() {
                let _ = fs::create_dir_all(d);
            }
            if let Ok(json) = serde_json::to_string_pretty(&self.undo)
                && let Err(e) = fs::write(undo_path, json)
            {
                self.profile_name.clear();
                self.log(
                    act::FAILED,
                    path_str(undo_path),
                    t!("Could not save the undo information: {0}", e),
                );
            }
        }

        self.result.summary = reports
            .iter()
            .map(|r| {
                let skipped = books.len().saturating_sub(r.success + r.failed);
                r.mode.report(&r.name, r.success, skipped, r.failed)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        std::mem::take(&mut self.result)
    }

    fn process_op(&mut self, books: &[Book], p: &Profile, op: &Op) -> Outcome {
        let book = &books[op.idx];
        let src = self.current_path(op.idx, book);
        if too_long(&op.dest) {
            self.log(act::FAILED, path_str(&src), too_long_msg(&op.dest));
            return Outcome::Failed;
        }
        if op.dest.exists() || self.moved_books.contains(&op.dest) {
            return Outcome::Duplicate;
        }
        if let Some(folder) = op.dest.parent()
            && !self.create_folder(folder, p, &src)
        {
            return Outcome::Failed;
        }
        self.finish_op(book, op, p, &src, &op.dest)
    }

    fn process_duplicate(&mut self, books: &[Book], p: &Profile, op: &Op) -> Outcome {
        let book = &books[op.idx];
        let src = self.current_path(op.idx, book);
        let mut dest = op.dest.clone();
        let mut current = book.clone();
        current.file_path = src.clone();
        match self.resolve_existing(&current, &mut dest, p.mode, p.copy_read_percentage) {
            Resolved::Proceed => {}
            Resolved::Skipped => return Outcome::Skipped,
            Resolved::Failed => return Outcome::Failed,
            Resolved::SimulatedOverwrite => {
                self.moved_books.insert(dest.clone());
                self.log(
                    p.mode.past(),
                    path_str(&src),
                    t!("to: {0}", path_str(&dest)),
                );
                return Outcome::Success;
            }
        }
        if let Some(folder) = dest.parent()
            && !self.create_folder(folder, p, &src)
        {
            return Outcome::Failed;
        }
        self.finish_op(book, op, p, &src, &dest)
    }

    fn finish_op(&mut self, book: &Book, op: &Op, p: &Profile, src: &Path, dest: &Path) -> Outcome {
        let old_folder = src.parent().map(Path::to_path_buf);
        let ok = self.transfer(book, op.idx, p, src, dest);
        if p.remove_empty_folders && p.mode == Mode::Move {
            if let Some(f) = old_folder {
                self.remove_empty_folders(&f, p);
            }
            if let Some(f) = dest.parent() {
                self.remove_empty_folders(f, p);
            }
        }
        if !ok {
            return Outcome::Failed;
        }
        if !op.failed_fields.is_empty() {
            let m = t!(
                "{0} {1} to {2}",
                empty_fields_msg(&op.failed_fields),
                tr(p.mode.past()).to_lowercase(),
                path_str(dest)
            );
            self.log(act::FAILED, path_str(src), m);
            return Outcome::Failed;
        }
        self.log(
            p.mode.past(),
            path_str(src),
            t!("to: {0}", path_str(dest)),
        );
        Outcome::Success
    }

    fn transfer(&mut self, book: &Book, idx: usize, p: &Profile, src: &Path, dest: &Path) -> bool {
        let pending = self.pending_last_page.take();
        let r = match p.mode {
            Mode::Simulate => {
                self.moved_books.insert(dest.to_path_buf());
                Ok(())
            }
            Mode::Move => move_path(src, dest).map(|()| {
                self.push_undo(&p.name, src, dest);
                self.current_paths.insert(idx, dest.to_path_buf());
                self.record_move(book.id, dest);
            }),
            Mode::Copy => copy_path(src, dest).map(|()| {
                if p.copy_add_to_library {
                    let mut nb = Book {
                        id: 0,
                        file_path: dest.to_path_buf(),
                        info: book.info.clone(),
                        series_complete: book.series_complete,
                        ..Default::default()
                    };
                    if let Some(lp) = pending {
                        nb.last_page_read = lp;
                    }
                    nb.refresh_file_stats();
                    self.result.added.push(nb);
                }
            }),
        };
        match r {
            Ok(()) => true,
            Err(e) => {
                self.log(
                    act::FAILED,
                    path_str(src),
                    t!("An error occurred: {0}", e),
                );
                false
            }
        }
    }

    fn push_undo(&mut self, profile: &str, from: &Path, to: &Path) {
        // A book moved several times keeps its original location.
        if let Some(e) = self.undo.iter_mut().find(|e| e.current == from) {
            e.current = to.to_path_buf();
        } else {
            self.undo.push(UndoEntry {
                profile: profile.to_string(),
                current: to.to_path_buf(),
                original: from.to_path_buf(),
            });
        }
    }

    fn create_folder(&mut self, folder: &Path, p: &Profile, src: &Path) -> bool {
        if folder.as_os_str().is_empty() || folder.exists() {
            return true;
        }
        if p.mode == Mode::Simulate {
            if self.created_paths.insert(folder.to_path_buf()) {
                self.log(act::FOLDER_CREATED, path_str(folder), "");
            }
            return true;
        }
        match fs::create_dir_all(folder) {
            Ok(()) => true,
            Err(e) => {
                let m = t!("Book {0} was not processed.\nError: {1}", path_str(src), e);
                self.log(act::FOLDER_FAILED, path_str(folder), m);
                false
            }
        }
    }

    /// Deletes `dir` and its parents while they are empty. Stops at excluded
    /// folders, the profile's base/failed folders and the home folder.
    fn remove_empty_folders(&self, dir: &Path, p: &Profile) {
        let home = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf());
        let mut d = dir.to_path_buf();
        loop {
            if !d.is_dir() || d.parent().is_none() {
                return;
            }
            let protected = p
                .excluded_empty_folders
                .iter()
                .chain([&p.base_folder, &p.failed_folder])
                .filter(|s| !s.trim().is_empty())
                .any(|s| same_path(Path::new(s), &d))
                || home.as_deref().is_some_and(|h| same_path(h, &d));
            if protected {
                return;
            }
            let empty = fs::read_dir(&d).is_ok_and(|mut it| it.next().is_none());
            if !empty {
                return;
            }
            let parent = d.parent().map(Path::to_path_buf);
            if fs::remove_dir(&d).is_err() {
                return;
            }
            match parent {
                Some(pd) => d = pd,
                None => return,
            }
        }
    }

    fn find_library_book(&self, path: &Path) -> Option<(u64, i32)> {
        self.library
            .iter()
            .filter(|b| !self.result.removed.contains(&b.id))
            .find(|b| self.moved_ids.get(&b.id).unwrap_or(&b.file_path) == path)
            .map(|b| (b.id, b.last_page_read))
    }

    fn rename_path(&self, path: &Path) -> Option<PathBuf> {
        let ext = extension_of(path);
        let s = path_str(path);
        let base = &s[..s.len() - ext.len()];
        let base = RENAME_SUFFIX_RE.replace(base, "").into_owned();
        (1..=100)
            .map(|i| PathBuf::from(format!("{base} ({i}){ext}")))
            .find(|c| !self.moved_books.contains(c) && !c.exists())
    }

    /// Asks what to do while `dest` is taken. `book.file_path` is the current path.
    fn resolve_existing(
        &mut self,
        book: &Book,
        dest: &mut PathBuf,
        mode: Mode,
        copy_read: bool,
    ) -> Resolved {
        let name = path_str(&book.file_path);
        loop {
            if !dest.exists() && !self.moved_books.contains(dest.as_path()) {
                return Resolved::Proceed;
            }
            let old = self.find_library_book(dest);
            if !self.dup_always {
                let size = fs::metadata(&*dest).ok().map(|m| m.len());
                let (action, all) = self.interaction.on_duplicate(book, dest, size);
                self.dup_action = action;
                if all {
                    self.dup_always = true;
                }
            }
            match self.dup_action {
                DuplicateAction::Skip => {
                    let m = t!(
                        "A file already exists at: {0} and the user declined to overwrite it",
                        path_str(dest)
                    );
                    self.log(act::SKIPPED, &name, m);
                    return Resolved::Skipped;
                }
                DuplicateAction::Rename => match self.rename_path(dest) {
                    Some(r) if too_long(&r) => {
                        self.log(act::FAILED, &name, too_long_msg(&r));
                        return Resolved::Failed;
                    }
                    Some(r) => *dest = r,
                    None => {
                        let m = t!("Could not find a free name for {0}", path_str(dest));
                        self.log(act::FAILED, &name, m);
                        return Resolved::Failed;
                    }
                },
                DuplicateAction::Overwrite => {
                    if mode == Mode::Simulate {
                        self.log(act::DELETED_SIM, path_str(dest), "");
                        return Resolved::SimulatedOverwrite;
                    }
                    if copy_read && let Some((_, lp)) = old {
                        if mode == Mode::Copy {
                            self.pending_last_page = Some(lp);
                        } else if book.id != 0 {
                            self.result.read_progress.push((book.id, lp));
                        }
                    }
                    if let Err(e) = remove_path(dest) {
                        let m = t!("Failed to overwrite {0}. Error: {1}", path_str(dest), e);
                        self.log(act::FAILED, &name, m);
                        return Resolved::Failed;
                    }
                    self.log(act::DELETED, path_str(dest), "");
                    if let Some((id, _)) = old {
                        self.result.removed.push(id);
                    }
                }
            }
        }
    }

    // ----- undo -------------------------------------------------------------

    pub fn undo(&mut self, settings: &OrganizerSettings, undo_path: &Path) -> RunResult {
        if !undo_path.exists() {
            self.result.summary = t!("Nothing to undo").into();
            return std::mem::take(&mut self.result);
        }
        let entries: Vec<UndoEntry> = fs::read_to_string(undo_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if entries.is_empty() {
            self.result.summary = t!("Error while loading the undo file").into();
            return std::mem::take(&mut self.result);
        }
        let (mut success, mut failed, mut skipped) = (0, 0, 0);
        let total = entries.len();
        let mut held = Vec::new();
        let mut cancelled = false;
        let tally = |r: Outcome, s: &mut usize, f: &mut usize, k: &mut usize| match r {
            Outcome::Success => *s += 1,
            Outcome::Failed => *f += 1,
            _ => *k += 1,
        };
        // Reverse order so that a book moved by several profiles ends back home.
        for e in entries.into_iter().rev() {
            if self.interaction.is_cancelled() {
                cancelled = true;
                let n = total - success - failed - skipped;
                self.log(act::CANCELLED, t!("{0} files", n), tr(msg::CANCELLED));
                break;
            }
            let r = self.undo_one(settings, &e, false);
            if r == Outcome::Duplicate {
                held.push(e);
                continue;
            }
            tally(r, &mut success, &mut failed, &mut skipped);
            self.interaction.progress(success + failed + skipped, total);
        }
        if !cancelled {
            for e in held {
                if self.interaction.is_cancelled() {
                    let n = total - success - failed - skipped;
                    self.log(act::CANCELLED, t!("{0} files", n), tr(msg::CANCELLED));
                    break;
                }
                let r = self.undo_one(settings, &e, true);
                tally(r, &mut success, &mut failed, &mut skipped);
                self.interaction.progress(success + failed + skipped, total);
            }
        }
        let _ = fs::remove_file(undo_path);
        if cancelled {
            skipped = total - success - failed;
        }
        self.result.failed_or_skipped = failed > 0 || skipped > 0;
        self.result.summary = t!(
            "Successfully moved: {0}\tFailed: {1}\tSkipped: {2}",
            success,
            failed,
            skipped
        );
        std::mem::take(&mut self.result)
    }

    fn undo_one(
        &mut self,
        settings: &OrganizerSettings,
        e: &UndoEntry,
        duplicate_pass: bool,
    ) -> Outcome {
        let profile = settings
            .profile(&e.profile)
            .cloned()
            .unwrap_or_else(|| Profile {
                name: e.profile.clone(),
                remove_empty_folders: false,
                ..Default::default()
            });
        self.profile_name = e.profile.clone();
        let name = path_str(&e.current);
        let lib_book = self
            .library
            .iter()
            .find(|b| self.moved_ids.get(&b.id).unwrap_or(&b.file_path) == &e.current)
            .cloned();
        if !e.current.exists() {
            self.log(act::FAILED, &name, tr(msg::FILE_MISSING));
            return Outcome::Failed;
        }
        if e.current == e.original {
            self.log(act::SKIPPED, &name, tr(msg::ALREADY_THERE));
            return Outcome::Skipped;
        }
        let mut dest = e.original.clone();
        if duplicate_pass {
            let mut b = lib_book.clone().unwrap_or_default();
            b.file_path = e.current.clone();
            match self.resolve_existing(&b, &mut dest, Mode::Move, profile.copy_read_percentage) {
                Resolved::Proceed => {}
                Resolved::Skipped => return Outcome::Skipped,
                _ => return Outcome::Failed,
            }
        } else if dest.exists() {
            return Outcome::Duplicate;
        }
        if let Some(folder) = dest.parent()
            && !self.create_folder(folder, &profile, &e.current)
        {
            return Outcome::Failed;
        }
        let r = move_path(&e.current, &dest);
        if profile.remove_empty_folders {
            if let Some(f) = e.current.parent() {
                self.remove_empty_folders(f, &profile);
            }
            if let Some(f) = dest.parent() {
                self.remove_empty_folders(f, &profile);
            }
        }
        match r {
            Ok(()) => {
                if let Some(b) = lib_book {
                    self.record_move(b.id, &dest);
                }
                self.log(act::RESTORED, &name, t!("to: {0}", path_str(&dest)));
                Outcome::Success
            }
            Err(err) => {
                self.log(
                    act::FAILED,
                    &name,
                    t!("An error occurred: {0}", err),
                );
                Outcome::Failed
            }
        }
    }
}

fn count(report: &mut ProfileReport, r: Outcome, result: &mut RunResult) {
    match r {
        Outcome::Success => report.success += 1,
        Outcome::Failed => {
            report.failed += 1;
            result.failed_or_skipped = true;
        }
        _ => result.failed_or_skipped = true,
    }
}

fn issue_string(p: &Profile, values: &[String], sep: &str, folder: bool) -> String {
    if folder {
        let sep = format!("{}{SEP}", replace_illegal_characters(p, sep));
        values
            .iter()
            .map(|v| replace_illegal_characters(p, v))
            .collect::<Vec<_>>()
            .join(&sep)
    } else {
        replace_illegal_characters(p, &values.join(sep))
    }
}

fn canonical_yes_no(v: &str) -> String {
    match v.trim().to_lowercase().as_str() {
        "yes" | "oui" => "Yes",
        "no" | "non" => "No",
        "yes (right to left)" | "yesandrighttoleft" | "oui (droite à gauche)" => {
            "Yes (Right to Left)"
        }
        _ => "Unknown",
    }
    .into()
}

fn in_excluded_folder(book: &Book, p: &Profile) -> bool {
    let path = path_str(&book.file_path);
    p.exclude_folders
        .iter()
        .any(|f| !f.is_empty() && path.contains(f.as_str()))
}

fn empty_fields_msg(fields: &[OrgField]) -> String {
    let names = fields
        .iter()
        .map(|f| f.label())
        .collect::<Vec<_>>()
        .join(", ");
    if fields.len() > 1 {
        t!("{0} are empty.", names)
    } else {
        t!("{0} is empty.", names)
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Destination full path of `book` with `profile`, or the original path
/// followed by the reason when the book would not be moved.
/// Multiple-value fields take every value (no dialog).
pub fn preview(book: &Book, library: &[Book], profile: &Profile) -> String {
    Organizer::new(library, &NoInteraction).preview(book, profile)
}

/// Runs the profiles (in order) on `books`. `library` is the whole library
/// (used by start/end year, first/last issue, series multiple values...).
/// The undo information of a run that moved files is written to `undo_path`.
pub fn run(
    books: &[Book],
    library: &[Book],
    profiles: &[Profile],
    interaction: &dyn Interaction,
    undo_path: &Path,
) -> RunResult {
    Organizer::new(library, interaction).run(books, profiles, undo_path)
}

/// Moves the files of the last run back to their original location and
/// deletes `undo_path`.
pub fn undo(
    library: &[Book],
    settings: &OrganizerSettings,
    interaction: &dyn Interaction,
    undo_path: &Path,
) -> RunResult {
    Organizer::new(library, interaction).undo(settings, undo_path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn book(series: &str, number: &str, volume: i32, year: i32, month: i32) -> Book {
        let mut b = Book::default();
        b.info.series = series.into();
        b.info.number = number.into();
        b.info.volume = volume;
        b.info.year = year;
        b.info.month = month;
        b.info.publisher = "Marvel".into();
        b.file_path = PathBuf::from(format!("/src/{series} {number}.cbz"));
        b
    }

    fn xmen() -> (Book, Vec<Book>) {
        let mut b = book("X-Men", "5", 1991, 1992, 3);
        b.info.count = 12;
        let first = book("X-Men", "1", 1991, 1991, 10);
        let last = book("X-Men", "150", 1991, 2003, 1);
        (b.clone(), vec![first, b, last])
    }

    fn profile() -> Profile {
        Profile {
            base_folder: "/base".into(),
            ..Default::default()
        }
    }

    fn expand(b: &Book, lib: &[Book], p: &Profile, t: &str) -> String {
        Organizer::new(lib, &NoInteraction).expand(b, p, t)
    }

    #[test]
    fn default_templates() {
        let (b, lib) = xmen();
        let p = profile();
        assert_eq!(
            preview(&b, &lib, &p),
            "/base/Marvel/X-Men (1991)/X-Men Vol.1991 #05 (of 12) (March, 1992).cbz"
        );
        let mut b2 = b.clone();
        b2.info.imprint = "Epic".into();
        b2.info.format = "TPB".into();
        b2.info.month = -1;
        b2.info.count = -1;
        assert_eq!(
            preview(&b2, &lib, &p),
            "/base/Marvel/Epic/X-Men (1991 TPB)/X-Men Vol.1991 #05 (1992).cbz"
        );
    }

    #[test]
    fn nested_blocks() {
        let (mut b, lib) = xmen();
        let p = profile();
        let t = "{ ({<month>, }<year>)}";
        assert_eq!(expand(&b, &lib, &p, t), " (March, 1992)");
        b.info.month = -1;
        assert_eq!(expand(&b, &lib, &p, t), " (1992)");
        b.info.month = 3;
        b.info.year = -1;
        assert_eq!(expand(&b, &lib, &p, t), "");
    }

    #[test]
    fn padding() {
        let (mut b, lib) = xmen();
        let p = profile();
        assert_eq!(expand(&b, &lib, &p, "{#<number3>}"), "#005");
        assert_eq!(expand(&b, &lib, &p, "{#<number0>}"), "#005"); // last issue is #150
        b.info.number = "7.1".into();
        assert_eq!(expand(&b, &lib, &p, "{<number3>}"), "007.1");
        b.info.number = "-1".into();
        assert_eq!(expand(&b, &lib, &p, "{<number3>}"), "-001");
        b.info.number = "12a".into();
        assert_eq!(expand(&b, &lib, &p, "{<number3>}"), "12a");
        assert_eq!(expand(&b, &lib, &p, "{<month#2>}"), "03");
        assert_eq!(expand(&b, &lib, &p, "{<month#>}"), "3");
        assert_eq!(pad("5", 0), "5");
    }

    #[test]
    fn invalid_blocks_stay() {
        let (b, lib) = xmen();
        let p = profile();
        assert_eq!(expand(&b, &lib, &p, "{<foo>}-{<series>}"), "{<foo>}-X-Men");
        let mut o = Organizer::new(&lib, &NoInteraction);
        let mut p2 = profile();
        p2.file_template = "{<foo>}".into();
        assert_eq!(o.make_file_name(&b, &p2), "{[foo]}.cbz");
    }

    #[test]
    fn conditionals_and_inversions() {
        let (b, lib) = xmen();
        let p = profile();
        assert_eq!(
            expand(&b, &lib, &p, "{Maison <?publisher(Marvel)>}"),
            "Maison "
        );
        assert_eq!(expand(&b, &lib, &p, "{Maison <?publisher(DC)>}"), "");
        assert_eq!(expand(&b, &lib, &p, "{X<?series(!X-)>}"), "X");
        assert_eq!(expand(&b, &lib, &p, "{X<?series(!Men)>}"), "");
        assert_eq!(expand(&b, &lib, &p, "{X<?series(!)>}"), "");
        assert_eq!(expand(&b, &lib, &p, "{<?series>}"), "{<?series>}");
        assert_eq!(
            expand(&b, &lib, &p, "{Sans collection<!imprint>}"),
            "Sans collection"
        );
        assert_eq!(expand(&b, &lib, &p, "{Sans série<!series>}"), "");
        assert_eq!(expand(&b, &lib, &p, "{Pas DC<!publisher(DC)>}"), "Pas DC");
        assert_eq!(expand(&b, &lib, &p, "{Pas Marvel<!publisher(Marvel)>}"), "");
        assert_eq!(expand(&b, &lib, &p, "{Pas X<!series(!X)>}"), "");
        // Field definitions inside arguments (resolved first).
        let mut b2 = b.clone();
        b2.info.alternate_series = "X-Men".into();
        assert_eq!(
            expand(&b2, &lib, &p, "{Même<?series({<altSeries>})>}"),
            "Même"
        );
    }

    #[test]
    fn first_letter() {
        let lib = vec![];
        let p = profile();
        let mut b = book("The Walking Dead", "1", 1, 2003, 10);
        assert_eq!(expand(&b, &lib, &p, "{<first(Series)>}"), "W");
        b.info.series = "Les Tuniques bleues".into();
        assert_eq!(expand(&b, &lib, &p, "{<first(Series)>}"), "T");
        b.info.series = "astérix".into();
        assert_eq!(expand(&b, &lib, &p, "{<first(ShadowSeries)>}"), "A");
        b.info.series = "Thorgal".into();
        assert_eq!(expand(&b, &lib, &p, "{<first(Series)>}"), "T");
        assert_eq!(expand(&b, &lib, &p, "{<first(Publisher)>}"), "M");
        assert_eq!(expand(&b, &lib, &p, "{<first>}"), "{<first>}");
        assert_eq!(expand(&b, &lib, &p, "{<first(Nope)>}"), "{<first(Nope)>}");
    }

    #[test]
    fn counter_runs_across_books() {
        let (b, lib) = xmen();
        let p = profile();
        let mut o = Organizer::new(&lib, &NoInteraction);
        assert_eq!(o.expand(&b, &p, "{<counter(5)(2)(3)>}"), "005");
        assert_eq!(o.expand(&b, &p, "{<counter(5)(2)(3)>}"), "007");
        assert_eq!(o.expand(&b, &p, "{<counter(5)(2)()>}"), "9");
    }

    #[test]
    fn computed_series_values() {
        let (b, lib) = xmen();
        let p = profile();
        assert_eq!(
            expand(&b, &lib, &p, "{<startyear>}-{<EndYear>}"),
            "1991-2003"
        );
        assert_eq!(
            expand(&b, &lib, &p, "{<startmonth>}/{<startmonth#2>}"),
            "October/10"
        );
        assert_eq!(
            expand(&b, &lib, &p, "{<EndMonth>}{ <EndMonth#2>}"),
            "January 01"
        );
        assert_eq!(
            expand(&b, &lib, &p, "{<firstissuenumber3>}-{<lastissuenumber>}"),
            "001-150"
        );
    }

    #[test]
    fn yes_no_read_custom_language_dates() {
        let (mut b, lib) = xmen();
        let p = profile();
        b.info.manga = Manga::YesAndRightToLeft;
        assert_eq!(expand(&b, &lib, &p, "{[<manga(Manga)>]}"), "[Manga]");
        assert_eq!(expand(&b, &lib, &p, "{<manga(Manga)(!)>}"), "");
        b.series_complete = YesNo::No;
        assert_eq!(
            expand(&b, &lib, &p, "{<seriesComplete(En cours)(!)>}"),
            "En cours"
        );
        b.info.page_count = 10;
        b.last_page_read = 9;
        assert_eq!(expand(&b, &lib, &p, "{<read(Lu)(>)(90)>}"), "Lu");
        assert_eq!(expand(&b, &lib, &p, "{<read(Lu)(<)(90)>}"), "");
        assert_eq!(expand(&b, &lib, &p, "{<read>}"), "100");
        b.custom_values.insert("shelf".into(), "A:1".into());
        assert_eq!(expand(&b, &lib, &p, "{<Custom(shelf)>}"), "A - 1");
        assert_eq!(expand(&b, &lib, &p, "{<Custom(none)>}"), "");
        b.info.language_iso = "fr".into();
        assert_eq!(expand(&b, &lib, &p, "{<language>}"), "French");
        b.info.day = 1;
        b.info.year = 2013;
        b.info.month = 2;
        assert_eq!(expand(&b, &lib, &p, "{<ReleasedDate(dd/MM/yy)>}"), "010213");
        assert_eq!(
            expand(&b, &lib, &p, "{<ReleasedDate(MMMM yyyy)>}"),
            "February 2013"
        );
        assert_eq!(
            expand(&b, &lib, &p, "{<ReleasedDate(MMM d, yyy)>}"),
            "Feb 1, 2013"
        );
        assert_eq!(
            expand(&b, &lib, &p, "{<ReleasedDate(D)>}"),
            "Friday, February 1, 2013"
        );
    }

    #[test]
    fn illegal_characters_and_spaces() {
        let lib = vec![];
        let mut p = profile();
        p.file_template = "{<title>}".into();
        let mut b = book("S", "1", 1, 2000, 1);
        b.info.title = "What: If?  <Yes>|\"No\"".into();
        let mut o = Organizer::new(&lib, &NoInteraction);
        assert_eq!(o.make_file_name(&b, &p), "What - If [Yes]!'No'.cbz");
        p.illegal_characters.insert("é".into(), "e".into());
        p.illegal_characters.remove("?");
        b.info.title = "été?".into();
        assert_eq!(o.make_file_name(&b, &p), "ete.cbz");
    }

    #[test]
    fn empty_data_and_failures() {
        let (mut b, lib) = xmen();
        b.file_path = std::env::temp_dir().join("x.cbz");
        let mut p = profile();
        p.empty_data
            .insert(Field::Publisher.into(), "Inconnu".into());
        p.empty_data.insert(OrgField::StartYear, "????".into());
        b.info.publisher.clear();
        let lib2: Vec<Book> = vec![];
        assert_eq!(
            expand(&b, &lib2, &p, "{<publisher>}/{(<startyear>)}"),
            "Inconnu/(1992)"
        );
        b.info.year = -1;
        assert_eq!(expand(&b, &lib2, &p, "{(<startyear>)}"), "????");

        p.fail_empty_values = true;
        p.failed_fields = vec![Field::Imprint.into()];
        p.folder_template = "{<imprint>}".into();
        let out = preview(&b, &lib, &p);
        assert!(
            out.starts_with(&path_str(&b.file_path)) && out.ends_with("Imprint is empty."),
            "{out}"
        );
        p.move_failed = true;
        p.failed_folder = "/failed".into();
        p.file_template = "{<series>}".into();
        assert_eq!(preview(&b, &lib, &p), "/failed/X-Men.cbz");
        // Empty folder names.
        p.fail_empty_values = false;
        p.folder_template = "{<imprint>}/{<series>}".into();
        assert_eq!(preview(&b, &lib, &p), "/base/X-Men/X-Men.cbz");
        p.empty_folder = "Divers".into();
        assert_eq!(preview(&b, &lib, &p), "/base/Divers/X-Men/X-Men.cbz");
    }

    struct Scripted {
        choice: MultiChoice,
        asked: RefCell<usize>,
        dup: DuplicateAction,
    }

    impl Interaction for Scripted {
        fn choose_multi_values(&self, _b: &Book, _f: Field, _v: &[String]) -> Option<MultiChoice> {
            *self.asked.borrow_mut() += 1;
            Some(self.choice.clone())
        }
        fn on_duplicate(&self, _b: &Book, _d: &Path, _s: Option<u64>) -> (DuplicateAction, bool) {
            (self.dup, false)
        }
    }

    #[test]
    fn multi_values() {
        let (mut b, mut lib) = xmen();
        let p = profile();
        b.info.writer = "Chris Claremont, Jim Lee".into();
        assert_eq!(
            expand(&b, &lib, &p, "{<writer(, )(issue)>}"),
            "Chris Claremont, Jim Lee"
        );
        let mut one = b.clone();
        one.info.writer = "Chris Claremont".into();
        assert_eq!(
            expand(&one, &lib, &p, "{<writer(&)(issue)>}"),
            "Chris Claremont"
        );
        assert_eq!(
            expand(&b, &lib, &p, "{<writer(, )(bogus)>}"),
            "{<writer(, )(bogus)>}"
        );

        // Folder per value, asked once per issue.
        let ia = Scripted {
            choice: MultiChoice {
                selected: vec!["Jim Lee".into(), "Chris Claremont".into()],
                each_as_folder: true,
                ..Default::default()
            },
            asked: RefCell::new(0),
            dup: DuplicateAction::Skip,
        };
        let mut pf = profile();
        pf.folder_template = "{<writer( &)(issue)>}".into();
        pf.file_template = "{<series>}".into();
        let mut o = Organizer::new(&lib, &ia);
        let (folder, _, _) = o.make_path(&b, &pf);
        assert_eq!(folder, PathBuf::from("/base/Jim Lee &/Chris Claremont"));
        o.make_path(&b, &pf);
        assert_eq!(*ia.asked.borrow(), 1);

        // Series mode: one question for the series, issue keeps only its own values.
        lib[0].info.writer = "Louise Simonson".into();
        let ia = Scripted {
            choice: MultiChoice {
                selected: vec!["Jim Lee".into(), "Louise Simonson".into()],
                ..Default::default()
            },
            asked: RefCell::new(0),
            dup: DuplicateAction::Skip,
        };
        let mut o = Organizer::new(&lib, &ia);
        assert_eq!(o.expand(&b, &p, "{<writer(, )(series)>}"), "Jim Lee");
        assert_eq!(
            o.expand(&lib[0], &p, "{<writer(, )(series)>}"),
            "Louise Simonson"
        );
        assert_eq!(*ia.asked.borrow(), 1);
    }

    #[test]
    fn rules() {
        let (b, lib) = xmen();
        let mut p = profile();
        let rule = |field: OrgField, operator, value: &str| RuleItem::Rule {
            field,
            operator,
            value: value.into(),
        };
        p.exclude_rules.items = vec![rule(Field::Publisher.into(), RuleOp::Is, "Marvel")];
        let mut o = Organizer::new(&lib, &NoInteraction);
        assert!(!o.passes_rules(&b, &p));
        p.exclude_mode = ExcludeMode::Only;
        assert!(o.passes_rules(&b, &p));
        p.exclude_rules = RuleGroup {
            operator: GroupOp::All,
            items: vec![
                rule(Field::Year.into(), RuleOp::GreaterThan, "1990"),
                RuleItem::Group(RuleGroup {
                    operator: GroupOp::Any,
                    items: vec![
                        rule(OrgField::StartYear, RuleOp::LessThan, "1991"),
                        rule(Field::Series.into(), RuleOp::Contains, "Men"),
                    ],
                }),
                RuleItem::Group(RuleGroup::default()),
            ],
        };
        assert!(o.passes_rules(&b, &p));
        p.exclude_rules.items[0] = rule(Field::Manga.into(), RuleOp::Is, "Yes");
        assert!(!o.passes_rules(&b, &p));
        p.exclude_rules.items[0] = rule(Field::Manga.into(), RuleOp::Is, "Unknown");
        assert!(o.passes_rules(&b, &p));
        p.exclude_rules.items.clear();
        assert!(o.passes_rules(&b, &p));
        // Serialization round trip.
        let json = serde_json::to_string(&OrganizerSettings::default()).unwrap();
        let s: OrganizerSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s.profiles[0].months[&13], "Spring");
    }

    fn setup(dir: &Path, names: &[&str]) -> Vec<Book> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let path = dir.join("in").join(format!("{n}.cbz"));
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, n.as_bytes()).unwrap();
                let mut b = book(
                    "Blake et Mortimer",
                    &(i + 1).to_string(),
                    -1,
                    1950 + i as i32,
                    -1,
                );
                b.info.publisher = "Dargaud".into();
                b.id = i as u64 + 1;
                b.file_path = path;
                b
            })
            .collect()
    }

    fn run_profile(dir: &Path, mode: Mode) -> Profile {
        Profile {
            name: "P".into(),
            mode,
            base_folder: path_str(&dir.join("out")),
            folder_template: "{<publisher>}\\{<series>}".into(),
            file_template: "{<series>}{ #<number2>}".into(),
            ..Default::default()
        }
    }

    #[test]
    fn run_simulate() {
        let tmp = tempfile::tempdir().unwrap();
        let books = setup(tmp.path(), &["a", "b"]);
        let p = run_profile(tmp.path(), Mode::Simulate);
        let undo_file = tmp.path().join("undo.json");
        let r = run(&books, &books, &[p], &NoInteraction, &undo_file);
        assert!(books.iter().all(|b| b.file_path.exists()));
        assert!(!tmp.path().join("out").exists());
        assert!(r.moved.is_empty() && r.simulated && !undo_file.exists());
        let actions: Vec<&str> = r.log.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(
            actions,
            [
                "Created Folder",
                "Moved (simulated)",
                "Moved (simulated)"
            ]
        );
        assert!(
            r.log[1]
                .message
                .ends_with("Dargaud/Blake et Mortimer/Blake et Mortimer #01.cbz")
        );
        assert!(
            r.summary
                .contains("Successfully moved (simulated): 2\tSkipped: 0\tFailed: 0")
        );
    }

    #[test]
    fn run_move_and_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let books = setup(tmp.path(), &["a", "b"]);
        let p = run_profile(tmp.path(), Mode::Move);
        let undo_file = tmp.path().join("undo.json");
        let r = run(
            &books,
            &books,
            std::slice::from_ref(&p),
            &NoInteraction,
            &undo_file,
        );
        let dest = tmp
            .path()
            .join("out/Dargaud/Blake et Mortimer/Blake et Mortimer #01.cbz");
        assert!(dest.exists());
        assert!(
            !tmp.path().join("in").exists(),
            "empty source folder removed"
        );
        assert_eq!(r.moved[0], (1, dest.clone()));
        assert_eq!(r.log[0].action, "Moved");
        assert!(undo_file.exists());

        // Second run: already there.
        let mut moved = books.clone();
        for (b, (_, path)) in moved.iter_mut().zip(&r.moved) {
            b.file_path = path.clone();
        }
        let r2 = run(
            &moved,
            &moved,
            std::slice::from_ref(&p),
            &NoInteraction,
            &undo_file,
        );
        assert!(r2.moved.is_empty() && r2.failed_or_skipped);
        assert!(r2.summary.contains("Skipped: 2"));

        let settings = OrganizerSettings {
            profiles: vec![p],
            last_used: vec![],
        };
        let u = undo(&moved, &settings, &NoInteraction, &undo_file);
        assert!(books.iter().all(|b| b.file_path.exists()), "{:?}", u.log);
        // Empty folders are removed up to (not including) the base folder.
        assert!(!tmp.path().join("out/Dargaud").exists());
        assert!(tmp.path().join("out").exists());
        assert!(!undo_file.exists());
        assert_eq!(u.moved.len(), 2);
        assert!(u.summary.starts_with("Successfully moved: 2"));
    }

    #[test]
    fn run_copy_adds_to_library() {
        let tmp = tempfile::tempdir().unwrap();
        let books = setup(tmp.path(), &["a"]);
        let p = run_profile(tmp.path(), Mode::Copy);
        let r = run(
            &books,
            &books,
            &[p],
            &NoInteraction,
            &tmp.path().join("undo.json"),
        );
        assert!(books[0].file_path.exists());
        assert_eq!(r.added.len(), 1);
        assert_eq!(r.added[0].id, 0);
        assert!(r.added[0].file_path.exists());
        assert_eq!(r.added[0].info.series, "Blake et Mortimer");
        assert!(!tmp.path().join("undo.json").exists());
    }

    #[test]
    fn duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let mut books = setup(tmp.path(), &["a", "b"]);
        // Both books compute the same destination.
        books[1].info.number = "1".into();
        let mut p = run_profile(tmp.path(), Mode::Move);
        p.remove_empty_folders = false;
        let undo_file = tmp.path().join("undo.json");
        let r = run(
            &books,
            &books,
            std::slice::from_ref(&p),
            &NoInteraction,
            &undo_file,
        );
        let folder = tmp.path().join("out/Dargaud/Blake et Mortimer");
        assert!(folder.join("Blake et Mortimer #01.cbz").exists());
        assert!(folder.join("Blake et Mortimer #01 (1).cbz").exists());
        assert_eq!(r.moved.len(), 2);

        // Skip, then overwrite (the library book at the destination is removed).
        let mut lib = books.clone();
        lib[0].file_path = folder.join("Blake et Mortimer #01.cbz");
        lib[0].last_page_read = 7;
        lib[1].file_path = folder.join("Blake et Mortimer #01 (1).cbz");
        let third = setup(&tmp.path().join("t"), &["c"]).remove(0);
        let mut third = Book { id: 3, ..third };
        third.info.number = "1".into();
        lib.push(third.clone());
        let skip = Scripted {
            choice: MultiChoice::default(),
            asked: RefCell::new(0),
            dup: DuplicateAction::Skip,
        };
        let r = run(
            std::slice::from_ref(&third),
            &lib,
            std::slice::from_ref(&p),
            &skip,
            &undo_file,
        );
        assert!(r.moved.is_empty() && third.file_path.exists());
        assert_eq!(r.log[0].action, "Skipped");
        let over = Scripted {
            dup: DuplicateAction::Overwrite,
            ..skip
        };
        let r = run(std::slice::from_ref(&third), &lib, &[p], &over, &undo_file);
        assert_eq!(r.removed, vec![1]);
        assert_eq!(r.read_progress, vec![(3, 7)]);
        assert_eq!(
            fs::read_to_string(folder.join("Blake et Mortimer #01.cbz")).unwrap(),
            "c"
        );
    }

    #[test]
    fn later_profile_wins_and_rules_skip() {
        let tmp = tempfile::tempdir().unwrap();
        let books = setup(tmp.path(), &["a"]);
        let mut p1 = run_profile(tmp.path(), Mode::Simulate);
        p1.name = "Premier".into();
        let mut p2 = p1.clone();
        p2.name = "Second".into();
        p2.file_template = "{<series>} - {<number3>}".into();
        let r = run(
            &books,
            &books,
            &[p1.clone(), p2],
            &NoInteraction,
            &tmp.path().join("u"),
        );
        assert_eq!(r.log[0].profile, "Premier");
        assert_eq!(r.log[0].message, tr(msg::LATER_PROFILE));
        assert!(
            r.log
                .last()
                .unwrap()
                .message
                .ends_with("Blake et Mortimer - 001.cbz")
        );
        p1.exclude_folders = vec![path_str(&tmp.path().join("in"))];
        let r = run(&books, &books, &[p1], &NoInteraction, &tmp.path().join("u"));
        assert_eq!(r.log[0].message, tr(msg::EXCLUDED_PATH));
        assert!(report_text(&r).contains("Skipped: "));
    }

    #[test]
    fn dotnet_dates() {
        let dt = chrono::NaiveDate::from_ymd_opt(2013, 2, 1)
            .unwrap()
            .and_hms_opt(14, 5, 9)
            .unwrap();
        assert_eq!(format_dotnet_date_in(dt, "D", false), "Friday, February 1, 2013");
        assert_eq!(format_dotnet_date_in(dt, "", false), "2/1/2013 2:05:09 PM");
        assert_eq!(format_dotnet_date_in(dt, "MMM d, yyy", false), "Feb 1, 2013");
        assert_eq!(format_dotnet_date_in(dt, "dd/MM/yy", true), "01/02/13");
        assert_eq!(format_dotnet_date_in(dt, "MM/dd/yy", true), "02/01/13");
        assert_eq!(format_dotnet_date_in(dt, "M", true), "1 février");
        assert_eq!(format_dotnet_date_in(dt, "Y", true), "février 2013");
        assert_eq!(format_dotnet_date_in(dt, "", true), "01/02/2013 14:05:09");
        assert_eq!(
            format_dotnet_date_in(dt, "yyyy-MM-dd 'à' HH'h'mm", true),
            "2013-02-01 à 14h05"
        );
        assert_eq!(format_dotnet_date_in(dt, "MMMM dd, yyy", true), "février 01, 2013");
    }
}
