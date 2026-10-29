---
type: "Architecture Specification"
title: "Technical Architecture"
description: "Technology stack, module boundaries, storage, background jobs and archive access of the application."
tags: [rayonnage, specification, architecture, rust, gtk]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Technical Architecture

> Part of the [Rayonnage documentation](index.md).
> See also: [Development](development.md) · [Features](features.md)

---

## 1. Technology Stack

| Concern | Choice |
|---|---|
| Language | Rust, edition 2024 |
| Interface | GTK 4 through gtk4-rs (GTK 4.12 API) |
| ZIP | `zip`, plus `flate2` and `crc32fast` for the in-place ComicInfo.xml update |
| RAR | `unrar` (embedded RARLAB source, reading only); the external `rar` program for writing |
| 7-Zip | `sevenz-rust2` |
| TAR | `tar` |
| PDF | `lopdf`; the external `pdftoppm` for pages that are not a single image |
| DjVu | The external `ddjvu` and `djvused` |
| Images | `image` |
| XML | `quick-xml` |
| Library storage | SQLite through `rusqlite` (bundled), rows in MessagePack (`rmp-serde`) |
| Settings | JSON through `serde_json` |
| Web | `reqwest` (blocking, with cookies) and `scraper` for bedetheque.com |

The application is a single executable with no runtime besides GTK.

## 2. Modules

One crate, `rayonnage`, split into a core that knows nothing of GTK and the
`ui` module on top of it.

| Module | Role |
|---|---|
| `model` | The `Book`: ComicInfo fields, ComicRack-only properties, pages; the `Field` table that names, reads and writes every column. |
| `comicinfo` | ComicInfo.xml reading and writing, in ComicRack CE's element order. |
| `archive` | Format detection, page lists and page reading, ComicInfo.xml writing and book file writing for every format. |
| `epub` | EPUB spine images and OPF metadata. |
| `library` | The SQLite library, the folder walk and the parallel scan, cover thumbnails. |
| `convert` | Export settings and the conversion of a book: page selection, order, names, image processing. |
| `packs` | Splitting packs into albums. |
| `autonumber` | Automatic numbering. |
| `bedetheque` | The bedetheque.com client: search, parsing, field mapping. |
| `organizer` | The template engine, the profiles, the rules and the file moves with undo. |
| `comicrack_import` | Reading and applying a ComicRack CE configuration. |
| `settings` | The settings and their JSON file. |
| `i18n` | The `t!` macro and the French catalogues. |
| `util` | Small shared helpers. |
| `ui` | The main window, the book list, actions and jobs (`ui/mod.rs`), and one file per dialog or panel. |

## 3. Key Design Decisions

- **Read little, read once.** A scan reads only new or changed books, and a
  book with as few accesses as its format allows: a CBZ with one read of
  its end (central directory and ComicInfo.xml), a RAR in one pass over its
  headers, a PDF from its beginning and end. Network drives are the target
  case.
- **ComicRack compatibility first.** Field names, ComicInfo.xml element
  order, page naming on conversion, Deflate for ComicInfo.xml, the Refresh
  rules and the plugins' options follow ComicRack CE and its plugins, so
  that a library moves between the two without surprises.
- **Book files are only changed on request.** Edits stay in the library
  until Update Book Files, or until the user turns automatic updates on.
- **Never lose the original.** Files are written to a temporary file next
  to the target, then renamed over it; a CBZ update only appends.

## 4. Storage

- `library.db` is an SQLite database in WAL mode with two tables: `meta`
  (key, value) and `books` (id, data), one row per book holding the book
  serialized in MessagePack with named fields. Default values are not
  serialized.
- The library is loaded whole at start. A save writes only the books changed
  or removed since the last one, in one transaction; saves are debounced
  after edits and forced on exit.
- `settings.json` holds the settings, without the values left at their
  defaults.
- Cover thumbnails are PNG files in the cache, keyed by path, size and
  modification date.

Locations are listed in [Quickstart §4](quickstart.md#4-where-the-data-lives).

## 5. Background Jobs

Every long operation runs through `run_job` (`ui/mod.rs`):

- the work runs in a thread and talks to the GTK thread through an
  `async_channel`: progress (`progress`, `pulse`), log lines (`log`,
  `error`), questions that wait for the user's answer (`ask`), and UI
  updates (`ui`);
- the job checks `cancelled()` between items; Cancel and closing the window
  set it;
- the result is handed back to the GTK thread, which updates the library and
  the list.

The scan uses its own pool of threads (Settings → Library → Parallel reads)
for the folder walk and the book reads.

## 6. Interface

- The book list is a `ColumnView` over a `SortListModel` and a
  `FilterListModel` of `BookObject`, a GObject wrapping a book with a
  revision property that cells bind to.
- Columns come from the `Field` table; their order, width, visibility and the
  sort are saved in the settings.
- The toolbar icons for conversion, Bedetheque and file organization are
  drawn with cairo, in the style of the symbolic theme icons; the
  application icon is installed in the icon theme at start.
- Strings are written in English with `t!` and translated at run time
  ([Development §4](development.md#4-translations)).
