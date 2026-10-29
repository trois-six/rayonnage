---
type: "Feature Specification"
title: "Library"
description: "The library folders and their scan, the formats read, what is read from each book, the book list with its columns, sorting, filters and in-place editing."
tags: [rayonnage, specification, library, scan, formats, book-list]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Library

> Part of the [Rayonnage documentation](index.md).
> See also: [Book Information](book-info.md) · [Book Files](book-files.md) · [Architecture](architecture.md)

---

## 1. Overview

The library is the list of every book found in a set of folders, with the
information read from each book or entered by the user. It is stored in a
local database ([Architecture §4](architecture.md#4-storage)); the book files
themselves are only read, until the user writes the information back
([Book Files](book-files.md)), converts ([Conversion](conversion.md)) or
files them ([File Organization](organizer.md)).

## 2. Library folders

Settings → Library holds:

| Setting | Effect |
|---|---|
| Folders scanned for books | The list of folders the scan walks, recursively. "Add Folder…" adds one, the cross removes one. Changing the list offers to scan at once. |
| Scan the folders at startup | Runs a scan each time the application starts. |
| Remove books whose file has disappeared | During a scan, drops from the library the books whose file no longer exists. Off, they stay in the library. |
| Parallel reads during the scan | Number of folders listed and books read at the same time, 16 by default. 16 to 32 suit network drives; 4 is enough for a local disk. |

## 3. Scanning

Library → Scan Folders (F5) walks the library folders and adds the books it
finds.

- Only new books, and books whose size or modification date changed, are
  read; the others are kept as they are in the library.
- Folders are listed and books are read in parallel, by the number of
  workers set in the settings (§2).
- Scanning is designed for network drives (sshfs, SMB, NFS), where each read
  waits for the network:
  - a CBZ is read with a single small access at the end of the file: the
    central directory and ComicInfo.xml, without going through the pages;
  - a RAR that is not solid is read in one pass over its headers;
  - for a PDF, the page count is looked for at the beginning and the end of
    the file; the whole document is parsed only as a last resort.
- A progress bar and an activity indicator are shown at the bottom right of
  the window while the scan runs, and books appear in the list as they are
  found. Every book added and every error is written in the
  [message log](interface.md#3-message-log).
- A Cancel button stops the scan. Closing the window during a scan asks for
  a confirmation, then stops it; the books already read are kept.

## 4. Formats

The formats read are those of ComicRack CE, plus EPUB. The real format is
detected from the first bytes of the file, so a ZIP archive named `.cbr` is
read as a ZIP.

| Extensions | Format | Notes |
|---|---|---|
| `.cbz` `.zip` | ZIP | Stored or Deflate entries. |
| `.cbr` `.rar` | RAR | RAR4 and RAR5, read by the embedded unrar library. |
| `.cb7` `.7z` | 7-Zip | |
| `.cbt` `.tar` | TAR | |
| `.pdf` | PDF | Pages that are a single image are extracted as they are; others need `pdftoppm` ([Quickstart §3](quickstart.md#3-external-programs)). |
| `.djvu` | DjVu | Needs `ddjvu` and `djvused`. |
| `.epub` | EPUB | The pages are the images of the spine, in reading order, cover first. |

Page images are the files with one of these extensions: jpg, jpeg, jif,
jiff, gif, png, tif, tiff, bmp, webp, heic, heif, avif, jp2, j2k, jxl. They
are sorted in natural order (`2` before `10`), and `__MACOSX` folders are
ignored.

## 5. What is read from a book

- **ComicInfo.xml**, from the archive or from an XML file of the same name
  next to the book, with every field ComicRack CE knows, including the page
  list (`<Pages>`) with types, sizes, rotation, position and bookmarks.
- **EPUB metadata**, from the OPF package: title, creators with their role
  (writer, artist, colorist…), publisher, date, language, summary, subjects,
  ISBN, and the calibre or EPUB 3 series and position.
- **The page count** and **the cover**: the first page, or the page marked
  as front cover. Cover thumbnails are kept in the cache folder.
- **File information**: name, folder, size, format, creation and
  modification dates.
- **Proposed values**: when the series or the number is empty, it is guessed
  from the file name and shown greyed out, as in ComicRack.

## 6. The book list

The list shows ComicRack's columns: series, number, volume, title, release
date, writer and the other creators, publisher, imprint, genre, tags,
characters, teams, locations, notes, catalog fields (price, condition,
ISBN…), file fields (name, path, size, format, dates), and the others of
ComicInfo.xml.

- Library → Columns… chooses the visible columns.
- Columns are sorted by clicking their header, resized, and reordered by
  dragging their header. The order, widths, visibility and sort are saved
  in the settings at once.
- Rows have alternating background colors, as in ComicRack.
- The number shows the count of the series when it is known: `7 (of 24)`.
- The release date is written month/year: `3/1992`.
- A book whose information changed but is not written to its file yet is
  marked ✎ ([Book Files §2](book-files.md#2-updating-book-files)).
- The information panel, on the right, shows the cover and the main fields
  of the selected book.

## 7. Search and filters

- The search field (Ctrl+F) filters the list on the series, title, number,
  alternate series, writer, penciller, publisher, imprint, genre, tags,
  characters, story arc, year and file name.
- The sidebar lists the whole library, the books with unsaved information,
  and each library folder with its number of books; selecting one filters
  the list.

## 8. In-place editing

As in ComicRack, a cell can be edited directly in the list:

- A slow click on a cell of a selected row, or F2, edits it.
- Suggestions come from the values already present in the whole library.
- Tab and Shift+Tab move to the next and previous editable column; Up and
  Down move to the next and previous row; Enter validates; Escape cancels.
- The value applies to every selected book.
- Editing the file name renames the file.
- Numeric columns (number, volume, count, year, month, day…) of the selected
  rows have up and down buttons that increment or decrement the value.

## 9. Book actions

The context menu of the list, and the Books menu, act on the selected books:

- **Open Book** opens the file with the system's default application;
  **Open Containing Folder** opens its folder.
- **Remove from Library** (Delete) removes the books from the library after
  a confirmation. The files are kept; a later scan adds them back.
- Edit Info, Update Book Files, Refresh, Convert, Fetch Info from
  Bedetheque and Organize Files are described in their own specifications
  ([Features](features.md)).
