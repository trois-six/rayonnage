//! Reading and writing of book files: CBZ/ZIP, CBR/RAR, CB7/7Z, CBT/TAR, PDF, DjVu.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;

use anyhow::{Context, Result, anyhow, bail};

use crate::comicinfo;
use crate::model::ComicInfo;
use crate::util::{extension_lower, natural_cmp};

/// Extensions of the book files discovered by the library scan.
pub const SOURCE_EXTENSIONS: &[&str] = &[
    "cbz", "zip", "cbr", "rar", "cb7", "7z", "cbt", "tar", "pdf", "djvu", "djv", "epub",
];

/// Page image types recognised inside archives (same list as ComicRack CE).
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jif", "jiff", "gif", "png", "tif", "tiff", "bmp", "djvu", "webp", "heic",
    "heif", "avif", "jp2", "j2k", "jxl",
];

/// Paths of the external tools, set from the settings.
#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub rar: String,
    pub ddjvu: String,
    pub djvused: String,
    pub pdftoppm: String,
}

static TOOLS: RwLock<Tools> = RwLock::new(Tools {
    rar: String::new(),
    ddjvu: String::new(),
    djvused: String::new(),
    pdftoppm: String::new(),
});

pub fn set_tools(t: Tools) {
    *TOOLS.write().unwrap() = t;
}

fn tools() -> Tools {
    TOOLS.read().unwrap().clone()
}

pub fn rar_program() -> Option<PathBuf> {
    crate::util::find_program(&tools().rar, &["rar", "Rar"])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookFormat {
    Zip,
    Rar,
    SevenZip,
    Tar,
    Pdf,
    Djvu,
    Epub,
}

impl BookFormat {
    /// Format from the file extension.
    pub fn from_path(path: &Path) -> Option<BookFormat> {
        Some(match extension_lower(path).as_str() {
            "cbz" | "zip" => BookFormat::Zip,
            "cbr" | "rar" => BookFormat::Rar,
            "cb7" | "7z" => BookFormat::SevenZip,
            "cbt" | "tar" => BookFormat::Tar,
            "pdf" => BookFormat::Pdf,
            "djvu" | "djv" => BookFormat::Djvu,
            "epub" => BookFormat::Epub,
            _ => return None,
        })
    }

    /// Actual format from the magic bytes (ComicRack also tolerates misnamed
    /// files, e.g. a ZIP renamed to .cbr), falling back to the extension.
    pub fn detect(path: &Path) -> Option<BookFormat> {
        let mut head = [0u8; 512];
        let n = File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
        let head = &head[..n];
        if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
            // An EPUB is a ZIP whose first entry is "mimetype".
            if head.get(30..38) == Some(b"mimetype") || extension_lower(path) == "epub" {
                Some(BookFormat::Epub)
            } else {
                Some(BookFormat::Zip)
            }
        } else if head.starts_with(b"Rar!\x1a\x07") {
            Some(BookFormat::Rar)
        } else if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
            Some(BookFormat::SevenZip)
        } else if head.starts_with(b"%PDF") {
            Some(BookFormat::Pdf)
        } else if head.starts_with(b"AT&TFORM") {
            Some(BookFormat::Djvu)
        } else if n >= 262 && &head[257..262] == b"ustar" {
            Some(BookFormat::Tar)
        } else {
            BookFormat::from_path(path)
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BookFormat::Zip => "eComic (ZIP)",
            BookFormat::Rar => "eComic (RAR)",
            BookFormat::SevenZip => "eComic (7z)",
            BookFormat::Tar => "eComic (TAR)",
            BookFormat::Pdf => t!("PDF Document"),
            BookFormat::Djvu => t!("DjVu Document"),
            BookFormat::Epub => t!("EPUB Document"),
        }
    }

    /// Whether ComicInfo.xml can be written into this kind of file.
    pub fn can_write_info(self) -> bool {
        match self {
            BookFormat::Zip | BookFormat::SevenZip | BookFormat::Tar => true,
            BookFormat::Rar => rar_program().is_some(),
            BookFormat::Pdf | BookFormat::Djvu | BookFormat::Epub => false,
        }
    }
}

pub fn is_book_file(path: &Path) -> bool {
    SOURCE_EXTENSIONS.contains(&extension_lower(path).as_str())
}

pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("__macosx/") || lower.contains(".ds_store") {
        return false;
    }
    Path::new(&lower)
        .extension()
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_string_lossy().as_ref()))
        .unwrap_or(false)
}

fn is_comicinfo_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("ComicInfo.xml")
}

fn sort_pages(names: &mut [String]) {
    names.sort_by(|a, b| natural_cmp(a, b));
}

/// A page image extracted from a book.
#[derive(Clone, Debug)]
pub struct Page {
    pub name: String,
    pub data: Vec<u8>,
}

/// What a scan learns about a book file.
#[derive(Clone, Debug, Default)]
pub struct BookContent {
    pub page_count: usize,
    pub info: Option<ComicInfo>,
}

fn open_format(path: &Path) -> Result<BookFormat> {
    BookFormat::detect(path).ok_or_else(|| anyhow!(t!("Unsupported format: {0}", path.display())))
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Read + Seek over a file that fetches large chunks and keeps them.
///
/// Archive parsers do many small reads at nearby offsets (end of a ZIP,
/// 7z header, TAR headers). On a network file system (sshfs, SMB, NFS) each
/// read is a round trip: fetching 512 KiB at once turns tens of round trips
/// into one or two.
pub struct ChunkReader {
    file: File,
    len: u64,
    pos: u64,
    chunks: Vec<(u64, Vec<u8>)>,
}

const CHUNK: u64 = 64 * 1024;
const FIRST_CHUNK: u64 = 32 * 1024;
const MAX_CHUNKS: usize = 8;

impl ChunkReader {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(ChunkReader { file, len, pos: 0, chunks: Vec::new() })
    }

    fn fetch(&mut self, pos: u64) -> std::io::Result<usize> {
        use std::io::Seek;
        // Reads near the end fetch the whole tail at once (ZIP central
        // directory, 7z header, PDF trailer...).
        // The first read is small: it is usually all a scan needs.
        let chunk = if self.chunks.is_empty() { FIRST_CHUNK } else { CHUNK };
        let start = if pos + chunk >= self.len { self.len.saturating_sub(chunk) } else { pos };
        let size = (self.len - start).min(chunk) as usize;
        let mut buf = vec![0u8; size];
        self.file.seek(std::io::SeekFrom::Start(start))?;
        self.file.read_exact(&mut buf)?;
        if self.chunks.len() >= MAX_CHUNKS {
            self.chunks.remove(0);
        }
        self.chunks.push((start, buf));
        Ok(self.chunks.len() - 1)
    }
}

impl Read for ChunkReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len || out.is_empty() {
            return Ok(0);
        }
        let idx = match self
            .chunks
            .iter()
            .position(|(s, d)| self.pos >= *s && self.pos < *s + d.len() as u64)
        {
            Some(i) => i,
            None => {
                // Big sequential reads (page data) bypass the cache.
                if out.len() as u64 >= CHUNK {
                    use std::io::Seek;
                    self.file.seek(std::io::SeekFrom::Start(self.pos))?;
                    let n = self.file.read(out)?;
                    self.pos += n as u64;
                    return Ok(n);
                }
                self.fetch(self.pos)?
            }
        };
        let (start, data) = &self.chunks[idx];
        let off = (self.pos - start) as usize;
        let n = out.len().min(data.len() - off);
        out[..n].copy_from_slice(&data[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for ChunkReader {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        let p = match to {
            std::io::SeekFrom::Start(p) => p as i128,
            std::io::SeekFrom::End(d) => self.len as i128 + d as i128,
            std::io::SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if p < 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, t!("Seek before the start of the file")));
        }
        self.pos = p as u64;
        Ok(self.pos)
    }
}

/// Reads the page count and the embedded ComicInfo.xml (or a sidecar XML).
#[cfg(test)]
pub fn read_content(path: &Path) -> Result<BookContent> {
    read_content_with(path, None)
}

/// Like `read_content`; `existing` lists the files known to exist (from a
/// folder scan), which avoids probing for sidecar XML files one by one.
pub fn read_content_with(path: &Path, existing: Option<&std::collections::HashSet<PathBuf>>) -> Result<BookContent> {
    // Trust the extension first (saves reading the file header); on failure
    // check the real format from the magic bytes, like ComicRack.
    let mut content = match BookFormat::from_path(path).map(|f| read_content_as(path, f)) {
        Some(Ok(c)) => c,
        first => {
            let real = BookFormat::detect(path)
                .ok_or_else(|| anyhow!(t!("Unsupported format: {0}", path.display())))?;
            match first {
                Some(Err(e)) if Some(real) == BookFormat::from_path(path) => return Err(e),
                _ => read_content_as(path, real)?,
            }
        }
    };
    if content.info.is_none() {
        content.info = read_sidecar(path, existing);
    }
    Ok(content)
}

fn read_content_as(path: &Path, fmt: BookFormat) -> Result<BookContent> {
    Ok(match fmt {
        BookFormat::Zip => {
            // Names come from the central directory: no read per entry.
            let mut zip = zip::ZipArchive::new(ChunkReader::open(path)?)?;
            let mut pages = 0;
            let mut info_name = None;
            for name in zip.file_names_raw() {
                let name = String::from_utf8_lossy(name);
                if is_image_name(&name) {
                    pages += 1;
                } else if is_comicinfo_name(&name) {
                    info_name = Some(name.into_owned());
                }
            }
            let info = match info_name.and_then(|n| zip.index_for_name(&n)) {
                Some(i) => {
                    let mut s = String::new();
                    zip.by_index(i)?.read_to_string(&mut s)?;
                    comicinfo::parse(&s).ok()
                }
                None => None,
            };
            BookContent { page_count: pages, info }
        }
        BookFormat::Rar => {
            if let Some(content) = rar_content_single_pass(path)? {
                return Ok(content);
            }
            let names = rar_list(path)?;
            let pages = names.iter().filter(|n| is_image_name(n)).count();
            let info = if names.iter().any(|n| is_comicinfo_name(n)) {
                rar_read_first(path, &|n| is_comicinfo_name(n))?
                    .and_then(|p| comicinfo::parse(&String::from_utf8_lossy(&p.data)).ok())
            } else {
                None
            };
            BookContent { page_count: pages, info }
        }
        BookFormat::SevenZip => {
            let mut r = sevenz_rust2::ArchiveReader::new(ChunkReader::open(path)?, sevenz_rust2::Password::empty())
                .map_err(|e| anyhow!("{e}"))?;
            let names: Vec<String> = r
                .archive()
                .files
                .iter()
                .filter(|f| !f.is_directory)
                .map(|f| f.name.clone())
                .collect();
            let pages = names.iter().filter(|n| is_image_name(n)).count();
            let info = match names.iter().find(|n| is_comicinfo_name(n)) {
                Some(n) => r
                    .read_file(n)
                    .ok()
                    .and_then(|d| comicinfo::parse(&String::from_utf8_lossy(&d)).ok()),
                None => None,
            };
            BookContent { page_count: pages, info }
        }
        BookFormat::Tar => {
            let mut pages = 0;
            let mut info = None;
            let mut ar = tar::Archive::new(ChunkReader::open(path)?);
            // With Seek, the page data is skipped instead of being read.
            for e in ar.entries_with_seek()? {
                let mut e = e?;
                let name = e.path()?.to_string_lossy().into_owned();
                if is_image_name(&name) {
                    pages += 1;
                } else if is_comicinfo_name(&name) {
                    let mut s = String::new();
                    e.read_to_string(&mut s)?;
                    info = comicinfo::parse(&s).ok();
                }
            }
            BookContent { page_count: pages, info }
        }
        BookFormat::Pdf => BookContent {
            page_count: pdf_page_count(path)?,
            info: None,
        },
        BookFormat::Djvu => BookContent {
            page_count: djvu_page_count(path).unwrap_or(0),
            info: None,
        },
        BookFormat::Epub => {
            // Only container.xml and the OPF are read (usually at the start
            // of the archive); the page count is the number of images.
            let mut zip = zip::ZipArchive::new(ChunkReader::open(path)?)?;
            let package = crate::epub::read_package(&mut zip, false)?;
            let info_name = zip
                .file_names_raw()
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .find(|n| is_comicinfo_name(n));
            let embedded = info_name.and_then(|n| {
                    let mut s = String::new();
                    zip.by_name(&n).ok()?.read_to_string(&mut s).ok()?;
                    comicinfo::parse(&s).ok()
                });
            BookContent {
                page_count: package.image_count,
                info: Some(embedded.unwrap_or(package.info)),
            }
        }
    })
}

/// Page count of a PDF. The beginning and the end of the file usually hold
/// the answer (linearization dictionary, root /Pages object): only when they
/// don't is the whole document parsed.
fn pdf_page_count(path: &Path) -> Result<usize> {
    use std::io::Seek;
    use std::sync::LazyLock;
    static LINEARIZED: LazyLock<regex::bytes::Regex> =
        LazyLock::new(|| regex::bytes::Regex::new(r"/Linearized\s[^>]*?/N\s+(\d+)").unwrap());
    static PAGES: LazyLock<regex::bytes::Regex> =
        LazyLock::new(|| regex::bytes::Regex::new(r"<<((?:[^<>]|<<[^<>]*>>)*?/Type\s*/Pages\b(?:[^<>]|<<[^<>]*>>)*?)>>").unwrap());
    static COUNT: LazyLock<regex::bytes::Regex> =
        LazyLock::new(|| regex::bytes::Regex::new(r"/Count\s+(\d+)").unwrap());
    let mut r = ChunkReader::open(path)?;
    let len = r.len;
    let mut head = vec![0u8; len.min(CHUNK) as usize];
    r.read_exact(&mut head)?;
    let parse = |m: &[u8]| std::str::from_utf8(m).ok().and_then(|s| s.parse::<usize>().ok());
    if let Some(n) = LINEARIZED.captures(&head).and_then(|c| parse(&c[1])) {
        return Ok(n);
    }
    let mut tail = Vec::new();
    if len > CHUNK {
        r.seek(std::io::SeekFrom::Start(len - CHUNK))?;
        r.read_to_end(&mut tail)?;
    }
    // The root of the page tree is the /Pages node without /Parent.
    let root = [&head, &tail].into_iter().find_map(|buf| {
        PAGES
            .captures_iter(buf)
            .filter(|c| !c[1].windows(7).any(|w| w == b"/Parent"))
            .find_map(|c| COUNT.captures(&c[1]).and_then(|n| parse(&n[1])))
    });
    if let Some(n) = root {
        return Ok(n);
    }
    let meta = lopdf::Document::load_metadata(path).map_err(|e| anyhow!(t!("Unreadable PDF: {0}", e)))?;
    Ok(meta.page_count as usize)
}

/// ComicRack also reads `<file>.xml` or `<name>.xml` next to the book.
fn read_sidecar(path: &Path, existing: Option<&std::collections::HashSet<PathBuf>>) -> Option<ComicInfo> {
    let mut with_ext = path.as_os_str().to_owned();
    with_ext.push(".xml");
    [PathBuf::from(with_ext), path.with_extension("xml")]
        .iter()
        .filter(|p| existing.map_or_else(|| p.is_file(), |e| e.contains(*p)))
        .find_map(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| comicinfo::parse(&s).ok())
}

/// Reads all the pages of a book, in reading order.
pub fn read_pages(path: &Path) -> Result<Vec<Page>> {
    read_pages_filtered(path, None)
}

/// Reads the first page (the cover).
pub fn read_cover(path: &Path) -> Result<Page> {
    read_page(path, 0)
}

/// Reads the image number `index` of the file (file order).
pub fn read_page(path: &Path, index: usize) -> Result<Page> {
    read_pages_filtered(path, Some(index + 1))?
        .into_iter()
        .nth(index)
        .ok_or_else(|| anyhow!(t!("No page found")))
}

fn read_pages_filtered(path: &Path, limit: Option<usize>) -> Result<Vec<Page>> {
    let fmt = open_format(path)?;
    let mut pages = match fmt {
        BookFormat::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;
            let mut names: Vec<String> = zip
                .file_names()
                .filter_map(|n| n.ok().map(|n| n.to_string()))
                .filter(|n| is_image_name(n))
                .collect();
            sort_pages(&mut names);
            if let Some(l) = limit {
                names.truncate(l);
            }
            let mut out = Vec::with_capacity(names.len());
            for n in names {
                let mut f = zip.by_name(&n)?;
                let mut data = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut data)?;
                out.push(Page { name: n, data });
            }
            out
        }
        BookFormat::Rar => {
            let wanted: Option<std::collections::HashSet<String>> = limit.map(|l| {
                let mut names: Vec<String> = rar_list(path)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|n| is_image_name(n))
                    .collect();
                sort_pages(&mut names);
                names.into_iter().take(l).collect()
            });
            rar_read(path, &|n| {
                is_image_name(n) && wanted.as_ref().is_none_or(|w| w.contains(n))
            })?
        }
        BookFormat::SevenZip => {
            let mut r = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
                .map_err(|e| anyhow!("{e}"))?;
            let mut out = Vec::new();
            if let Some(l) = limit {
                let mut names: Vec<String> = r
                    .archive()
                    .files
                    .iter()
                    .filter(|f| !f.is_directory && is_image_name(&f.name))
                    .map(|f| f.name.clone())
                    .collect();
                sort_pages(&mut names);
                for n in names.into_iter().take(l) {
                    let data = r.read_file(&n).map_err(|e| anyhow!("{e}"))?;
                    out.push(Page { name: n, data });
                }
            } else {
                r.for_each_entries(|entry, reader| {
                    if !entry.is_directory && is_image_name(&entry.name) {
                        let mut data = Vec::with_capacity(entry.size as usize);
                        reader.read_to_end(&mut data)?;
                        out.push(Page {
                            name: entry.name.clone(),
                            data,
                        });
                    } else {
                        std::io::copy(reader, &mut std::io::sink())?;
                    }
                    Ok(true)
                })
                .map_err(|e| anyhow!("{e}"))?;
            }
            out
        }
        BookFormat::Tar => {
            let mut out = Vec::new();
            let mut ar = tar::Archive::new(BufReader::new(File::open(path)?));
            for e in ar.entries()? {
                let mut e = e?;
                let name = e.path()?.to_string_lossy().into_owned();
                if is_image_name(&name) {
                    let mut data = Vec::new();
                    e.read_to_end(&mut data)?;
                    out.push(Page { name, data });
                }
            }
            out
        }
        BookFormat::Pdf => pdf_pages(path, limit)?,
        BookFormat::Djvu => djvu_pages(path, limit)?,
        BookFormat::Epub => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;
            let mut names = crate::epub::read_package(&mut zip, true)?.pages;
            if names.is_empty() {
                // No image in the spine: every image of the archive.
                names = zip
                    .file_names_raw()
                    .map(|n| String::from_utf8_lossy(n).into_owned())
                    .filter(|n| is_image_name(n))
                    .collect();
                sort_pages(&mut names);
            }
            if let Some(l) = limit {
                names.truncate(l);
            }
            let mut out = Vec::with_capacity(names.len());
            for n in names {
                let mut f = zip.by_name(&n)?;
                let mut data = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut data)?;
                out.push(Page { name: n, data });
            }
            out
        }
    };
    // Reading order: given by the document for PDF, DjVu and EPUB.
    if !matches!(fmt, BookFormat::Pdf | BookFormat::Djvu | BookFormat::Epub) {
        pages.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    }
    if let Some(l) = limit {
        pages.truncate(l);
    }
    Ok(pages)
}

fn rar_list(path: &Path) -> Result<Vec<String>> {
    let archive = unrar::Archive::new(path)
        .open_for_listing()
        .map_err(|e| anyhow!(t!("Unreadable RAR: {0}", e)))?;
    let mut names = Vec::new();
    for entry in archive {
        let entry = entry.map_err(|e| anyhow!("{e}"))?;
        if entry.is_file() {
            names.push(entry.filename.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(names)
}

/// Page count and ComicInfo.xml of a non-solid RAR in a single pass over the
/// archive (skipping an entry is just a seek). Returns `None` for a solid
/// archive, where skipping entries would mean decompressing them: there the
/// headers are listed first, and the archive is only processed up to
/// ComicInfo.xml when it exists.
fn rar_content_single_pass(path: &Path) -> Result<Option<BookContent>> {
    let mut archive = unrar::Archive::new(path)
        .open_for_processing()
        .map_err(|e| anyhow!(t!("Unreadable RAR: {0}", e)))?;
    if archive.is_solid() {
        return Ok(None);
    }
    let mut content = BookContent::default();
    while let Some(header) = archive.read_header().map_err(|e| anyhow!("{e}"))? {
        let name = header.entry().filename.to_string_lossy().replace('\\', "/");
        let is_file = header.entry().is_file();
        archive = if is_file && is_comicinfo_name(&name) && content.info.is_none() {
            let (data, next) = header.read().map_err(|e| anyhow!("{e}"))?;
            content.info = comicinfo::parse(&String::from_utf8_lossy(&data)).ok();
            next
        } else {
            if is_file && is_image_name(&name) {
                content.page_count += 1;
            }
            header.skip().map_err(|e| anyhow!("{e}"))?
        };
    }
    Ok(Some(content))
}

/// First entry matching `want`, stopping there (no need to go through the
/// rest of the archive).
fn rar_read_first(path: &Path, want: &dyn Fn(&str) -> bool) -> Result<Option<Page>> {
    let mut archive = unrar::Archive::new(path)
        .open_for_processing()
        .map_err(|e| anyhow!(t!("Unreadable RAR: {0}", e)))?;
    while let Some(header) = archive.read_header().map_err(|e| anyhow!("{e}"))? {
        let name = header.entry().filename.to_string_lossy().replace('\\', "/");
        if header.entry().is_file() && want(&name) {
            let (data, _) = header.read().map_err(|e| anyhow!("{e}"))?;
            return Ok(Some(Page { name, data }));
        }
        archive = header.skip().map_err(|e| anyhow!("{e}"))?;
    }
    Ok(None)
}

fn rar_read(path: &Path, want: &dyn Fn(&str) -> bool) -> Result<Vec<Page>> {
    let mut archive = unrar::Archive::new(path)
        .open_for_processing()
        .map_err(|e| anyhow!(t!("Unreadable RAR: {0}", e)))?;
    let mut out = Vec::new();
    while let Some(header) = archive.read_header().map_err(|e| anyhow!("{e}"))? {
        let name = header.entry().filename.to_string_lossy().replace('\\', "/");
        archive = if header.entry().is_file() && want(&name) {
            let (data, next) = header.read().map_err(|e| anyhow!("{e}"))?;
            out.push(Page { name, data });
            next
        } else {
            header.skip().map_err(|e| anyhow!("{e}"))?
        };
    }
    Ok(out)
}

/// Extracts the page images of a PDF: the largest image of each page is
/// taken as is (JPEG, JPEG 2000) or rebuilt from raw pixels. When a page has
/// no usable image, `pdftoppm` (poppler) is used when available.
fn pdf_pages(path: &Path, limit: Option<usize>) -> Result<Vec<Page>> {
    let doc = lopdf::Document::load(path).map_err(|e| anyhow!(t!("Unreadable PDF: {0}", e)))?;
    let pages = doc.get_pages();
    let mut out = Vec::new();
    for (num, page_id) in pages.iter().take(limit.unwrap_or(usize::MAX)) {
        let extracted = doc
            .get_page_images(*page_id)
            .ok()
            .and_then(|imgs| imgs.into_iter().max_by_key(|i| i.width * i.height))
            .and_then(|img| pdf_image_bytes(&doc, &img));
        let page = match extracted {
            Some((data, ext)) => Page {
                name: format!("P{num:05}.{ext}"),
                data,
            },
            None => Page {
                name: format!("P{num:05}.png"),
                data: pdftoppm_page(path, *num)?,
            },
        };
        out.push(page);
    }
    Ok(out)
}

fn pdf_image_bytes(doc: &lopdf::Document, img: &lopdf::xobject::PdfImage) -> Option<(Vec<u8>, &'static str)> {
    let filters = img.filters.clone().unwrap_or_default();
    match filters.last().map(String::as_str) {
        Some("DCTDecode") if filters.len() == 1 => return Some((img.content.to_vec(), "jpg")),
        Some("JPXDecode") if filters.len() == 1 => return Some((img.content.to_vec(), "jp2")),
        _ => {}
    }
    let stream = doc.get_object(img.id).ok()?.as_stream().ok()?;
    let raw = if filters.is_empty() {
        stream.content.clone()
    } else if filters.iter().all(|f| f == "FlateDecode") {
        stream.decompressed_content().ok()?
    } else {
        return None;
    };
    if img.bits_per_component != Some(8) {
        return None;
    }
    let (w, h) = (img.width as u32, img.height as u32);
    let dynimg = match img.color_space.as_deref() {
        Some("DeviceRGB") => image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, raw)?),
        Some("DeviceGray") => image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, raw)?),
        _ => return None,
    };
    let mut buf = Cursor::new(Vec::new());
    dynimg.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some((buf.into_inner(), "png"))
}

fn pdftoppm_page(path: &Path, num: u32) -> Result<Vec<u8>> {
    let prog = crate::util::find_program(&tools().pdftoppm, &["pdftoppm"])
        .ok_or_else(|| anyhow!(t!("PDF page without an image: install pdftoppm (poppler) to convert it")))?;
    let out = Command::new(prog)
        .args(["-png", "-r", "150", "-singlefile", "-f", &num.to_string(), "-l", &num.to_string()])
        .arg(path)
        .arg("-")
        .output()?;
    if !out.status.success() || out.stdout.is_empty() {
        bail!(t!("pdftoppm failed: {0}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

fn djvu_page_count(path: &Path) -> Result<usize> {
    let prog = crate::util::find_program(&tools().djvused, &["djvused"])
        .ok_or_else(|| anyhow!(t!("djvused (DjVuLibre) not found")))?;
    let out = Command::new(prog).arg(path).args(["-e", "n"]).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().parse()?)
}

fn djvu_pages(path: &Path, limit: Option<usize>) -> Result<Vec<Page>> {
    let prog = crate::util::find_program(&tools().ddjvu, &["ddjvu"])
        .ok_or_else(|| anyhow!(t!("ddjvu (DjVuLibre) not found: DjVu files cannot be read")))?;
    let count = djvu_page_count(path)?.min(limit.unwrap_or(usize::MAX));
    let dir = tempfile::tempdir()?;
    let mut out = Vec::new();
    for n in 1..=count {
        let tif = dir.path().join(format!("p{n}.tif"));
        let st = Command::new(&prog)
            .args(["-format=tiff", &format!("-page={n}")])
            .arg(path)
            .arg(&tif)
            .status()?;
        if !st.success() {
            bail!(t!("ddjvu failed on page {0}", n));
        }
        out.push(Page {
            name: format!("P{n:05}.tif"),
            data: std::fs::read(&tif)?,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Target formats of the conversion ("Export") feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OutputFormat {
    Cbz,
    Cbr,
    Cb7,
    Cbt,
    Pdf,
    ImageFolder,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 6] = [
        OutputFormat::Cbz,
        OutputFormat::Cbr,
        OutputFormat::Cb7,
        OutputFormat::Cbt,
        OutputFormat::Pdf,
        OutputFormat::ImageFolder,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Cbz => "eComic (ZIP) .cbz",
            OutputFormat::Cbr => "eComic (RAR) .cbr",
            OutputFormat::Cb7 => "eComic (7z) .cb7",
            OutputFormat::Cbt => "eComic (TAR) .cbt",
            OutputFormat::Pdf => t!("PDF Document .pdf"),
            OutputFormat::ImageFolder => t!("Image folder"),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Cbz => "cbz",
            OutputFormat::Cbr => "cbr",
            OutputFormat::Cb7 => "cb7",
            OutputFormat::Cbt => "cbt",
            OutputFormat::Pdf => "pdf",
            OutputFormat::ImageFolder => "",
        }
    }

    /// Output format matching an existing book file ("same as original").
    pub fn from_book(fmt: BookFormat) -> OutputFormat {
        match fmt {
            BookFormat::Zip => OutputFormat::Cbz,
            BookFormat::Rar => OutputFormat::Cbr,
            BookFormat::SevenZip => OutputFormat::Cb7,
            BookFormat::Tar => OutputFormat::Cbt,
            BookFormat::Pdf => OutputFormat::Pdf,
            BookFormat::Djvu | BookFormat::Epub => OutputFormat::Cbz,
        }
    }
}

/// Compression level: 0 = none, 1 = medium, 2 = strong.
pub fn write_book(
    dest: &Path,
    format: OutputFormat,
    pages: &[Page],
    info: Option<&ComicInfo>,
    compression: u8,
) -> Result<()> {
    let xml = info.map(comicinfo::to_xml);
    match format {
        OutputFormat::Cbz => {
            let tmp = temp_sibling(dest);
            let mut zw = zip::ZipWriter::new(File::create(&tmp)?);
            let method = if compression == 0 {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            let level = match compression {
                0 => None,
                1 => Some(5),
                _ => Some(9),
            };
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(method)
                .compression_level(level)
                .large_file(false);
            for p in pages {
                zw.start_file(p.name.as_str(), opts)?;
                zw.write_all(&p.data)?;
            }
            if let Some(xml) = &xml {
                zw.start_file("ComicInfo.xml", opts)?;
                zw.write_all(xml.as_bytes())?;
            }
            zw.finish()?;
            std::fs::rename(&tmp, dest)?;
        }
        OutputFormat::Cb7 => {
            let tmp = temp_sibling(dest);
            let mut w = sevenz_rust2::ArchiveWriter::create(&tmp).map_err(|e| anyhow!("{e}"))?;
            let method: sevenz_rust2::EncoderConfiguration = match compression {
                0 => sevenz_rust2::EncoderMethod::COPY.into(),
                1 => sevenz_rust2::encoder_options::Lzma2Options::from_level(5).into(),
                _ => sevenz_rust2::encoder_options::Lzma2Options::from_level(9).into(),
            };
            w.set_content_methods(vec![method]);
            for p in pages {
                w.push_archive_entry(
                    sevenz_rust2::ArchiveEntry::new_file(&p.name),
                    Some(Cursor::new(&p.data)),
                )
                .map_err(|e| anyhow!("{e}"))?;
            }
            if let Some(xml) = &xml {
                w.push_archive_entry(
                    sevenz_rust2::ArchiveEntry::new_file("ComicInfo.xml"),
                    Some(Cursor::new(xml.as_bytes())),
                )
                .map_err(|e| anyhow!("{e}"))?;
            }
            w.finish()?;
            std::fs::rename(&tmp, dest)?;
        }
        OutputFormat::Cbt => {
            let tmp = temp_sibling(dest);
            let mut b = tar::Builder::new(File::create(&tmp)?);
            let mut add = |name: &str, data: &[u8]| -> Result<()> {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_mtime(chrono::Utc::now().timestamp() as u64);
                h.set_cksum();
                b.append_data(&mut h, name, data)?;
                Ok(())
            };
            for p in pages {
                add(&p.name, &p.data)?;
            }
            if let Some(xml) = &xml {
                add("ComicInfo.xml", xml.as_bytes())?;
            }
            b.into_inner()?.flush()?;
            std::fs::rename(&tmp, dest)?;
        }
        OutputFormat::Cbr => {
            let rar = rar_program().ok_or_else(|| {
                anyhow!(t!("The “rar” program (WinRAR) is required to create CBR files; set its path in the settings"))
            })?;
            let dir = tempfile::tempdir()?;
            let mut list = String::new();
            for p in pages {
                let file = dir.path().join(&p.name);
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&file, &p.data)?;
                list.push_str(&p.name);
                list.push('\n');
            }
            if let Some(xml) = &xml {
                std::fs::write(dir.path().join("ComicInfo.xml"), xml)?;
                list.push_str("ComicInfo.xml\n");
            }
            let list_file = dir.path().join("__files.lst");
            std::fs::write(&list_file, list)?;
            let tmp = temp_sibling(dest).with_extension("rar");
            let dest_abs = std::path::absolute(&tmp)?;
            let level = match compression {
                0 => "-m0",
                1 => "-m3",
                _ => "-m5",
            };
            let out = Command::new(rar)
                .current_dir(dir.path())
                .args(["a", "-idq", "-y", "-ep2", level, "-sc", "utf-8l"])
                .arg(&dest_abs)
                .arg(format!("@{}", list_file.file_name().unwrap().to_string_lossy()))
                .output()?;
            if !out.status.success() {
                let _ = std::fs::remove_file(&tmp);
                bail!(t!(
                    "rar failed: {0}",
                    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
                ));
            }
            std::fs::rename(&tmp, dest)?;
        }
        OutputFormat::Pdf => write_pdf(dest, pages)?,
        OutputFormat::ImageFolder => {
            std::fs::create_dir_all(dest)?;
            for p in pages {
                let file = dest.join(&p.name);
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(file, &p.data)?;
            }
            if let Some(xml) = &xml {
                std::fs::write(dest.join("ComicInfo.xml"), xml)?;
            }
        }
    }
    Ok(())
}

fn temp_sibling(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

/// One image per page, each page sized to its image (JPEG embedded as is).
fn write_pdf(dest: &Path, pages: &[Page]) -> Result<()> {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for p in pages {
        let is_jpeg = p.data.starts_with(&[0xFF, 0xD8]);
        let img = image::load_from_memory(&p.data).context(p.name.clone())?;
        let (w, h) = (img.width(), img.height());
        let (data, cs) = if is_jpeg && matches!(img.color(), image::ColorType::Rgb8 | image::ColorType::L8) {
            let cs = if img.color() == image::ColorType::L8 { "DeviceGray" } else { "DeviceRGB" };
            (p.data.clone(), cs)
        } else {
            let mut buf = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(img.to_rgb8())
                .write_to(&mut buf, image::ImageFormat::Jpeg)?;
            (buf.into_inner(), "DeviceRGB")
        };
        let img_stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w as i64,
                "Height" => h as i64,
                "ColorSpace" => cs,
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            data,
        );
        let img_id = doc.add_object(img_stream);
        let content = format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), (w as i64).into(), (h as i64).into()],
            "Contents" => content_id,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => img_id } },
        });
        kids.push(Object::Reference(page_id));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let tmp = temp_sibling(dest);
    doc.save(&tmp)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// Writes (or replaces) ComicInfo.xml inside an existing book file
/// ("Update Book Files" in ComicRack).
pub fn write_comic_info(path: &Path, info: &ComicInfo) -> Result<()> {
    let fmt = open_format(path)?;
    let xml = comicinfo::to_xml(info);
    match fmt {
        BookFormat::Zip if zip_update_in_place(path, xml.as_bytes())? => {}
        BookFormat::Zip => {
            let mut src = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;
            let tmp = temp_sibling(path);
            let result = (|| -> Result<()> {
                let mut zw = zip::ZipWriter::new(File::create(&tmp)?);
                for i in 0..src.len() {
                    let f = src.by_index_raw(i)?;
                    if is_comicinfo_name(&f.name()?) {
                        continue;
                    }
                    zw.raw_copy_file(f)?;
                }
                zw.start_file(
                    "ComicInfo.xml",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )?;
                zw.write_all(xml.as_bytes())?;
                zw.finish()?;
                Ok(())
            })();
            if let Err(e) = result {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            replace_file(&tmp, path)?;
        }
        BookFormat::SevenZip | BookFormat::Tar => {
            let entries = read_all_entries(path, fmt)?;
            let pages: Vec<Page> = entries
                .into_iter()
                .filter(|p| !is_comicinfo_name(&p.name))
                .collect();
            let out_fmt = if fmt == BookFormat::Tar { OutputFormat::Cbt } else { OutputFormat::Cb7 };
            let tmp = temp_sibling(path);
            write_book(&tmp, out_fmt, &pages, Some(info), 2)?;
            replace_file(&tmp, path)?;
        }
        BookFormat::Rar => {
            let rar = rar_program().ok_or_else(|| {
                anyhow!(t!("The “rar” program is required to write into a CBR file"))
            })?;
            let dir = tempfile::tempdir()?;
            std::fs::write(dir.path().join("ComicInfo.xml"), &xml)?;
            let abs = std::path::absolute(path)?;
            let out = Command::new(rar)
                .current_dir(dir.path())
                .args(["a", "-idq", "-y", "-ep"])
                .arg(&abs)
                .arg("ComicInfo.xml")
                .output()?;
            if !out.status.success() {
                bail!(t!("rar failed: {0}", String::from_utf8_lossy(&out.stderr)));
            }
        }
        BookFormat::Pdf | BookFormat::Djvu | BookFormat::Epub => {
            bail!(t!("This format cannot store the book information in the file"))
        }
    }
    Ok(())
}

fn u16_at(b: &[u8], p: usize) -> usize {
    u16::from_le_bytes([b[p], b[p + 1]]) as usize
}

fn u32_at(b: &[u8], p: usize) -> u64 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]) as u64
}

/// Adds or replaces ComicInfo.xml (Deflate, like ComicRack) in a ZIP/CBZ
/// without rewriting the pages:
/// the new entry, a new central directory (without the old ComicInfo.xml)
/// and its end record are appended at the end of the file; nothing before is
/// modified, so an interrupted update leaves the previous version readable.
/// On a network drive this writes a few KB instead of downloading and
/// uploading the whole book. Returns false when the archive is not suitable
/// (ZIP64, multi-disk...) and must be rewritten instead.
fn zip_update_in_place(path: &Path, xml: &[u8]) -> Result<bool> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    let tail_len = len.min(22 + 65535);
    let mut tail = vec![0u8; tail_len as usize];
    f.seek(SeekFrom::Start(len - tail_len))?;
    f.read_exact(&mut tail)?;
    // End of central directory record (the one whose comment ends the file).
    let Some(e) = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..i + 4] == *b"PK\x05\x06" && i + 22 + u16_at(&tail, i + 20) == tail.len())
    else {
        return Ok(false);
    };
    let zip64_locator = e >= 20 && tail[e - 20..e - 16] == *b"PK\x06\x07";
    let (disk, cd_disk) = (u16_at(&tail, e + 4), u16_at(&tail, e + 6));
    let total = u16_at(&tail, e + 10);
    let (cd_size, cd_offset) = (u32_at(&tail, e + 12), u32_at(&tail, e + 16));
    if zip64_locator || disk != 0 || cd_disk != 0 || total == 0xFFFF || cd_offset + cd_size > len {
        return Ok(false);
    }
    let comment = tail[e + 22..].to_vec();
    let mut cd = vec![0u8; cd_size as usize];
    f.seek(SeekFrom::Start(cd_offset))?;
    f.read_exact(&mut cd)?;

    // Keep every central record except the old ComicInfo.xml.
    let mut kept = Vec::with_capacity(cd.len() + 128);
    let mut count = 0usize;
    let mut p = 0;
    while p + 46 <= cd.len() && cd[p..p + 4] == *b"PK\x01\x02" {
        let n = u16_at(&cd, p + 28);
        let rec_len = 46 + n + u16_at(&cd, p + 30) + u16_at(&cd, p + 32);
        if p + rec_len > cd.len() {
            return Ok(false);
        }
        let name = String::from_utf8_lossy(&cd[p + 46..p + 46 + n]);
        if !is_comicinfo_name(&name) {
            kept.extend_from_slice(&cd[p..p + rec_len]);
            count += 1;
        }
        p += rec_len;
    }
    if p != cd.len() {
        return Ok(false);
    }

    let name = b"ComicInfo.xml";
    let crc = crc32fast::hash(xml);
    // Deflate (level 5), as the "7z u" that ComicRack uses for this update.
    let packed = {
        use flate2::write::DeflateEncoder;
        let mut enc = DeflateEncoder::new(Vec::new(), flate2::Compression::new(5));
        enc.write_all(xml)?;
        enc.finish()?
    };
    let now = chrono::Local::now();
    use chrono::{Datelike, Timelike};
    let time = ((now.hour() << 11) | (now.minute() << 5) | (now.second() / 2)) as u16;
    let date = (((now.year() - 1980).max(0) as u32) << 9 | (now.month() << 5) | now.day()) as u16;
    let local_offset = len;
    let new_cd_offset = len + 30 + name.len() as u64 + packed.len() as u64;
    if new_cd_offset + kept.len() as u64 + 46 + name.len() as u64 > u32::MAX as u64 || count + 1 >= 0xFFFF {
        return Ok(false);
    }
    let mut out = Vec::with_capacity(xml.len() + kept.len() + 256);
    // Local file header (deflated, UTF-8 name).
    out.extend_from_slice(b"PK\x03\x04");
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0x0800u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&time.to_le_bytes());
    out.extend_from_slice(&date.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    out.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&packed);
    // Central directory: kept records + the new one.
    out.extend_from_slice(&kept);
    out.extend_from_slice(b"PK\x01\x02");
    out.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0x0800u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&time.to_le_bytes());
    out.extend_from_slice(&date.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    out.extend_from_slice(&(xml.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&(0o100644u32 << 16).to_le_bytes());
    out.extend_from_slice(&(local_offset as u32).to_le_bytes());
    out.extend_from_slice(name);
    let new_cd_size = kept.len() + 46 + name.len();
    // End of central directory.
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&((count + 1) as u16).to_le_bytes());
    out.extend_from_slice(&((count + 1) as u16).to_le_bytes());
    out.extend_from_slice(&(new_cd_size as u32).to_le_bytes());
    out.extend_from_slice(&(new_cd_offset as u32).to_le_bytes());
    out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    out.extend_from_slice(&comment);
    f.seek(SeekFrom::End(0))?;
    f.write_all(&out)?;
    f.sync_all()?;
    Ok(true)
}

/// Every entry of a 7z/tar archive (not only images), in archive order.
fn read_all_entries(path: &Path, fmt: BookFormat) -> Result<Vec<Page>> {
    let mut out = Vec::new();
    match fmt {
        BookFormat::SevenZip => {
            let mut r = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
                .map_err(|e| anyhow!("{e}"))?;
            r.for_each_entries(|entry, reader| {
                if !entry.is_directory {
                    let mut data = Vec::new();
                    reader.read_to_end(&mut data)?;
                    out.push(Page {
                        name: entry.name.clone(),
                        data,
                    });
                }
                Ok(true)
            })
            .map_err(|e| anyhow!("{e}"))?;
        }
        BookFormat::Tar => {
            let mut ar = tar::Archive::new(BufReader::new(File::open(path)?));
            for e in ar.entries()? {
                let mut e = e?;
                if e.header().entry_type().is_file() {
                    let name = e.path()?.to_string_lossy().into_owned();
                    let mut data = Vec::new();
                    e.read_to_end(&mut data)?;
                    out.push(Page { name, data });
                }
            }
        }
        _ => unreachable!(),
    }
    Ok(out)
}

/// Replaces `dest` by `tmp` (both are in the same folder, so this is a rename).
fn replace_file(tmp: &Path, dest: &Path) -> Result<()> {
    if cfg!(windows) && dest.exists() {
        std::fs::remove_file(dest)?;
    }
    std::fs::rename(tmp, dest).with_context(|| t!("Replacing {0}", dest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(4, 6, image::Rgb([200, 10, 10]));
        let mut c = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut c, image::ImageFormat::Png)
            .unwrap();
        c.into_inner()
    }

    #[test]
    fn write_and_read_formats() {
        let dir = tempfile::tempdir().unwrap();
        let pages = vec![
            Page { name: "p10.png".into(), data: png() },
            Page { name: "p2.png".into(), data: png() },
        ];
        let info = ComicInfo {
            series: "Test".into(),
            number: "3".into(),
            ..Default::default()
        };
        for fmt in [OutputFormat::Cbz, OutputFormat::Cb7, OutputFormat::Cbt] {
            let dest = dir.path().join(format!("book.{}", fmt.extension()));
            write_book(&dest, fmt, &pages, Some(&info), 1).unwrap();
            let c = read_content(&dest).unwrap();
            assert_eq!(c.page_count, 2, "{fmt:?}");
            assert_eq!(c.info.as_ref().unwrap().series, "Test");
            let read = read_pages(&dest).unwrap();
            assert_eq!(read[0].name, "p2.png");
            assert_eq!(read_cover(&dest).unwrap().name, "p2.png");
            let mut info2 = info.clone();
            info2.title = "Nouveau".into();
            write_comic_info(&dest, &info2).unwrap();
            let c = read_content(&dest).unwrap();
            assert_eq!(c.info.unwrap().title, "Nouveau", "{fmt:?}");
            assert_eq!(c.page_count, 2);
        }
        // CBZ updated in place several times stays a valid archive.
        let cbz = dir.path().join("book.cbz");
        for title in ["Un", "Deux", "Trois"] {
            let mut i = info.clone();
            i.title = title.into();
            write_comic_info(&cbz, &i).unwrap();
            let c = read_content(&cbz).unwrap();
            assert_eq!((c.page_count, c.info.unwrap().title.as_str()), (2, title));
            let mut z = zip::ZipArchive::new(File::open(&cbz).unwrap()).unwrap();
            assert_eq!(z.len(), 3);
            for k in 0..z.len() {
                std::io::copy(&mut z.by_index(k).unwrap(), &mut std::io::sink()).unwrap();
            }
        }
        // EPUB: pages in reading order, OPF metadata, conversion to CBZ.
        let epub = dir.path().join("book.epub");
        std::fs::write(&epub, crate::epub::tests::sample_epub()).unwrap();
        assert_eq!(BookFormat::detect(&epub), Some(BookFormat::Epub));
        let c = read_content(&epub).unwrap();
        assert_eq!(c.page_count, 3);
        assert_eq!(c.info.unwrap().penciller, "Félix Meynet");
        let names: Vec<String> = read_pages(&epub).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["OEBPS/Images/cover.png", "OEBPS/Images/p1.png", "OEBPS/Images/p2.png"]);
        assert!(write_comic_info(&epub, &info).is_err());
        let pdf = dir.path().join("book.pdf");
        write_book(&pdf, OutputFormat::Pdf, &pages, None, 1).unwrap();
        assert_eq!(read_content(&pdf).unwrap().page_count, 2);
        assert_eq!(read_pages(&pdf).unwrap().len(), 2);
    }
}

