//! Book conversion/export (ComicRack "Export Books" / "Convert to CBZ/CB7").

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, GenericImageView, ImageFormat};
use serde::{Deserialize, Serialize};

use crate::archive::{self, BookFormat, OutputFormat, Page};
use crate::model::{Book, Manga, PageInfo};
use crate::util::split_list;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    /// Selected folder.
    NewFolder,
    /// Same folder as the original book.
    SameAsSource,
    /// Same folder, and the new file replaces the original in the library.
    ReplaceSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Naming {
    Filename,
    Caption,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageFormat {
    Original,
    Jpeg,
    Png,
    Webp,
    Gif,
    Tiff,
    Bmp,
}

impl PageFormat {
    pub const ALL: [PageFormat; 7] = [
        PageFormat::Original,
        PageFormat::Jpeg,
        PageFormat::Png,
        PageFormat::Webp,
        PageFormat::Gif,
        PageFormat::Tiff,
        PageFormat::Bmp,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PageFormat::Original => t!("Same as original"),
            PageFormat::Jpeg => "JPEG",
            PageFormat::Png => "PNG",
            PageFormat::Webp => t!("WebP (lossless)"),
            PageFormat::Gif => "GIF",
            PageFormat::Tiff => "TIFF",
            PageFormat::Bmp => "BMP",
        }
    }
    fn image_format(self) -> Option<(ImageFormat, &'static str)> {
        Some(match self {
            PageFormat::Original => return None,
            PageFormat::Jpeg => (ImageFormat::Jpeg, "jpg"),
            PageFormat::Png => (ImageFormat::Png, "png"),
            PageFormat::Webp => (ImageFormat::WebP, "webp"),
            PageFormat::Gif => (ImageFormat::Gif, "gif"),
            PageFormat::Tiff => (ImageFormat::Tiff, "tif"),
            PageFormat::Bmp => (ImageFormat::Bmp, "bmp"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageResize {
    Original,
    /// Best fit in width × height.
    WidthHeight,
    Width,
    Height,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoublePages {
    Keep,
    Split,
    Rotate,
    AdaptWidth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub name: String,
    // Location
    pub target: Target,
    pub target_folder: String,
    pub delete_original: bool,
    pub add_to_library: bool,
    pub overwrite: bool,
    pub combine: bool,
    // Naming
    pub naming: Naming,
    pub custom_name: String,
    pub custom_naming_start: i32,
    // File format (None = same as original)
    pub format: Option<OutputFormat>,
    /// 0 = none, 1 = medium, 2 = strong
    pub compression: u8,
    pub embed_comic_info: bool,
    pub remove_pages: bool,
    pub remove_page_types: Vec<String>,
    /// Pages to keep, e.g. "1-5,8" (1-based, empty = all).
    pub include_pages: String,
    pub ignore_error_pages: bool,
    pub tags_to_append: String,
    // Pages
    pub page_format: PageFormat,
    pub page_quality: u8,
    pub page_resize: PageResize,
    pub page_width: u32,
    pub page_height: u32,
    pub dont_enlarge: bool,
    pub double_pages: DoublePages,
    pub keep_original_names: bool,
    // Image processing
    pub brightness: i32,
    pub contrast: i32,
    pub saturation: i32,
    pub gamma: i32,
    pub sharpen: i32,
    pub auto_contrast: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        ExportSettings {
            name: t!("Custom").into(),
            target: Target::NewFolder,
            target_folder: String::new(),
            delete_original: false,
            add_to_library: false,
            overwrite: false,
            combine: false,
            naming: Naming::Filename,
            custom_name: String::new(),
            custom_naming_start: 1,
            format: Some(OutputFormat::Cbz),
            compression: 0,
            embed_comic_info: true,
            remove_pages: true,
            remove_page_types: vec!["Deleted".into()],
            include_pages: String::new(),
            ignore_error_pages: false,
            tags_to_append: String::new(),
            page_format: PageFormat::Original,
            page_quality: 75,
            page_resize: PageResize::Original,
            page_width: 1000,
            page_height: 1000,
            dont_enlarge: true,
            double_pages: DoublePages::Keep,
            keep_original_names: false,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            gamma: 0,
            sharpen: 0,
            auto_contrast: false,
        }
    }
}

impl ExportSettings {
    /// Built-in presets ("Convert to CBZ/CB7/CBR" replace the source book).
    pub fn presets() -> Vec<ExportSettings> {
        [OutputFormat::Cbz, OutputFormat::Cb7, OutputFormat::Cbr]
            .into_iter()
            .map(|f| ExportSettings {
                name: t!("Convert to {0}", f.extension().to_uppercase()),
                target: Target::ReplaceSource,
                format: Some(f),
                ..Default::default()
            })
            .collect()
    }

    fn needs_image_processing(&self) -> bool {
        self.page_format != PageFormat::Original
            || self.page_resize != PageResize::Original
            || self.double_pages != DoublePages::Keep
            || self.brightness != 0
            || self.contrast != 0
            || self.saturation != 0
            || self.gamma != 0
            || self.sharpen != 0
            || self.auto_contrast
    }
}

/// What happened to one exported book.
#[derive(Clone, Debug)]
pub struct ExportResult {
    pub source_ids: Vec<u64>,
    pub new_path: PathBuf,
    /// New metadata of the exported file (page count/pages/tags updated).
    pub info: crate::model::ComicInfo,
    /// The library entry must now point to `new_path`.
    pub replace_in_library: bool,
    /// Original files were deleted: remove the source books from the library.
    pub deleted_sources: bool,
    /// Add the new file as a new library book.
    pub add_to_library: bool,
}

/// Parses "1-5,8,10-" into a predicate on 1-based page numbers.
fn page_ranges(spec: &str) -> Vec<(usize, usize)> {
    spec.split([',', ';'])
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            match part.split_once('-') {
                Some((a, b)) => Some((
                    a.trim().parse().unwrap_or(1),
                    b.trim().parse().unwrap_or(usize::MAX),
                )),
                None => part.parse().ok().map(|n| (n, n)),
            }
        })
        .collect()
}

fn sanitize_file_name(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | '?' | '*' | '|' | '"' | '<' | '>' => '_',
            ':' => '-',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    s.trim().trim_end_matches('.').to_string()
}

pub fn target_path(book: &Book, settings: &ExportSettings, index: usize) -> Result<PathBuf> {
    let src_fmt = BookFormat::detect(&book.file_path);
    let fmt = settings
        .format
        .unwrap_or_else(|| src_fmt.map(OutputFormat::from_book).unwrap_or(OutputFormat::Cbz));
    let dir = match settings.target {
        Target::NewFolder => {
            if settings.target_folder.trim().is_empty() {
                bail!(t!("No destination folder selected"));
            }
            PathBuf::from(settings.target_folder.trim())
        }
        Target::SameAsSource | Target::ReplaceSource => book
            .file_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    };
    let stem = match settings.naming {
        Naming::Filename => book.file_stem(),
        Naming::Caption => book.caption(),
        Naming::Custom => format!(
            "{} ({})",
            settings.custom_name,
            index as i32 + settings.custom_naming_start
        ),
    };
    let stem = sanitize_file_name(&stem);
    let stem = if stem.is_empty() { book.file_stem() } else { stem };
    Ok(match fmt {
        OutputFormat::ImageFolder => dir.join(stem),
        _ => dir.join(format!("{stem}.{}", fmt.extension())),
    })
}

fn encode(img: &DynamicImage, fmt: ImageFormat, quality: u8) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    match fmt {
        ImageFormat::Jpeg => {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
            DynamicImage::ImageRgb8(img.to_rgb8()).write_with_encoder(enc)?;
        }
        ImageFormat::WebP | ImageFormat::Gif | ImageFormat::Bmp => {
            DynamicImage::ImageRgba8(img.to_rgba8()).write_to(&mut buf, fmt)?;
        }
        _ => img.write_to(&mut buf, fmt)?,
    }
    Ok(buf.into_inner())
}

fn adjust(img: DynamicImage, s: &ExportSettings) -> DynamicImage {
    let mut img = img;
    if s.auto_contrast {
        img = auto_contrast(&img);
    }
    if s.brightness != 0 {
        img = img.brighten((s.brightness as f32 * 2.55) as i32);
    }
    if s.contrast != 0 {
        img = img.adjust_contrast(s.contrast as f32);
    }
    if s.saturation != 0 || s.gamma != 0 {
        let sat = 1.0 + s.saturation as f32 / 100.0;
        // gamma slider -100..100 → exponent 2.0..0.5
        let g = 2f32.powf(-(s.gamma as f32) / 100.0);
        let lut: Vec<u8> = (0..256)
            .map(|v| ((v as f32 / 255.0).powf(g) * 255.0).round().clamp(0.0, 255.0) as u8)
            .collect();
        let mut rgba = img.to_rgba8();
        for p in rgba.pixels_mut() {
            let [r, g, b, a] = p.0;
            let gray = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
            let mix = |c: u8| lut[(gray + (c as f32 - gray) * sat).round().clamp(0.0, 255.0) as usize];
            p.0 = [mix(r), mix(g), mix(b), a];
        }
        img = DynamicImage::ImageRgba8(rgba);
    }
    if s.sharpen > 0 {
        img = img.unsharpen(0.5 + s.sharpen as f32 / 50.0, 2);
    }
    img
}

/// Stretches each channel so that 0.5 % of the pixels are clipped at each end.
fn auto_contrast(img: &DynamicImage) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    let n = (rgba.width() * rgba.height()) as usize;
    if n == 0 {
        return img.clone();
    }
    let mut lo = [0u8; 3];
    let mut hi = [255u8; 3];
    for c in 0..3 {
        let mut hist = [0usize; 256];
        for p in rgba.pixels() {
            hist[p.0[c] as usize] += 1;
        }
        let clip = n / 200;
        let mut acc = 0;
        lo[c] = (0..256).find(|&v| { acc += hist[v]; acc > clip }).unwrap_or(0) as u8;
        acc = 0;
        hi[c] = (0..256).rev().find(|&v| { acc += hist[v]; acc > clip }).unwrap_or(255) as u8;
    }
    for p in rgba.pixels_mut() {
        for c in 0..3 {
            if hi[c] > lo[c] {
                let v = (p.0[c].saturating_sub(lo[c])) as f32 * 255.0 / (hi[c] - lo[c]) as f32;
                p.0[c] = v.clamp(0.0, 255.0) as u8;
            }
        }
    }
    DynamicImage::ImageRgba8(rgba)
}

fn resize(img: DynamicImage, s: &ExportSettings, double: bool) -> DynamicImage {
    let (w, h) = img.dimensions();
    let tw = if double && s.double_pages == DoublePages::AdaptWidth { s.page_width * 2 } else { s.page_width };
    let (nw, nh) = match s.page_resize {
        PageResize::Original => return img,
        PageResize::Width => (tw, (h as f64 * tw as f64 / w as f64).round() as u32),
        PageResize::Height => ((w as f64 * s.page_height as f64 / h as f64).round() as u32, s.page_height),
        PageResize::WidthHeight => {
            let r = (tw as f64 / w as f64).min(s.page_height as f64 / h as f64);
            ((w as f64 * r).round() as u32, (h as f64 * r).round() as u32)
        }
    };
    if nw == 0 || nh == 0 || (s.dont_enlarge && nw >= w) {
        return img;
    }
    img.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3)
}

struct OutPage {
    page: Page,
    info: PageInfo,
}

/// Selects and transforms the pages of one book.
fn process_pages(book: &Book, s: &ExportSettings, out: &mut Vec<OutPage>) -> Result<()> {
    let pages = archive::read_pages(&book.file_path)?;
    let ranges = page_ranges(&s.include_pages);
    let rtl = book.info.manga == Manga::YesAndRightToLeft;
    for (idx, page) in pages.into_iter().enumerate() {
        let pinfo = book
            .info
            .pages
            .iter()
            .find(|p| p.image as usize == idx)
            .cloned()
            .unwrap_or_else(|| PageInfo { page_type: "Story".into(), ..Default::default() });
        if s.remove_pages && s.remove_page_types.iter().any(|t| t == &pinfo.page_type) {
            continue;
        }
        if !ranges.is_empty() && !ranges.iter().any(|(a, b)| (*a..=*b).contains(&(idx + 1))) {
            continue;
        }
        let ext = Path::new(&page.name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| "jpg".into());
        if !s.needs_image_processing() {
            out.push(OutPage { page, info: pinfo });
            continue;
        }
        let img = match image::load_from_memory(&page.data) {
            Ok(i) => i,
            Err(e) => {
                if s.ignore_error_pages {
                    out.push(OutPage { page, info: pinfo });
                    continue;
                }
                bail!(t!("Unreadable page “{0}”: {1}", page.name, e));
            }
        };
        let (target_fmt, target_ext) = s
            .page_format
            .image_format()
            .or_else(|| ImageFormat::from_extension(&ext).map(|f| (f, "")))
            .unwrap_or((ImageFormat::Jpeg, "jpg"));
        let target_ext = if target_ext.is_empty() { ext.clone() } else { target_ext.to_string() };
        let double = img.width() > img.height();
        let parts: Vec<DynamicImage> = match (double, s.double_pages) {
            (true, DoublePages::Split) => {
                let half = img.width() / 2;
                let left = img.crop_imm(0, 0, half, img.height());
                let right = img.crop_imm(half, 0, img.width() - half, img.height());
                if rtl { vec![right, left] } else { vec![left, right] }
            }
            (true, DoublePages::Rotate) => vec![img.rotate90()],
            _ => vec![img],
        };
        let n = parts.len();
        let stem = Path::new(&page.name)
            .file_stem()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (k, part) in parts.into_iter().enumerate() {
            let part = adjust(resize(part, s, double && n == 1), s);
            let data = encode(&part, target_fmt, s.page_quality)?;
            let suffix = if n > 1 { format!("_{}", k + 1) } else { String::new() };
            let mut info = pinfo.clone();
            info.double_page = double && n == 1 && s.double_pages != DoublePages::Rotate;
            info.image_width = part.width() as i32;
            info.image_height = part.height() as i32;
            out.push(OutPage {
                page: Page { name: format!("{stem}{suffix}.{target_ext}"), data },
                info,
            });
        }
    }
    Ok(())
}

/// Exports one book (or several, combined into the first one's name).
pub fn export(books: &[Book], s: &ExportSettings, index: usize) -> Result<ExportResult> {
    let first = books.first().ok_or_else(|| anyhow!(t!("No book")))?;
    let fmt = s.format.unwrap_or_else(|| {
        BookFormat::detect(&first.file_path)
            .map(OutputFormat::from_book)
            .unwrap_or(OutputFormat::Cbz)
    });
    let mut pages = Vec::new();
    for b in books {
        process_pages(b, s, &mut pages)?;
    }
    if pages.is_empty() {
        bail!(t!("No page to export"));
    }
    // Page names, as ComicRack: "P00001.ext" (plus " - bookmark"), or the
    // original image name without its folder; duplicates get "_2", "_3"...
    let mut names = std::collections::HashSet::new();
    for (i, p) in pages.iter_mut().enumerate() {
        let path = Path::new(&p.page.name);
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "jpg".into());
        let original = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut stem = if s.keep_original_names && !original.is_empty() {
            original
        } else {
            format!("P{:05}", i + 1)
        };
        if !s.keep_original_names && !p.info.bookmark.is_empty() {
            stem.push_str(" - ");
            stem.push_str(&sanitize_file_name(&p.info.bookmark.chars().take(25).collect::<String>()));
        }
        let base = stem.clone();
        let mut n = 2;
        while !names.insert(stem.to_lowercase()) {
            stem = format!("{base}_{n}");
            n += 1;
        }
        let name = format!("{stem}.{ext}");
        p.page.name = name;
        p.info.image = i as i32;
        p.info.image_size = p.page.data.len() as i64;
    }

    let mut info = first.info.clone();
    info.page_count = pages.len() as i32;
    info.pages = if first.info.pages.is_empty() && !s.needs_image_processing() {
        Vec::new()
    } else {
        pages.iter().map(|p| p.info.clone()).collect()
    };
    if !s.tags_to_append.trim().is_empty() {
        let mut tags = split_list(&info.tags);
        for t in split_list(&s.tags_to_append) {
            if !tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
                tags.push(t);
            }
        }
        info.tags = tags.join(", ");
    }

    let mut dest = target_path(first, &ExportSettings { format: Some(fmt), ..s.clone() }, index)?;
    let same_file = dest == first.file_path;
    if dest.exists() && !same_file && !s.overwrite {
        if s.target == Target::ReplaceSource && BookFormat::detect(&dest).is_some() {
            bail!(t!("{0} already exists (check “Overwrite existing files”)", dest.display()));
        }
        bail!(t!("{0} already exists", dest.display()));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw: Vec<Page> = pages.into_iter().map(|p| p.page).collect();
    let embedded = s.embed_comic_info.then_some(&info);
    if same_file {
        // Rewriting the source itself: write next to it, then replace it.
        let tmp = dest.with_extension(format!("{}.new", fmt.extension()));
        archive::write_book(&tmp, fmt, &raw, embedded, s.compression)?;
        if cfg!(windows) {
            std::fs::remove_file(&dest)?;
        }
        std::fs::rename(&tmp, &dest)?;
    } else {
        if dest.exists() && fmt == OutputFormat::ImageFolder {
            std::fs::remove_dir_all(&dest)?;
        }
        archive::write_book(&dest, fmt, &raw, embedded, s.compression)?;
    }
    dest = std::path::absolute(&dest).unwrap_or(dest);

    let replace = s.target == Target::ReplaceSource && fmt != OutputFormat::ImageFolder;
    let delete_sources = s.delete_original || replace;
    if delete_sources {
        for b in books {
            if b.file_path != dest && b.file_path.is_file() {
                std::fs::remove_file(&b.file_path)?;
            }
        }
    }
    Ok(ExportResult {
        source_ids: books.iter().map(|b| b.id).collect(),
        new_path: dest,
        info,
        replace_in_library: replace,
        deleted_sources: s.delete_original && !replace,
        add_to_library: s.add_to_library && !replace && fmt != OutputFormat::ImageFolder,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_book(dir: &Path) -> Book {
        let img = image::RgbImage::from_pixel(40, 20, image::Rgb([10, 200, 10]));
        let mut c = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img).write_to(&mut c, ImageFormat::Png).unwrap();
        let pages = vec![
            Page { name: "01.png".into(), data: c.get_ref().clone() },
            Page { name: "02.png".into(), data: c.into_inner() },
        ];
        let path = dir.join("src.cbz");
        archive::write_book(&path, OutputFormat::Cbz, &pages, None, 0).unwrap();
        let mut b = Book::from_file(path);
        b.id = 7;
        b.info.series = "Série".into();
        b.info.pages = vec![PageInfo { image: 1, page_type: "Deleted".into(), ..Default::default() }];
        b
    }

    #[test]
    fn convert_replace_source() {
        let dir = tempfile::tempdir().unwrap();
        let b = make_book(dir.path());
        let mut s = ExportSettings::presets()[1].clone(); // CB7
        s.page_format = PageFormat::Jpeg;
        s.double_pages = DoublePages::Split;
        s.tags_to_append = "converti".into();
        let r = export(std::slice::from_ref(&b), &s, 0).unwrap();
        assert!(r.replace_in_library);
        assert_eq!(r.new_path.extension().unwrap(), "cb7");
        assert!(!b.file_path.exists());
        let c = archive::read_content(&r.new_path).unwrap();
        // page 2 is "Deleted", page 1 is a double page split in two
        assert_eq!(c.page_count, 2);
        let names: Vec<String> = archive::read_pages(&r.new_path).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["P00001.jpg", "P00002.jpg"]);
        let info = c.info.unwrap();
        assert_eq!(info.series, "Série");
        assert_eq!(info.tags, "converti");
    }

    #[test]
    fn ranges() {
        assert_eq!(page_ranges("1-3, 5"), vec![(1, 3), (5, 5)]);
    }
}
