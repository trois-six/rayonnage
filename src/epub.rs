//! EPUB books: page images in reading order and OPF metadata.
//!
//! An EPUB is a ZIP archive. `META-INF/container.xml` points to the OPF
//! package document, whose spine lists the content documents in reading
//! order. Comic EPUBs (fixed layout) have one image per content document;
//! the images are taken from the spine, in that order.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};
use std::sync::LazyLock;

use anyhow::{Result, anyhow};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::model::ComicInfo;

/// What the OPF says about the book.
#[derive(Debug, Default)]
pub struct Package {
    /// Archive paths of the page images, in reading order.
    pub pages: Vec<String>,
    /// Number of images in the package (cheap page count).
    pub image_count: usize,
    pub info: ComicInfo,
}

fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<String> {
    let mut f = zip.by_name(name)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s)
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .map(|a| html_escape::decode_html_entities(a.value.as_ref()).into_owned())
}

/// Directory part of an archive path ("OEBPS/content.opf" → "OEBPS/").
fn dir_of(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..=i]).unwrap_or("")
}

/// Resolves a relative, percent-encoded reference against `base_dir`.
fn resolve(base_dir: &str, href: &str) -> String {
    let href = href.split(['#', '?']).next().unwrap_or("");
    let href = urlencoding::decode(href).map(|c| c.into_owned()).unwrap_or_else(|_| href.to_string());
    let joined = if href.starts_with('/') { href.trim_start_matches('/').to_string() } else { format!("{base_dir}{href}") };
    let mut parts: Vec<&str> = Vec::new();
    for p in joined.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// Images referenced by an (X)HTML or SVG content document, in order.
fn images_of(doc: &str) -> Vec<String> {
    static IMG: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?is)<(?:img|image)\b[^>]*?\s(?:src|xlink:href|href)\s*=\s*["']([^"']+)["']"#).unwrap()
    });
    IMG.captures_iter(doc).map(|c| c[1].to_string()).collect()
}

/// Text of an OPF description, without its HTML markup.
fn plain_text(s: &str) -> String {
    static TAGS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?s)<[^>]*>").unwrap());
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n").replace("</p>", "\n");
    let s = TAGS.replace_all(&s, "");
    html_escape::decode_html_entities(&s)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[derive(Default)]
struct Creator {
    id: Option<String>,
    role: Option<String>,
    name: String,
}

/// Reads the OPF package of an EPUB. With `resolve_pages`, the content
/// documents of the spine are read to find their images (one read per page);
/// otherwise only `image_count` gives an estimate of the page count.
pub fn read_package<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, resolve_pages: bool) -> Result<Package> {
    let container = read_entry(zip, "META-INF/container.xml")?;
    let opf_path = {
        let mut reader = Reader::from_str(&container);
        let mut found = None;
        loop {
            match reader.read_event()? {
                Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "rootfile" => {
                    found = attr(&e, "full-path");
                    break;
                }
                Event::Eof => break,
                _ => {}
            }
        }
        found.ok_or_else(|| anyhow!(t!("Invalid EPUB: no package document")))?
    };
    let opf = read_entry(zip, &opf_path)?;
    let base = dir_of(&opf_path).to_string();

    // id → (href, media type, properties)
    let mut manifest: HashMap<String, (String, String, String)> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    let mut info = ComicInfo::default();
    let mut creators: Vec<Creator> = Vec::new();
    let mut refines: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut subjects: Vec<String> = Vec::new();
    let mut collections: Vec<(Option<String>, String)> = Vec::new();
    let mut cover_id: Option<String> = None;

    let mut reader = Reader::from_str(&opf);
    let mut in_metadata = false;
    // Element being read inside <metadata>: (local name, start element).
    let mut current: Option<(String, BytesStart<'static>)> = None;
    let mut text = String::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_string();
                if name == "metadata" {
                    in_metadata = true;
                } else if in_metadata {
                    current = Some((name, e.into_owned()));
                    text.clear();
                }
            }
            Event::Empty(e) => match e.local_name().as_ref() {
                "item" => {
                    if let (Some(id), Some(href)) = (attr(&e, "id"), attr(&e, "href")) {
                        let mt = attr(&e, "media-type").unwrap_or_default();
                        let props = attr(&e, "properties").unwrap_or_default();
                        manifest.insert(id, (resolve(&base, &href), mt, props));
                    }
                }
                "itemref" => {
                    if attr(&e, "linear").as_deref() != Some("no")
                        && let Some(id) = attr(&e, "idref")
                    {
                        spine.push(id);
                    }
                }
                // EPUB 2 / calibre metadata.
                "meta" => match (attr(&e, "name").as_deref(), attr(&e, "content")) {
                    (Some("calibre:series"), Some(v)) => info.series = v.trim().to_string(),
                    (Some("calibre:series_index"), Some(v)) => info.number = clean_number(&v),
                    (Some("cover"), Some(v)) => cover_id = Some(v),
                    _ => {}
                },
                _ => {}
            },
            Event::Text(t) => {
                if current.is_some() {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if current.is_some() {
                    text.push_str(&t);
                }
            }
            Event::GeneralRef(r) => {
                if current.is_some() {
                    text.push_str(&html_escape::decode_html_entities(&format!("&{};", &*r)));
                }
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_string();
                if name == "metadata" {
                    in_metadata = false;
                } else if let Some((cur, start)) = current.take().filter(|(c, _)| *c == name) {
                    let value = text.trim().to_string();
                    match cur.as_str() {
                        "title" if info.title.is_empty() => info.title = value,
                        "creator" | "contributor" => creators.push(Creator {
                            id: attr(&start, "id"),
                            role: attr(&start, "role"),
                            name: value,
                        }),
                        "publisher" => info.publisher = value,
                        "language" if info.language_iso.is_empty() => {
                            info.language_iso = value.split(['-', '_']).next().unwrap_or("").to_lowercase()
                        }
                        "description" => info.summary = plain_text(&value),
                        "subject" if !value.is_empty() => subjects.push(value),
                        "date" => set_date(&mut info, &value),
                        "identifier" => {
                            let digits: String = value.chars().filter(char::is_ascii_digit).collect();
                            let scheme = attr(&start, "scheme").unwrap_or_default().to_lowercase();
                            if (scheme == "isbn" || value.to_lowercase().contains("isbn")) && matches!(digits.len(), 10 | 13)
                                || (digits.len() == 13 && digits.starts_with("97") && value.len() <= 20)
                            {
                                info.gtin = digits;
                            }
                        }
                        // EPUB 3 refinements (creator roles, series position...).
                        "meta" => {
                            let property = attr(&start, "property").unwrap_or_default();
                            match attr(&start, "refines") {
                                Some(r) => refines
                                    .entry(r.trim_start_matches('#').to_string())
                                    .or_default()
                                    .push((property, value)),
                                None if property == "belongs-to-collection" => {
                                    collections.push((attr(&start, "id"), value))
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    // Creators, with their MARC relator role (EPUB 2 attribute or EPUB 3 refinement).
    let mut by_role: HashMap<&'static str, Vec<String>> = HashMap::new();
    for c in &creators {
        if c.name.is_empty() {
            continue;
        }
        let role = c
            .role
            .clone()
            .or_else(|| {
                c.id.as_ref()
                    .and_then(|id| refines.get(id))
                    .and_then(|r| r.iter().find(|(p, _)| p == "role").map(|(_, v)| v.clone()))
            })
            .unwrap_or_else(|| "aut".into());
        let field = match role.as_str() {
            "ill" | "art" | "drm" => "penciller",
            "clr" => "colorist",
            "cov" => "cover",
            "edt" => "editor",
            "trl" => "translator",
            "ink" => "inker",
            "ltr" => "letterer",
            "aut" | "wrt" | "sce" | "cwt" | "aus" => "writer",
            _ => continue,
        };
        let names = by_role.entry(field).or_default();
        if !names.contains(&c.name) {
            names.push(c.name.clone());
        }
    }
    let join = |k: &str| by_role.get(k).map(|v| v.join(", ")).unwrap_or_default();
    info.writer = join("writer");
    info.penciller = join("penciller");
    info.colorist = join("colorist");
    info.cover_artist = join("cover");
    info.editor = join("editor");
    info.translator = join("translator");
    info.inker = join("inker");
    info.letterer = join("letterer");
    if !subjects.is_empty() {
        info.genre = subjects.join(", ");
    }
    // EPUB 3 series: a collection of type "series" with its position.
    if info.series.is_empty() {
        for (id, name) in &collections {
            let refs = id.as_ref().and_then(|id| refines.get(id));
            let kind = refs.and_then(|r| r.iter().find(|(p, _)| p == "collection-type")).map(|(_, v)| v.as_str());
            if kind.is_none_or(|k| k == "series") {
                info.series = name.clone();
                if let Some((_, pos)) = refs.and_then(|r| r.iter().find(|(p, _)| p == "group-position")) {
                    info.number = clean_number(pos);
                }
                break;
            }
        }
    }

    let is_image = |mt: &str| mt.starts_with("image/");
    let image_count = manifest
        .values()
        .filter(|(h, mt, _)| is_image(mt) && crate::archive::is_image_name(h))
        .count();
    let spine_items: Vec<&(String, String, String)> = spine.iter().filter_map(|id| manifest.get(id)).collect();
    let mut pages = Vec::new();
    if resolve_pages {
        let names: HashSet<String> = zip.file_names_raw().map(|n| String::from_utf8_lossy(n).into_owned()).collect();
        let mut seen = HashSet::new();
        // The cover first when it is not part of the spine.
        let cover = manifest
            .values()
            .find(|(_, mt, props)| is_image(mt) && props.split_whitespace().any(|p| p == "cover-image"))
            .map(|(h, _, _)| h.clone())
            .or_else(|| cover_id.as_ref().and_then(|id| manifest.get(id)).map(|(h, _, _)| h.clone()));
        for (href, mt, _) in &spine_items {
            let found: Vec<String> = if is_image(mt) {
                vec![href.clone()]
            } else {
                let doc = read_entry(zip, href).unwrap_or_default();
                images_of(&doc).iter().map(|src| resolve(dir_of(href), src)).collect()
            };
            for img in found {
                if names.contains(&img) && crate::archive::is_image_name(&img) && seen.insert(img.clone()) {
                    pages.push(img);
                }
            }
        }
        if let Some(c) = cover
            && names.contains(&c)
            && !seen.contains(&c)
        {
            pages.insert(0, c);
        }
    }
    Ok(Package { pages, image_count, info })
}

fn clean_number(v: &str) -> String {
    let v = v.trim();
    v.strip_suffix(".0").unwrap_or(v).to_string()
}

/// "2014", "2014-03" or "2014-03-12T…" → year, month, day.
fn set_date(info: &mut ComicInfo, value: &str) {
    let mut parts = value.split(['-', 'T']).map(|p| p.trim().parse::<i32>().ok());
    if let Some(Some(y)) = parts.next()
        && (1000..10000).contains(&y)
        && info.year < 0
    {
        info.year = y;
        info.month = parts.next().flatten().filter(|m| (1..=12).contains(m)).unwrap_or(-1);
        info.day = parts.next().flatten().filter(|d| (1..=31).contains(d)).unwrap_or(-1);
    }
}

#[cfg(test)]
pub mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    /// A small fixed-layout comic EPUB (EPUB 3 with calibre series metadata).
    pub fn sample_epub() -> Vec<u8> {
        let png = {
            let img = image::RgbImage::from_pixel(4, 6, image::Rgb([10, 20, 200]));
            let mut c = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(img).write_to(&mut c, image::ImageFormat::Png).unwrap();
            c.into_inner()
        };
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, data: &[u8]| {
            zw.start_file(name, opts).unwrap();
            zw.write_all(data).unwrap();
        };
        add("mimetype", b"application/epub+zip");
        add(
            "META-INF/container.xml",
            br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        );
        add(
            "OEBPS/content.opf",
            r##"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>L'Héritage du diable</dc:title>
    <dc:creator id="c1">Jean Dufaux</dc:creator>
    <meta refines="#c1" property="role" scheme="marc:relators">aut</meta>
    <dc:creator id="c2">Félix Meynet</dc:creator>
    <meta refines="#c2" property="role" scheme="marc:relators">ill</meta>
    <dc:creator opf:role="clr">Sébastien Bouët</dc:creator>
    <dc:publisher>Dargaud</dc:publisher>
    <dc:date>2008-11-14</dc:date>
    <dc:language>fr-FR</dc:language>
    <dc:identifier id="uid">urn:isbn:978-2-505-00123-4</dc:identifier>
    <dc:subject>Aventure</dc:subject>
    <dc:description>&lt;p&gt;Paris, 1940 &amp;amp; la guerre.&lt;/p&gt;</dc:description>
    <meta name="calibre:series" content="L'Héritage du diable"/>
    <meta name="calibre:series_index" content="2.0"/>
  </metadata>
  <manifest>
    <item id="cover" href="Images/cover.png" media-type="image/png" properties="cover-image"/>
    <item id="p1" href="Text/page%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="p2" href="Text/page2.xhtml" media-type="application/xhtml+xml"/>
    <item id="i1" href="Images/p1.png" media-type="image/png"/>
    <item id="i2" href="Images/p2.png" media-type="image/png"/>
  </manifest>
  <spine><itemref idref="p1"/><itemref idref="p2"/></spine>
</package>"##
                .as_bytes(),
        );
        add("OEBPS/Text/page 1.xhtml", br#"<html><body><img src="../Images/p1.png" alt=""/></body></html>"#);
        add(
            "OEBPS/Text/page2.xhtml",
            br#"<html><body><svg><image width="4" height="6" xlink:href="../Images/p2.png"/></svg></body></html>"#,
        );
        add("OEBPS/Images/cover.png", &png);
        add("OEBPS/Images/p1.png", &png);
        add("OEBPS/Images/p2.png", &png);
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_pages_and_metadata() {
        let mut zip = zip::ZipArchive::new(Cursor::new(sample_epub())).unwrap();
        let p = read_package(&mut zip, true).unwrap();
        assert_eq!(p.pages, ["OEBPS/Images/cover.png", "OEBPS/Images/p1.png", "OEBPS/Images/p2.png"]);
        assert_eq!(p.image_count, 3);
        let i = &p.info;
        assert_eq!((i.series.as_str(), i.number.as_str()), ("L'Héritage du diable", "2"));
        assert_eq!(i.writer, "Jean Dufaux");
        assert_eq!(i.penciller, "Félix Meynet");
        assert_eq!(i.colorist, "Sébastien Bouët");
        assert_eq!((i.year, i.month, i.day), (2008, 11, 14));
        assert_eq!(i.language_iso, "fr");
        assert_eq!(i.gtin, "9782505001234");
        assert_eq!(i.genre, "Aventure");
        assert_eq!(i.summary, "Paris, 1940 & la guerre.");
    }

    #[test]
    fn resolves_paths() {
        assert_eq!(resolve("OEBPS/Text/", "../Images/a%20b.jpg#x"), "OEBPS/Images/a b.jpg");
        assert_eq!(resolve("", "/img/c.png"), "img/c.png");
    }
}
