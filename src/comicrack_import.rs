//! Import of a ComicRack configuration: the settings of the Bedetheque
//! Scrapper 2 plugin (`App.Config`), the profiles of the Library Organizer
//! plugin (`losettingsx.dat`) and a few preferences of ComicRack itself
//! (`Config.xml`: scan at startup, missing files, export presets).
//!
//! The files can be given as a folder or a ZIP archive; they are recognized
//! by their names. Windows paths are translated with a list of
//! (Windows prefix, local folder) pairs.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Result, bail};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::archive::OutputFormat;
use crate::bedetheque::{Ambiguity, ScraperSettings};
use crate::convert::{DoublePages, ExportSettings, Naming, PageFormat, PageResize, Target};
use crate::organizer::{ExcludeMode, GroupOp, Mode, OrgField, OrganizerSettings, Profile, RuleGroup, RuleItem, RuleOp};
use crate::settings::Settings;

/// Option of a settings struct, given by the XML key that sets it.
type Flag<T> = (&'static str, fn(&mut T) -> &mut bool);

/// A small XML element tree.
#[derive(Debug, Default, Clone)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Element>,
    text: String,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Text of a child element.
    fn get(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }

    fn bool(&self, name: &str) -> Option<bool> {
        self.get(name).map(|v| matches!(v.trim(), "1" | "true" | "True"))
    }
}

fn parse_xml(xml: &str) -> Result<Element> {
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));
    let mut stack: Vec<Element> = vec![Element::default()];
    let attrs = |e: &quick_xml::events::BytesStart| {
        e.attributes()
            .flatten()
            .map(|a| {
                (
                    a.key.local_name().as_ref().to_string(),
                    html_escape::decode_html_entities(a.value.as_ref()).into_owned(),
                )
            })
            .collect::<Vec<_>>()
    };
    loop {
        match reader.read_event()? {
            Event::Start(e) => stack.push(Element {
                name: e.local_name().as_ref().to_string(),
                attrs: attrs(&e),
                ..Default::default()
            }),
            Event::Empty(e) => {
                let el = Element { name: e.local_name().as_ref().to_string(), attrs: attrs(&e), ..Default::default() };
                stack.last_mut().unwrap().children.push(el);
            }
            Event::Text(t) => stack.last_mut().unwrap().text.push_str(&t.xml10_content()),
            Event::CData(t) => stack.last_mut().unwrap().text.push_str(&t),
            Event::GeneralRef(r) => {
                let s = html_escape::decode_html_entities(&format!("&{};", &*r)).into_owned();
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Event::End(_) => {
                let el = stack.pop().unwrap();
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => bail!("XML"),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    stack.pop().and_then(|root| root.children.into_iter().next()).ok_or_else(|| anyhow::anyhow!("XML"))
}

/// What was found in a ComicRack configuration.
#[derive(Debug, Default)]
pub struct ComicRackConfig {
    pub scraper: Option<ScraperSettings>,
    pub organizer: Option<OrganizerSettings>,
    pub scan_on_startup: Option<bool>,
    pub remove_missing: Option<bool>,
    /// The confirmation before converting books was hidden in ComicRack.
    pub no_convert_confirmation: bool,
    pub export: Option<ExportSettings>,
    pub export_presets: Vec<ExportSettings>,
    /// Windows path prefixes used by the profiles ("X:\", "\\server\share").
    pub windows_roots: Vec<String>,
}

/// Reads the configuration files from a folder or a ZIP archive.
pub fn read(path: &Path) -> Result<ComicRackConfig> {
    let mut files: Vec<(String, String)> = Vec::new();
    if path.is_dir() {
        for e in walkdir::WalkDir::new(path).max_depth(4).into_iter().flatten() {
            if e.file_type().is_file()
                && let Ok(s) = std::fs::read_to_string(e.path())
            {
                files.push((e.file_name().to_string_lossy().into_owned(), s));
            }
        }
    } else {
        let mut zip = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path)?))?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            let name = f.name()?.rsplit('/').next().unwrap_or("").to_string();
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() {
                files.push((name, s));
            }
        }
    }
    let mut cfg = ComicRackConfig::default();
    for (name, content) in &files {
        let lower = name.to_lowercase();
        if lower.ends_with("app.config") {
            cfg.scraper = Some(scraper_settings(&parse_xml(content)?));
        } else if lower.ends_with("losettingsx.dat") {
            cfg.organizer = Some(organizer_settings(&parse_xml(content)?, &mut cfg.windows_roots));
        } else if lower.ends_with("config.xml") {
            comicrack_settings(&parse_xml(content)?, &mut cfg);
        }
    }
    if cfg.scraper.is_none() && cfg.organizer.is_none() && cfg.export.is_none() && cfg.scan_on_startup.is_none() {
        bail!(t!("No ComicRack configuration file (App.Config, losettingsx.dat, Config.xml) was found"));
    }
    Ok(cfg)
}

/// Bedetheque Scrapper 2 `App.Config`: `<configuration><KEY>value</KEY>…`.
fn scraper_settings(root: &Element) -> ScraperSettings {
    let mut s = ScraperSettings::default();
    let flags: [Flag<ScraperSettings>; 21] = [
        ("CBSeries", |s| &mut s.series),
        ("CBTitle", |s| &mut s.title),
        ("CBWriter", |s| &mut s.writer),
        ("CBPenciller", |s| &mut s.penciller),
        ("CBColorist", |s| &mut s.colorist),
        ("CBLetterer", |s| &mut s.letterer),
        ("CBInker", |s| &mut s.inker),
        ("CBCouverture", |s| &mut s.cover_artist),
        ("CBEditor", |s| &mut s.publisher),
        ("CBLanguage", |s| &mut s.language),
        ("CBFormat", |s| &mut s.format),
        ("CBRating", |s| &mut s.rating),
        ("CBPrinted", |s| &mut s.printed),
        ("CBImprint", |s| &mut s.imprint),
        ("CBSynopsys", |s| &mut s.synopsis),
        ("CBCount", |s| &mut s.count),
        ("CBNotes", |s| &mut s.notes),
        ("CBStatus", |s| &mut s.status),
        ("CBGenre", |s| &mut s.genre),
        ("CBISBN", |s| &mut s.isbn),
        ("ONESHOTFORMAT", |s| &mut s.one_shot_format),
    ];
    for (key, field) in flags {
        if let Some(v) = root.bool(key) {
            *field(&mut s) = v;
        }
    }
    // CBWeb = 2 is the former "short web link" setting.
    match root.get("CBWeb").map(str::trim) {
        Some("2") => {
            s.web = true;
            s.short_web_link = true;
        }
        Some(v) => s.web = v == "1",
        None => {}
    }
    if let Some(v) = root.bool("ShortWebLink") {
        s.short_web_link |= v;
    }
    if s.one_shot_format {
        s.format = false;
    }
    if let Some(v) = root.get("TBTags") {
        s.tags = v.trim().to_string();
    }
    let bools: [Flag<ScraperSettings>; 9] = [
        ("COUNTOF", |s| &mut s.count_of),
        ("COUNTFINIE", |s| &mut s.count_only_if_complete),
        ("SerieResumeEverywhere", |s| &mut s.serie_resume_everywhere),
        ("AcceptGenericArtists", |s| &mut s.accept_generic_artists),
        ("TITLEIT", |s| &mut s.titleit),
        ("FORMATARTICLES", |s| &mut s.format_articles),
        ("AlwaysChooseSerie", |s| &mut s.always_choose_serie),
        ("CBRescrape", |s| &mut s.rescrape),
        ("DBGONOFF", |s| &mut s.debug_log),
    ];
    for (key, field) in bools {
        if let Some(v) = root.bool(key) {
            *field(&mut s) = v;
        }
    }
    // "PopUpEditionForm" off means the edition is chosen automatically.
    if let Some(v) = root.bool("PopUpEditionForm") {
        s.auto_choose_edition = !v;
    }
    if let Some(v) = root.get("SUBPATT") {
        s.subpatt = v.to_string();
    }
    if let Some(v) = root.get("ARTICLES") {
        s.articles = v.trim().to_string();
    }
    if let Some(v) = root.get("LANGENFR") {
        s.lang_fr = v.trim().eq_ignore_ascii_case("FR");
    }
    let num = |k: &str| root.get(k).and_then(|v| v.trim().parse::<u32>().ok());
    if let Some(v) = num("PadNumber") {
        s.pad_number = v;
    }
    if let Some(v) = num("TIMEOUT") {
        s.max_books = v;
    }
    if let Some(v) = num("TIMEOUTS") {
        s.delay_secs = v;
    }
    if let Some(v) = num("TIMEPOPUP") {
        s.popup_timeout_secs = v.max(1);
    }
    if let Some(v) = num("CBStop") {
        s.ambiguity = match v {
            0 => Ambiguity::Skip,
            1 => Ambiguity::Ask,
            _ => Ambiguity::AskWithTimeout,
        };
    }
    s
}

/// Field of a Library Organizer setting: a ComicRack property name
/// ("ShadowSeries", "Publisher") or a display name ("Age Rating").
fn org_field(name: &str) -> Option<OrgField> {
    let key: String = name.trim().trim_start_matches("Shadow").split_whitespace().collect();
    match key.as_str() {
        "Language" | "LanguageAsText" => OrgField::from_key("LanguageISO"),
        "MainCharacterorTeam" => OrgField::from_key("MainCharacterOrTeam"),
        "BlackandWhite" => OrgField::from_key("BlackAndWhite"),
        k => OrgField::from_key(k),
    }
}

fn rule_op(op: &str) -> RuleOp {
    match op.trim().to_lowercase().as_str() {
        "does not contain" => RuleOp::DoesNotContain,
        "greater than" => RuleOp::GreaterThan,
        "less than" => RuleOp::LessThan,
        "is" => RuleOp::Is,
        "is not" => RuleOp::IsNot,
        _ => RuleOp::Contains,
    }
}

fn group_op(op: Option<&str>) -> GroupOp {
    if op.is_some_and(|o| o.eq_ignore_ascii_case("All")) { GroupOp::All } else { GroupOp::Any }
}

fn rule_group(el: &Element) -> RuleGroup {
    let mut g = RuleGroup { operator: group_op(el.attr("Operator")), items: Vec::new() };
    for c in &el.children {
        match c.name.as_str() {
            "ExcludeRule" => {
                if let Some(field) = c.attr("Field").and_then(org_field) {
                    g.items.push(RuleItem::Rule {
                        field,
                        operator: rule_op(c.attr("Operator").unwrap_or("")),
                        value: c.attr("Value").unwrap_or("").to_string(),
                    });
                }
            }
            "ExcludeGroup" => g.items.push(RuleItem::Group(rule_group(c))),
            _ => {}
        }
    }
    g
}

/// Windows path prefix of `path`: "X:\" or "\\server\share\".
fn windows_root(path: &str) -> Option<String> {
    let p = path.trim();
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return Some(format!("{}:\\", (b[0] as char).to_ascii_uppercase()));
    }
    if let Some(rest) = p.strip_prefix("\\\\") {
        let mut parts = rest.split('\\').filter(|s| !s.is_empty());
        if let (Some(server), Some(share)) = (parts.next(), parts.next()) {
            return Some(format!("\\\\{server}\\{share}\\"));
        }
    }
    None
}

/// Library Organizer `losettingsx.dat`.
fn organizer_settings(root: &Element, roots: &mut Vec<String>) -> OrganizerSettings {
    let mut out = OrganizerSettings { profiles: Vec::new(), last_used: Vec::new() };
    if let Some(v) = root.attr("LastUsed") {
        out.last_used = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    }
    let mut add_root = |p: &str| {
        if let Some(r) = windows_root(p)
            && !roots.contains(&r)
        {
            roots.push(r);
        }
    };
    for p in root.children("Profile") {
        let mut prof = Profile { name: p.attr("Name").unwrap_or("ComicRack").to_string(), ..Profile::default() };
        let text = |k: &str| p.get(k).map(str::to_string);
        if let Some(v) = text("BaseFolder") {
            add_root(&v);
            prof.base_folder = v;
        }
        if let Some(v) = text("FolderTemplate") {
            prof.folder_template = v;
        }
        if let Some(v) = text("FileTemplate") {
            prof.file_template = v;
        }
        if let Some(v) = text("EmptyFolder") {
            prof.empty_folder = v;
        }
        if let Some(v) = text("FailedFolder") {
            add_root(&v);
            prof.failed_folder = v;
        }
        let bools: [Flag<Profile>; 10] = [
            ("UseFolder", |p| &mut p.use_folder),
            ("UseFileName", |p| &mut p.use_file_name),
            ("CopyMode", |p| &mut p.copy_add_to_library),
            ("DontAskWhenMultiOne", |p| &mut p.dont_ask_when_multi_one),
            ("RemoveEmptyFolder", |p| &mut p.remove_empty_folders),
            ("FailEmptyValues", |p| &mut p.fail_empty_values),
            ("MoveFailed", |p| &mut p.move_failed),
            ("ReplaceMultipleSpaces", |p| &mut p.replace_multiple_spaces),
            ("CopyReadPercentage", |p| &mut p.copy_read_percentage),
            ("AutoSpaceFields", |p| &mut p.auto_space_fields),
        ];
        for (key, field) in bools {
            if let Some(v) = p.bool(key) {
                *field(&mut prof) = v;
            }
        }
        prof.mode = match p.get("Mode").map(str::trim) {
            Some("Copy") => Mode::Copy,
            Some("Simulate" | "Test") => Mode::Simulate,
            _ => Mode::Move,
        };
        if let Some(e) = p.child("EmptyData") {
            prof.empty_data = e
                .children("Item")
                .filter_map(|i| Some((org_field(i.attr("Name")?)?, i.attr("Value")?.to_string())))
                .collect();
        }
        if let Some(e) = p.child("IllegalCharacters") {
            let items: BTreeMap<String, String> = e
                .children("Item")
                .filter_map(|i| Some((i.attr("Name")?.to_string(), i.attr("Value").unwrap_or("").to_string())))
                .collect();
            if !items.is_empty() {
                prof.illegal_characters = items;
            }
        }
        if let Some(e) = p.child("Months") {
            for i in e.children("Item") {
                if let (Some(n), Some(v)) = (i.attr("Name").and_then(|n| n.parse().ok()), i.attr("Value")) {
                    prof.months.insert(n, v.to_string());
                }
            }
        }
        let list = |k: &str| -> Vec<String> {
            p.child(k)
                .map(|e| e.children("Item").map(|i| i.text.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default()
        };
        prof.exclude_folders = list("ExcludeFolders");
        prof.excluded_empty_folders = list("ExcludedEmptyFolder");
        for f in prof.exclude_folders.iter().chain(&prof.excluded_empty_folders) {
            add_root(f);
        }
        prof.failed_fields = list("FailedFields").iter().filter_map(|f| org_field(f)).collect();
        if let Some(r) = p.child("ExcludeRules") {
            prof.exclude_rules = rule_group(r);
            prof.exclude_mode = match r.attr("ExcludeMode").or_else(|| p.get("ExcludeMode")) {
                Some("Only") => ExcludeMode::Only,
                _ => ExcludeMode::DoNot,
            };
        }
        out.profiles.push(prof);
    }
    out
}

/// One `ExportSetting` of ComicRack's Config.xml (absent elements have
/// ComicRack's default values, which are also ours).
fn export_setting(e: &Element) -> ExportSettings {
    let mut s = ExportSettings { name: e.attr("Name").unwrap_or("ComicRack").to_string(), ..Default::default() };
    s.format = match e.get("FormatId").and_then(|v| v.trim().parse::<i32>().ok()) {
        Some(0) => None,
        Some(3) | Some(9) => Some(OutputFormat::Cbr),
        Some(6) => Some(OutputFormat::Cb7),
        _ => Some(OutputFormat::Cbz),
    };
    s.target = match e.get("Target").map(str::trim) {
        Some("SameAsSource") => Target::SameAsSource,
        Some("ReplaceSource") => Target::ReplaceSource,
        _ => Target::NewFolder,
    };
    if let Some(v) = e.get("TargetFolder") {
        s.target_folder = v.to_string();
    }
    s.naming = match e.get("Naming").map(str::trim) {
        Some("Caption") => Naming::Caption,
        Some("Custom") => Naming::Custom,
        _ => Naming::Filename,
    };
    if let Some(v) = e.get("CustomName") {
        s.custom_name = v.to_string();
    }
    let bools: [Flag<ExportSettings>; 8] = [
        ("DeleteOriginal", |s| &mut s.delete_original),
        ("AddToLibrary", |s| &mut s.add_to_library),
        ("Overwrite", |s| &mut s.overwrite),
        ("Combine", |s| &mut s.combine),
        ("EmbedComicInfo", |s| &mut s.embed_comic_info),
        ("RemovePages", |s| &mut s.remove_pages),
        ("DontEnlarge", |s| &mut s.dont_enlarge),
        ("KeepOriginalImageNames", |s| &mut s.keep_original_names),
    ];
    for (key, field) in bools {
        if let Some(v) = e.bool(key) {
            *field(&mut s) = v;
        }
    }
    s.compression = match e.get("ComicCompression").map(str::trim) {
        Some("Medium") => 1,
        Some("Strong") => 2,
        _ => 0,
    };
    if let Some(v) = e.get("IncludePages") {
        s.include_pages = v.to_string();
    }
    if let Some(v) = e.get("TagsToAppend") {
        s.tags_to_append = v.to_string();
    }
    s.page_format = match e.get("PageType").map(str::trim) {
        Some("Jpeg") => PageFormat::Jpeg,
        Some("Png") => PageFormat::Png,
        Some("Gif") => PageFormat::Gif,
        Some("Tiff") => PageFormat::Tiff,
        Some("Bmp") => PageFormat::Bmp,
        Some("Webp") => PageFormat::Webp,
        _ => PageFormat::Original,
    };
    let num = |k: &str| e.get(k).and_then(|v| v.trim().parse::<u32>().ok());
    if let Some(v) = num("PageCompression") {
        s.page_quality = v.clamp(1, 100) as u8;
    }
    if let Some(v) = num("PageWidth") {
        s.page_width = v;
    }
    if let Some(v) = num("PageHeight") {
        s.page_height = v;
    }
    s.page_resize = match e.get("PageResize").map(str::trim) {
        Some("WidthHeight") => PageResize::WidthHeight,
        Some("Width") => PageResize::Width,
        Some("Height") => PageResize::Height,
        _ => PageResize::Original,
    };
    s.double_pages = match e.get("DoublePages").map(str::trim) {
        Some("Split") => DoublePages::Split,
        Some("Rotate") => DoublePages::Rotate,
        Some("AdaptWidth") => DoublePages::AdaptWidth,
        _ => DoublePages::Keep,
    };
    s
}

/// ComicRack's own `Config.xml` (`<Settings>`).
fn comicrack_settings(root: &Element, cfg: &mut ComicRackConfig) {
    cfg.scan_on_startup = root.bool("ScanStartup");
    cfg.remove_missing = root.bool("RemoveMissingFilesOnFullScan");
    cfg.no_convert_confirmation = root.get("HiddenMessageBoxes").is_some_and(|v| v.contains("ConvertComics"));
    cfg.export = root.child("CurrentExportSetting").map(export_setting);
    if let Some(p) = root.child("ExportUserPresets") {
        cfg.export_presets = p.children.iter().map(export_setting).collect();
    }
}

/// Translates a Windows path with the (prefix, local folder) pairs; paths
/// without a known prefix are kept.
pub fn map_path(path: &str, mappings: &[(String, String)]) -> String {
    for (win, local) in mappings {
        if local.trim().is_empty() {
            continue;
        }
        if path.len() >= win.len() && path[..win.len()].eq_ignore_ascii_case(win) {
            let rest = path[win.len()..].replace('\\', "/");
            let base = std::path::PathBuf::from(local.trim());
            return if rest.is_empty() { base } else { base.join(rest) }.to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

/// Which parts of the configuration to apply.
pub struct ImportChoices {
    pub scraper: bool,
    pub organizer: bool,
    pub preferences: bool,
    pub export: bool,
}

/// Applies the configuration to the settings; returns a short report.
pub fn apply(cfg: &ComicRackConfig, choices: &ImportChoices, mappings: &[(String, String)], s: &mut Settings) -> Vec<String> {
    let mut report = Vec::new();
    if choices.scraper
        && let Some(sc) = &cfg.scraper
    {
        s.scraper = sc.clone();
        report.push(t!("Bedetheque: settings imported").to_string());
    }
    if choices.organizer
        && let Some(org) = &cfg.organizer
    {
        let mut org = org.clone();
        for p in &mut org.profiles {
            p.base_folder = map_path(&p.base_folder, mappings);
            p.failed_folder = map_path(&p.failed_folder, mappings);
            for f in p.exclude_folders.iter_mut().chain(p.excluded_empty_folders.iter_mut()) {
                *f = map_path(f, mappings);
            }
        }
        report.push(t!("File organization: {0} profile(s) imported", org.profiles.len()));
        s.organizer = org;
    }
    if choices.preferences {
        if let Some(v) = cfg.scan_on_startup {
            s.scan_on_startup = v;
        }
        if let Some(v) = cfg.remove_missing {
            s.remove_missing = v;
        }
        if cfg.no_convert_confirmation {
            s.confirm_convert = false;
        }
        report.push(t!("Preferences imported").to_string());
    }
    if choices.export {
        if let Some(e) = &cfg.export {
            let mut e = e.clone();
            e.target_folder = map_path(&e.target_folder, mappings);
            s.export = e;
        }
        for p in &cfg.export_presets {
            let mut p = p.clone();
            p.target_folder = map_path(&p.target_folder, mappings);
            s.export_presets.retain(|x| x.name != p.name);
            s.export_presets.push(p);
        }
        if cfg.export.is_some() || !cfg.export_presets.is_empty() {
            report.push(t!("Export settings imported").to_string());
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const LO: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Profiles LastUsed="BDs">
  <Profile Name="BDs" Version="2.1">
    <FolderTemplate>BDs\{&lt;series&gt;}{ (&lt;startyear&gt;)}</FolderTemplate>
    <BaseFolder>X:\</BaseFolder>
    <FileTemplate>{&lt;series&gt;}{ #&lt;number2&gt;}{ [&lt;title&gt;]}{ (&lt;year&gt;)}</FileTemplate>
    <EmptyData><Item Name="ShadowSeries" Value="Inconnue" /></EmptyData>
    <IllegalCharacters><Item Name="?" Value="" /><Item Name=":" Value=" - " /></IllegalCharacters>
    <Months><Item Name="1" Value="Janvier" /></Months>
    <UseFolder>true</UseFolder>
    <Mode>Copy</Mode>
    <FailedFields><Item>Age Rating</Item></FailedFields>
    <ExcludeFolders><Item>X:\Temp</Item></ExcludeFolders>
    <ExcludeRules Operator="All" ExcludeMode="Only">
      <ExcludeRule Field="Publisher" Operator="is" Value="Dargaud" />
      <ExcludeGroup Operator="Any"><ExcludeRule Field="Black And White" Operator="is not" Value="Yes" /></ExcludeGroup>
    </ExcludeRules>
  </Profile>
</Profiles>"#;

    #[test]
    fn organizer_profiles() {
        let mut roots = Vec::new();
        let org = organizer_settings(&parse_xml(LO).unwrap(), &mut roots);
        assert_eq!(roots, ["X:\\"]);
        assert_eq!(org.last_used, ["BDs"]);
        let p = &org.profiles[0];
        assert_eq!(p.folder_template, "BDs\\{<series>}{ (<startyear>)}");
        assert_eq!(p.mode, Mode::Copy);
        assert_eq!(p.months.get(&1).map(String::as_str), Some("Janvier"));
        assert_eq!(p.empty_data.get(&OrgField::from_key("Series").unwrap()).map(String::as_str), Some("Inconnue"));
        assert_eq!(p.failed_fields, [OrgField::from_key("AgeRating").unwrap()]);
        assert_eq!(p.exclude_mode, ExcludeMode::Only);
        assert_eq!(p.exclude_rules.operator, GroupOp::All);
        assert_eq!(p.exclude_rules.items.len(), 2);
        assert_eq!(map_path(&p.base_folder, &[("X:\\".into(), "/home/me/temp".into())]), "/home/me/temp");
        assert_eq!(map_path("x:\\Temp\\A", &[("X:\\".into(), "/m".into())]), "/m/Temp/A");
    }

    #[test]
    fn scraper_options() {
        let xml = "<configuration><CBSeries>0</CBSeries><CBWeb>2</CBWeb><TIMEOUTS>3</TIMEOUTS>\
                   <CBStop>0</CBStop><PopUpEditionForm>1</PopUpEditionForm><LANGENFR>EN</LANGENFR>\
                   <TBTags>\n  </TBTags></configuration>";
        let s = scraper_settings(&parse_xml(xml).unwrap());
        assert!(!s.series && s.web && s.short_web_link);
        assert_eq!((s.delay_secs, s.ambiguity, s.auto_choose_edition, s.lang_fr), (3, Ambiguity::Skip, false, false));
        assert_eq!(s.tags, "");
    }

    #[test]
    fn comicrack_preferences() {
        let xml = "<Settings><ScanStartup>true</ScanStartup><HiddenMessageBoxes>ConvertComics</HiddenMessageBoxes>\
                   <CurrentExportSetting Name=\"Convertir en CBZ\"><FormatId>2</FormatId><Target>ReplaceSource</Target>\
                   </CurrentExportSetting></Settings>";
        let mut cfg = ComicRackConfig::default();
        comicrack_settings(&parse_xml(xml).unwrap(), &mut cfg);
        assert_eq!(cfg.scan_on_startup, Some(true));
        assert!(cfg.no_convert_confirmation);
        let e = cfg.export.unwrap();
        assert_eq!((e.format, e.target), (Some(OutputFormat::Cbz), Target::ReplaceSource));
    }
}
