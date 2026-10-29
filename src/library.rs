//! Library database (JSON file) and folder scanning.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::{Instant, SystemTime};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::archive;
use crate::model::Book;

pub fn project_dirs() -> directories::ProjectDirs {
    directories::ProjectDirs::from("org", "trois-six", "rayonnage").expect("no home directory found")
}

/// The application used to be called "books-organizer": its settings and
/// library are copied into the new folders the first time (the old folders
/// are left untouched).
pub fn migrate_old_dirs() {
    let Some(old) = directories::ProjectDirs::from("org", "trois-six", "books-organizer") else { return };
    let new = project_dirs();
    let files = [
        (old.config_dir(), new.config_dir(), "settings.json"),
        (old.data_dir(), new.data_dir(), "library.json"),
        (old.data_dir(), new.data_dir(), "organizer-undo.json"),
    ];
    for (from, to, name) in files {
        let (src, dst) = (from.join(name), to.join(name));
        if src.is_file() && !dst.exists() {
            let _ = std::fs::create_dir_all(to);
            let _ = std::fs::copy(&src, &dst);
        }
    }
}

pub fn data_dir() -> PathBuf {
    let d = project_dirs().data_dir().to_path_buf();
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn cache_dir() -> PathBuf {
    let d = project_dirs().cache_dir().to_path_buf();
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Writes a file atomically (temp file + rename).
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub books: Vec<Book>,
    pub next_id: u64,
}

impl Library {
    pub fn path() -> PathBuf {
        data_dir().join("library.json")
    }

    pub fn load() -> Library {
        let mut lib: Library = std::fs::read(Self::path())
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default();
        lib.next_id = lib.next_id.max(lib.books.iter().map(|b| b.id).max().unwrap_or(0) + 1).max(1);
        lib
    }

    pub fn save(&self) -> Result<()> {
        let data = serde_json::to_vec(self)?;
        write_atomic(&Self::path(), &data).context(t!("Saving the library"))
    }

    pub fn add(&mut self, mut book: Book) -> u64 {
        book.id = self.next_id;
        self.next_id += 1;
        let id = book.id;
        self.books.push(book);
        id
    }

    pub fn get(&self, id: u64) -> Option<&Book> {
        self.books.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Book> {
        self.books.iter_mut().find(|b| b.id == id)
    }

    pub fn remove(&mut self, ids: &[u64]) {
        self.books.retain(|b| !ids.contains(&b.id));
    }
}

/// Fills a book's metadata and page count from its file. With `force`, the
/// ComicInfo of the file replaces the library values (ComicRack "Refresh"
/// with Ctrl); otherwise only empty values are filled.
pub fn refresh_from_file(book: &mut Book, force: bool) -> Result<()> {
    book.refresh_file_stats();
    load_content(book, force, None)
}

/// Reads the metadata and page count of the book file (file stats untouched).
fn load_content(book: &mut Book, force: bool, existing: Option<&HashSet<PathBuf>>) -> Result<()> {
    let content = archive::read_content_with(&book.file_path, existing)?;
    if let Some(info) = content.info {
        if force || book.info == crate::model::ComicInfo::default() {
            book.info = info;
        } else {
            fill_empty(book, &info);
        }
    }
    if content.page_count > 0 {
        book.info.page_count = content.page_count as i32;
    }
    Ok(())
}

fn fill_empty(book: &mut Book, info: &crate::model::ComicInfo) {
    let src = Book {
        info: info.clone(),
        ..Default::default()
    };
    for f in crate::model::Field::ALL.iter().filter(|f| f.in_comicinfo()) {
        if f.get(book).is_empty() {
            let v = f.get(&src);
            if !v.is_empty() {
                f.set(book, &v);
            }
        }
    }
    if book.info.pages.is_empty() {
        book.info.pages = info.pages.clone();
    }
}

pub enum ScanEvent {
    /// Looking for book files (before reading them).
    Discovering { found: usize, folder: String },
    /// About to read file `done + 1` of `total`.
    Progress { done: usize, total: usize, current: String },
    /// A new book file (id not yet assigned).
    Added(Book),
    /// An already known book whose file changed on disk.
    Updated(Book),
    Error(String),
    Finished { missing: Vec<u64>, cancelled: bool },
}

/// A book file found by the folder walk, with the stats read from the listing.
pub struct FoundFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

/// Result of the folder walk.
#[derive(Default)]
pub struct WalkResult {
    pub books: Vec<FoundFile>,
    /// XML files (possible ComicInfo sidecars).
    pub xml: HashSet<PathBuf>,
    /// Folders successfully listed.
    pub listed: HashSet<PathBuf>,
}

enum WalkMsg {
    Book(FoundFile),
    Xml(PathBuf),
    Listed(PathBuf),
    Error(String),
}

/// Lists the book files below the given folders with `threads` folders read
/// at the same time (on a network file system every folder listing is a round
/// trip). Progress is reported about every 250 ms.
pub fn find_book_files(
    folders: &[PathBuf],
    threads: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
    send: &mut dyn FnMut(ScanEvent),
) -> WalkResult {
    let mut result = WalkResult::default();
    // (pending folders, folders being read)
    let queue: Mutex<(VecDeque<PathBuf>, usize)> = Mutex::new((VecDeque::new(), 0));
    let wake = Condvar::new();
    // Real paths of the folders reached through symbolic links, walked once.
    let visited: Mutex<HashSet<PathBuf>> = Mutex::new(HashSet::new());
    for f in folders {
        if f.is_dir() {
            queue.lock().unwrap().0.push_back(f.clone());
            if let Ok(c) = std::fs::canonicalize(f) {
                visited.lock().unwrap().insert(c);
            }
        } else {
            send(ScanEvent::Error(t!("{0}: folder not found", f.display())));
        }
    }
    let (tx, rx) = mpsc::channel::<WalkMsg>();
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            let tx = tx.clone();
            let (queue, wake, visited) = (&queue, &wake, &visited);
            scope.spawn(move || {
                loop {
                    let dir = {
                        let mut q = queue.lock().unwrap();
                        loop {
                            if cancelled() {
                                q.0.clear();
                            }
                            if let Some(d) = q.0.pop_front() {
                                q.1 += 1;
                                break Some(d);
                            }
                            if q.1 == 0 {
                                break None;
                            }
                            q = wake.wait_timeout(q, std::time::Duration::from_millis(100)).unwrap().0;
                        }
                    };
                    let Some(dir) = dir else {
                        wake.notify_all();
                        return;
                    };
                    let mut subdirs = Vec::new();
                    match std::fs::read_dir(&dir) {
                        Ok(entries) => {
                            let _ = tx.send(WalkMsg::Listed(dir.clone()));
                            for e in entries.flatten() {
                                let name = e.file_name();
                                if name.to_string_lossy().starts_with('.') {
                                    continue;
                                }
                                let path = e.path();
                                let Ok(mut ft) = e.file_type() else { continue };
                                let mut link = false;
                                if ft.is_symlink() {
                                    match std::fs::metadata(&path) {
                                        Ok(m) => {
                                            ft = m.file_type();
                                            link = true;
                                        }
                                        Err(_) => continue,
                                    }
                                }
                                if ft.is_dir() {
                                    let first = !link
                                        || std::fs::canonicalize(&path)
                                            .map(|c| visited.lock().unwrap().insert(c))
                                            .unwrap_or(false);
                                    if first {
                                        subdirs.push(path);
                                    }
                                } else if ft.is_file() {
                                    if archive::is_book_file(&path) {
                                        match e.metadata().or_else(|_| std::fs::metadata(&path)) {
                                            Ok(m) => {
                                                let _ = tx.send(WalkMsg::Book(FoundFile {
                                                    path,
                                                    size: m.len(),
                                                    modified: m.modified().ok(),
                                                    created: m.created().ok(),
                                                }));
                                            }
                                            Err(err) => {
                                                let _ = tx.send(WalkMsg::Error(t!("{0}: {1}", path.display(), err)));
                                            }
                                        }
                                    } else if crate::util::extension_lower(&path) == "xml" {
                                        let _ = tx.send(WalkMsg::Xml(path));
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(WalkMsg::Error(t!("{0}: {1}", dir.display(), err)));
                        }
                    }
                    let mut q = queue.lock().unwrap();
                    q.0.extend(subdirs);
                    q.1 -= 1;
                    drop(q);
                    wake.notify_all();
                }
            });
        }
        drop(tx);
        let mut last = Instant::now();
        let mut current = String::new();
        for msg in rx {
            match msg {
                WalkMsg::Book(f) => result.books.push(f),
                WalkMsg::Xml(p) => {
                    result.xml.insert(p);
                }
                WalkMsg::Listed(d) => {
                    current = d.display().to_string();
                    result.listed.insert(d);
                }
                WalkMsg::Error(e) => send(ScanEvent::Error(e)),
            }
            if last.elapsed().as_millis() >= 250 {
                last = Instant::now();
                send(ScanEvent::Discovering {
                    found: result.books.len(),
                    folder: t!("{0} folder(s) read — {1}", result.listed.len(), current),
                });
            }
        }
    });
    result.books.sort_by(|a, b| a.path.cmp(&b.path));
    result.books.dedup_by(|a, b| a.path == b.path);
    result
}

/// Scans the folders (worker thread). `known` holds the library books. New
/// and modified files are read by `threads` threads at the same time.
pub fn scan(
    folders: Vec<PathBuf>,
    known: Vec<Book>,
    threads: usize,
    cancelled: impl Fn() -> bool + Sync,
    mut send: impl FnMut(ScanEvent),
) {
    let walk = find_book_files(&folders, threads, &cancelled, &mut send);
    if cancelled() {
        send(ScanEvent::Finished { missing: Vec::new(), cancelled: true });
        return;
    }
    let by_path: HashMap<&Path, &Book> = known.iter().map(|b| (b.file_path.as_path(), b)).collect();
    // Only new files and files whose size or date changed are read.
    // (book, replace the library values, new book)
    let mut work: Vec<(Book, bool, bool)> = Vec::new();
    for f in &walk.books {
        let modified = f.modified.map(chrono::DateTime::<chrono::Local>::from);
        match by_path.get(f.path.as_path()) {
            Some(existing) if f.size == existing.file_size && modified == existing.file_modified => {}
            Some(existing) => {
                let mut b = (*existing).clone();
                b.file_size = f.size;
                b.file_modified = modified;
                let force = !b.info_dirty;
                work.push((b, force, false));
            }
            None => {
                let b = Book {
                    file_path: f.path.clone(),
                    file_size: f.size,
                    file_modified: modified,
                    file_created: f.created.map(chrono::DateTime::<chrono::Local>::from),
                    ..Default::default()
                };
                work.push((b, true, true));
            }
        }
    }
    let total = work.len();
    send(ScanEvent::Progress { done: 0, total, current: t!("{0} file(s) found", walk.books.len()) });

    enum Done {
        Ok(Book, bool),
        Err(Book, bool, String),
    }
    let next = AtomicUsize::new(0);
    let work_ref = &work;
    let (tx, rx) = mpsc::channel::<Done>();
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1).min(total.max(1)) {
            let tx = tx.clone();
            let (next, cancelled, xml) = (&next, &cancelled, &walk.xml);
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= work_ref.len() || cancelled() {
                        return;
                    }
                    let (mut b, force, is_new) = work_ref[i].clone();
                    let msg = match load_content(&mut b, force, Some(xml)) {
                        Ok(()) => Done::Ok(b, is_new),
                        Err(e) => {
                            let err = t!("{0}: {1}", b.file_path.display(), format!("{e:#}"));
                            Done::Err(b, is_new, err)
                        }
                    };
                    if tx.send(msg).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx);
        let mut done = 0;
        let mut last = Instant::now();
        for msg in rx {
            done += 1;
            let (b, is_new) = match msg {
                Done::Ok(b, n) => (b, n),
                Done::Err(b, n, e) => {
                    send(ScanEvent::Error(e));
                    (b, n)
                }
            };
            if last.elapsed().as_millis() >= 100 || done == total {
                last = Instant::now();
                send(ScanEvent::Progress {
                    done,
                    total,
                    current: b.file_path.display().to_string(),
                });
            }
            send(if is_new { ScanEvent::Added(b) } else { ScanEvent::Updated(b) });
        }
    });
    if cancelled() {
        send(ScanEvent::Finished { missing: Vec::new(), cancelled: true });
        return;
    }
    // Missing: in a folder that was listed (or that disappeared), and not found.
    let found: HashSet<&Path> = walk.books.iter().map(|f| f.path.as_path()).collect();
    let missing = known
        .iter()
        .filter(|b| folders.iter().any(|f| b.file_path.starts_with(f)))
        .filter(|b| !found.contains(b.file_path.as_path()))
        .filter(|b| match b.file_path.parent() {
            Some(dir) if walk.listed.contains(dir) => true,
            Some(dir) => !dir.exists(),
            None => false,
        })
        .map(|b| b.id)
        .collect();
    send(ScanEvent::Finished { missing, cancelled: false });
}

/// Cover thumbnail (PNG bytes), cached on disk by path, size and mtime.
pub fn thumbnail(book: &Book, max: u32) -> Result<Vec<u8>> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    book.file_path.hash(&mut h);
    book.file_size.hash(&mut h);
    book.file_modified.map(|d| d.timestamp()).hash(&mut h);
    max.hash(&mut h);
    // The cover is the first front cover of the page order (ComicRack).
    let cover = crate::model::cover_index(&book.info, book.info.page_count.max(1) as usize);
    cover.hash(&mut h);
    let cache = cache_dir().join("thumbnails").join(format!("{:016x}.png", h.finish()));
    if let Ok(d) = std::fs::read(&cache) {
        return Ok(d);
    }
    let page = archive::read_page(&book.file_path, cover).or_else(|_| archive::read_cover(&book.file_path))?;
    let img = image::load_from_memory(&page.data).context(t!("Unreadable cover image"))?;
    let thumb = img.thumbnail(max, max);
    let mut buf = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut buf, image::ImageFormat::Png)?;
    let data = buf.into_inner();
    let _ = write_atomic(&cache, &data);
    Ok(data)
}

#[cfg(test)]
mod bench {
    /// Full scan (walk + read) of $BENCH_DIR with $BENCH_THREADS threads, into
    /// an empty library. Read-only.
    #[test]
    #[ignore]
    fn bench_scan() {
        let dir: std::path::PathBuf = std::env::var("BENCH_DIR").unwrap().into();
        let threads = std::env::var("BENCH_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
        let t = std::time::Instant::now();
        let (mut added, mut errors, mut walk_done) = (0, 0, None);
        super::scan(vec![dir], Vec::new(), threads, || false, |ev| match ev {
            super::ScanEvent::Progress { done: 0, .. } => walk_done = Some(t.elapsed()),
            super::ScanEvent::Added(_) => added += 1,
            super::ScanEvent::Error(e) => {
                errors += 1;
                eprintln!("ERR {e}");
            }
            _ => {}
        });
        eprintln!("threads={threads} walk={:.1?} total={:.1?} added={added} errors={errors}", walk_done.unwrap(), t.elapsed());
    }

    /// Reads the books listed in $BENCH_LIST (one path per line, first $BENCH_N)
    /// with $BENCH_THREADS threads and prints the timings. Read-only.
    #[test]
    #[ignore]
    fn bench_read_content() {
        let list = std::fs::read_to_string(std::env::var("BENCH_LIST").unwrap()).unwrap();
        let n: usize = std::env::var("BENCH_N").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
        let files: Vec<std::path::PathBuf> = list.lines().take(n).map(Into::into).collect();
        let t = std::time::Instant::now();
        for f in &files {
            let t1 = std::time::Instant::now();
            let r = crate::archive::read_content(f);
            eprintln!("{:>6.2}s {:?} {}", t1.elapsed().as_secs_f64(), r.as_ref().map(|c| (c.page_count, c.info.is_some())).map_err(|e| e.to_string()), f.display());
        }
        eprintln!("TOTAL {} files in {:.2}s", files.len(), t.elapsed().as_secs_f64());
    }
}
