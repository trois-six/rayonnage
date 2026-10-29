//! Splitting "packs" (an archive holding several albums) into one book file
//! per album.
//!
//! The pack is extracted to a temporary folder. Each archive found inside
//! (cbz, cbr, zip, rar, cb7, pdf...) is an album, and so is each folder that
//! directly contains page images (with its ComicInfo.xml, if any).

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::archive::{self, BookFormat, OutputFormat, Page};
use crate::util::{extension_lower, natural_cmp};

/// Format of the albums taken out of a pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitFormat {
    /// Albums that are archives are kept as they are (.zip becomes .cbz,
    /// .rar .cbr...); image folders become CBZ.
    Keep,
    Convert(OutputFormat),
}

/// Where the albums are written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitTarget {
    /// Next to the pack.
    SameFolder,
    /// In a folder named after the pack, next to it.
    PackFolder,
    Folder(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitOptions {
    pub format: SplitFormat,
    pub target: SplitTarget,
    /// Delete the pack once every album was written.
    pub delete_pack: bool,
}

impl Default for SplitOptions {
    fn default() -> Self {
        SplitOptions {
            format: SplitFormat::Convert(OutputFormat::Cbz),
            target: SplitTarget::PackFolder,
            delete_pack: false,
        }
    }
}

/// An album found in an extracted pack.
#[derive(Debug)]
enum Album {
    /// A book file (archive, PDF...).
    File(PathBuf),
    /// A folder of page images.
    Images(PathBuf),
}

impl Album {
    fn name(&self) -> String {
        let p = match self {
            Album::File(p) => p.file_stem(),
            Album::Images(p) => p.file_name(),
        };
        p.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// Extracts every file of an archive into `dest`.
fn extract(pack: &Path, dest: &Path) -> Result<()> {
    match BookFormat::detect(pack) {
        Some(BookFormat::Zip | BookFormat::Epub) => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(pack)?))?;
            for i in 0..zip.len() {
                let mut f = zip.by_index(i)?;
                // enclosed_name refuses absolute paths and "..".
                let Some(rel) = f.enclosed_name() else { continue };
                let out = dest.join(rel);
                if f.is_dir() {
                    std::fs::create_dir_all(&out)?;
                    continue;
                }
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::io::copy(&mut f, &mut File::create(&out)?)?;
            }
        }
        Some(BookFormat::Rar) => {
            let mut archive = unrar::Archive::new(pack)
                .open_for_processing()
                .map_err(|e| anyhow!(t!("Unreadable RAR: {0}", e)))?;
            while let Some(header) = archive.read_header().map_err(|e| anyhow!("{e}"))? {
                archive = if header.entry().is_file() {
                    header.extract_with_base(dest).map_err(|e| anyhow!("{e}"))?
                } else {
                    header.skip().map_err(|e| anyhow!("{e}"))?
                };
            }
        }
        Some(BookFormat::SevenZip) => sevenz_rust2::decompress_file(pack, dest).map_err(|e| anyhow!("{e}"))?,
        Some(BookFormat::Tar) => tar::Archive::new(BufReader::new(File::open(pack)?)).unpack(dest)?,
        _ => bail!(t!("This file is not an archive")),
    }
    Ok(())
}

/// Albums of an extracted pack, in natural order of their paths.
fn find_albums(root: &Path) -> Vec<Album> {
    let mut albums = Vec::new();
    let mut image_dirs = HashSet::new();
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy();
        if name.starts_with('.') || path.components().any(|c| c.as_os_str() == "__MACOSX") {
            continue;
        }
        if archive::is_book_file(path) {
            albums.push(Album::File(path.to_path_buf()));
        } else if archive::is_image_name(&name)
            && let Some(dir) = path.parent()
        {
            image_dirs.insert(dir.to_path_buf());
        }
    }
    albums.extend(image_dirs.into_iter().map(Album::Images));
    albums.sort_by(|a, b| {
        let p = |x: &Album| match x {
            Album::File(p) | Album::Images(p) => p.to_string_lossy().into_owned(),
        };
        natural_cmp(&p(a), &p(b))
    });
    albums
}

fn sanitize(name: &str) -> String {
    let s: String = name
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

/// Destination file that does not exist yet: "name.ext", "name (2).ext"...
fn free_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{name}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{name} ({n}).{ext}"));
        n += 1;
    }
    p
}

/// Splits `pack` into one book per album; returns the files written.
/// `progress` receives (album number, album count, album name).
pub fn split(pack: &Path, options: &SplitOptions, progress: &mut dyn FnMut(usize, usize, &str)) -> Result<Vec<PathBuf>> {
    let tmp = tempfile::tempdir()?;
    extract(pack, tmp.path())?;
    let albums = find_albums(tmp.path());
    // A single folder of images is an ordinary book, not a pack.
    if albums.is_empty() || (albums.len() == 1 && matches!(albums[0], Album::Images(_))) {
        bail!(t!("{0} does not contain several albums", pack.display()));
    }
    let pack_dir = pack.parent().map(Path::to_path_buf).unwrap_or_default();
    let pack_name = pack.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dest = match &options.target {
        SplitTarget::SameFolder => pack_dir,
        SplitTarget::PackFolder => pack_dir.join(sanitize(&pack_name)),
        SplitTarget::Folder(f) => f.clone(),
    };
    std::fs::create_dir_all(&dest)?;

    let total = albums.len();
    let mut written = Vec::new();
    for (i, album) in albums.iter().enumerate() {
        let mut name = sanitize(&album.name());
        if name.is_empty() {
            name = format!("{} {}", sanitize(&pack_name), i + 1);
        }
        progress(i, total, &name);
        let out = match (album, options.format) {
            (Album::File(src), SplitFormat::Keep) => {
                let ext = match extension_lower(src).as_str() {
                    "zip" => "cbz".to_string(),
                    "rar" => "cbr".to_string(),
                    "7z" => "cb7".to_string(),
                    "tar" => "cbt".to_string(),
                    e => e.to_string(),
                };
                let out = free_path(&dest, &name, &ext);
                std::fs::copy(src, &out)?;
                out
            }
            (Album::File(src), SplitFormat::Convert(fmt)) => {
                let pages = archive::read_pages(src)?;
                let info = archive::read_content_with(src, None).ok().and_then(|c| c.info);
                let out = free_path(&dest, &name, fmt.extension());
                archive::write_book(&out, fmt, &pages, info.as_ref(), 0)?;
                out
            }
            (Album::Images(dir), format) => {
                let fmt = match format {
                    SplitFormat::Convert(f) => f,
                    SplitFormat::Keep => OutputFormat::Cbz,
                };
                let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && archive::is_image_name(&p.file_name().unwrap_or_default().to_string_lossy()))
                    .collect();
                files.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
                let pages = files
                    .iter()
                    .map(|p| {
                        Ok(Page {
                            name: p.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                            data: std::fs::read(p)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let info = std::fs::read_dir(dir)?
                    .flatten()
                    .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("ComicInfo.xml"))
                    .and_then(|e| std::fs::read_to_string(e.path()).ok())
                    .and_then(|s| crate::comicinfo::parse(&s).ok());
                let out = free_path(&dest, &name, fmt.extension());
                archive::write_book(&out, fmt, &pages, info.as_ref(), 0)?;
                out
            }
        };
        written.push(out);
    }
    if options.delete_pack {
        std::fs::remove_file(pack)?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    fn png() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(3, 4, image::Rgb([1, 2, 3]));
        let mut c = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut c, image::ImageFormat::Png).unwrap();
        c.into_inner()
    }

    #[test]
    fn splits_folders_and_nested_archives() {
        let dir = tempfile::tempdir().unwrap();
        // A nested CBZ album.
        let inner = dir.path().join("inner.cbz");
        archive::write_book(&inner, OutputFormat::Cbz, &[Page { name: "1.png".into(), data: png() }], None, 0).unwrap();
        let pack = dir.path().join("Pack Astérix.zip");
        let mut zw = zip::ZipWriter::new(File::create(&pack).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("Astérix 01/p1.png", png()),
            ("Astérix 01/p2.png", png()),
            ("Astérix 01/ComicInfo.xml", b"<ComicInfo><Series>Asterix</Series><Number>1</Number></ComicInfo>".to_vec()),
            ("Astérix 02/p1.png", png()),
            ("Astérix 03.cbz", std::fs::read(&inner).unwrap()),
        ] {
            zw.start_file(name, opts).unwrap();
            zw.write_all(&data).unwrap();
        }
        zw.finish().unwrap();

        let options = SplitOptions { delete_pack: true, ..Default::default() };
        let mut seen = Vec::new();
        let out = split(&pack, &options, &mut |_, _, n| seen.push(n.to_string())).unwrap();
        let names: Vec<String> = out.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Astérix 01.cbz", "Astérix 02.cbz", "Astérix 03.cbz"]);
        assert!(out.iter().all(|p| p.parent().unwrap().ends_with("Pack Astérix")));
        assert!(!pack.exists());
        let first = archive::read_content(&out[0]).unwrap();
        assert_eq!((first.page_count, first.info.unwrap().series.as_str()), (2, "Asterix"));
        assert_eq!(archive::read_content(&out[2]).unwrap().page_count, 1);
    }

    #[test]
    fn a_single_book_is_not_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book.cbz");
        archive::write_book(&book, OutputFormat::Cbz, &[Page { name: "1.png".into(), data: png() }], None, 0).unwrap();
        assert!(split(&book, &SplitOptions::default(), &mut |_, _, _| {}).is_err());
    }
}
