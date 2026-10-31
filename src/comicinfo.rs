//! ComicInfo.xml reading and writing (ComicRack CE element order).

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::model::{ComicInfo, Field, PageInfo};

/// ComicInfo elements mapped to `Field`s, in the order ComicRack CE serializes them.
const ELEMENTS: &[(&str, Field)] = &[
    ("Title", Field::Title),
    ("Series", Field::Series),
    ("Number", Field::Number),
    ("Count", Field::Count),
    ("Volume", Field::Volume),
    ("AlternateSeries", Field::AlternateSeries),
    ("AlternateNumber", Field::AlternateNumber),
    ("StoryArc", Field::StoryArc),
    ("StoryArcNumber", Field::StoryArcNumber),
    ("SeriesGroup", Field::SeriesGroup),
    ("AlternateCount", Field::AlternateCount),
    ("Summary", Field::Summary),
    ("Notes", Field::Notes),
    ("Review", Field::Review),
    ("Year", Field::Year),
    ("Month", Field::Month),
    ("Day", Field::Day),
    ("Writer", Field::Writer),
    ("Penciller", Field::Penciller),
    ("Inker", Field::Inker),
    ("Colorist", Field::Colorist),
    ("Letterer", Field::Letterer),
    ("CoverArtist", Field::CoverArtist),
    ("Editor", Field::Editor),
    ("Translator", Field::Translator),
    ("Publisher", Field::Publisher),
    ("Imprint", Field::Imprint),
    ("Genre", Field::Genre),
    ("Web", Field::Web),
    ("PageCount", Field::PageCount),
    ("LanguageISO", Field::LanguageISO),
    ("Format", Field::Format),
    ("AgeRating", Field::AgeRating),
    ("BlackAndWhite", Field::BlackAndWhite),
    ("Manga", Field::Manga),
    ("Characters", Field::Characters),
    ("Teams", Field::Teams),
    ("MainCharacterOrTeam", Field::MainCharacterOrTeam),
    ("Locations", Field::Locations),
    ("CommunityRating", Field::CommunityRating),
    ("ScanInformation", Field::ScanInformation),
    ("Tags", Field::Tags),
    ("GTIN", Field::Gtin),
];

/// Simple text elements of ComicInfo.xml handled through a scratch `Book`.
fn set_element(info: &mut ComicInfo, name: &str, value: &str) -> bool {
    let Some((_, field)) = ELEMENTS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) else {
        return false;
    };
    let mut book = crate::model::Book {
        info: std::mem::take(info),
        ..Default::default()
    };
    field.set(&mut book, value.trim());
    *info = book.info;
    true
}

fn resolve_entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let n = name.strip_prefix('#')?;
            let code = if let Some(h) = n.strip_prefix('x').or_else(|| n.strip_prefix('X')) {
                u32::from_str_radix(h, 16).ok()?
            } else {
                n.parse().ok()?
            };
            char::from_u32(code)
        }
    }
}

fn unescape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let tail = &rest[p + 1..];
        match tail.find(';').and_then(|e| resolve_entity(&tail[..e]).map(|c| (c, e))) {
            Some((c, e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text of an XML file: UTF-8 (with or without a byte order mark), UTF-16
/// with a byte order mark, or else Latin-1 (any byte is a character): a
/// ComicInfo.xml in another encoding must not make the whole book unreadable.
pub fn decode_xml(data: &[u8]) -> String {
    let utf16 = |bytes: &[u8], to_u16: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| to_u16(*c)).collect();
        String::from_utf16_lossy(&units)
    };
    match data {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => match std::str::from_utf8(data) {
            Ok(text) => text.to_string(),
            Err(_) => data.iter().map(|&b| char::from(b)).collect(),
        },
    }
}

/// Parses a ComicInfo.xml file, whatever its encoding (see [`decode_xml`]).
pub fn parse_bytes(data: &[u8]) -> Result<ComicInfo> {
    parse(&decode_xml(data))
}

pub fn parse(xml: &str) -> Result<ComicInfo> {
    let xml = xml.trim_start_matches('\u{feff}');
    let mut reader = Reader::from_str(xml);
    let mut info = ComicInfo::default();
    let mut depth = 0usize;
    let mut current: Option<String> = None;
    let mut text = String::new();
    let mut in_pages = false;
    loop {
        match reader.read_event().context(t!("Invalid ComicInfo.xml"))? {
            Event::Start(e) => {
                depth += 1;
                let name = e.local_name().as_ref().to_string();
                // Another XML file next to a book is not its ComicInfo.
                if depth == 1 && name != "ComicInfo" {
                    anyhow::bail!(t!("Not a ComicInfo.xml file (root element: {0})", name));
                }
                if depth == 2 && name == "Pages" {
                    in_pages = true;
                } else if in_pages && name == "Page" {
                    info.pages.push(parse_page(&e));
                } else if depth == 2 {
                    current = Some(name);
                    text.clear();
                }
            }
            Event::Empty(e) => {
                let name = e.local_name().as_ref().to_string();
                if in_pages && name == "Page" {
                    info.pages.push(parse_page(&e));
                }
            }
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
                    let name = r.to_string();
                    match resolve_entity(&name) {
                        Some(c) => text.push(c),
                        None => {
                            text.push('&');
                            text.push_str(&name);
                            text.push(';');
                        }
                    }
                }
            }
            Event::End(_) => {
                if depth == 2 {
                    if let Some(name) = current.take()
                        && !set_element(&mut info, &name, &text)
                        && !text.trim().is_empty()
                    {
                        info.extra.push((name, text.trim().to_string()));
                    }
                    in_pages = false;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(info)
}

fn parse_page(e: &quick_xml::events::BytesStart) -> PageInfo {
    let mut p = PageInfo {
        page_type: "Story".into(),
        ..Default::default()
    };
    for a in e.attributes().flatten() {
        let key = a.key.local_name().as_ref().to_string();
        let v = unescape_attr(&a.value);
        match key.as_str() {
            "Image" => p.image = v.parse().unwrap_or(0),
            "Type" => p.page_type = v,
            "DoublePage" => p.double_page = v.eq_ignore_ascii_case("true"),
            "ImageSize" => p.image_size = v.parse().unwrap_or(0),
            "Key" => p.key = v,
            "Bookmark" => p.bookmark = v,
            "ImageWidth" => p.image_width = v.parse().unwrap_or(0),
            "ImageHeight" => p.image_height = v.parse().unwrap_or(0),
            "Rotation" => p.rotation = v,
            "PagePosition" => p.position = v,
            _ => {}
        }
    }
    p
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\r' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

pub fn to_xml(info: &ComicInfo) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n",
    );
    // One book for every element (the fields are read through it).
    let book = crate::model::Book { info: info.clone(), ..Default::default() };
    for (name, field) in ELEMENTS {
        let v = field.get(&book);
        if !v.is_empty() {
            s.push_str(&format!("  <{name}>{}</{name}>\n", escape(&v)));
        }
    }
    for (name, v) in &info.extra {
        if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            s.push_str(&format!("  <{name}>{}</{name}>\n", escape(v)));
        }
    }
    if !info.pages.is_empty() {
        s.push_str("  <Pages>\n");
        for p in &info.pages {
            s.push_str(&format!("    <Page Image=\"{}\"", p.image));
            if !p.page_type.is_empty() && p.page_type != "Story" {
                s.push_str(&format!(" Type=\"{}\"", escape(&p.page_type)));
            }
            if p.double_page {
                s.push_str(" DoublePage=\"true\"");
            }
            if p.image_size > 0 {
                s.push_str(&format!(" ImageSize=\"{}\"", p.image_size));
            }
            if !p.key.is_empty() {
                s.push_str(&format!(" Key=\"{}\"", escape(&p.key)));
            }
            if !p.bookmark.is_empty() {
                s.push_str(&format!(" Bookmark=\"{}\"", escape(&p.bookmark)));
            }
            if p.image_width > 0 {
                s.push_str(&format!(" ImageWidth=\"{}\"", p.image_width));
            }
            if p.image_height > 0 {
                s.push_str(&format!(" ImageHeight=\"{}\"", p.image_height));
            }
            if !p.rotation.is_empty() && p.rotation != "None" {
                s.push_str(&format!(" Rotation=\"{}\"", escape(&p.rotation)));
            }
            if !p.position.is_empty() && p.position != "Default" {
                s.push_str(&format!(" PagePosition=\"{}\"", escape(&p.position)));
            }
            s.push_str(" />\n");
        }
        s.push_str("  </Pages>\n");
    }
    s.push_str("</ComicInfo>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_encoding_and_only_comicinfo() {
        let latin1 = b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><ComicInfo><Series>H\xe9ritage</Series></ComicInfo>";
        assert_eq!(parse_bytes(latin1).unwrap().series, "Héritage");
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("<ComicInfo><Series>Été</Series></ComicInfo>".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(parse_bytes(&utf16).unwrap().series, "Été");
        assert!(parse_bytes(b"<book><title>Autre</title></book>").is_err());
    }
    use crate::model::Manga;

    #[test]
    fn roundtrip() {
        let xml = r#"<?xml version="1.0"?>
<ComicInfo>
  <Title>L'&amp;Été &lt;1&gt;</Title>
  <Series>Blacksad</Series>
  <Number>1</Number>
  <Count>7</Count>
  <Year>2000</Year>
  <Manga>YesAndRightToLeft</Manga>
  <CommunityRating>4.5</CommunityRating>
  <Foo>bar</Foo>
  <Pages><Page Image="0" Type="FrontCover" ImageWidth="800" /><Page Image="1"/></Pages>
</ComicInfo>"#;
        let info = parse(xml).unwrap();
        assert_eq!(info.title, "L'&Été <1>");
        assert_eq!(info.count, 7);
        assert_eq!(info.manga, Manga::YesAndRightToLeft);
        assert_eq!(info.community_rating, 4.5);
        assert_eq!(info.pages.len(), 2);
        assert_eq!(info.pages[0].page_type, "FrontCover");
        assert_eq!(info.extra, vec![("Foo".to_string(), "bar".to_string())]);
        let again = parse(&to_xml(&info)).unwrap();
        assert_eq!(again, info);
    }
}
