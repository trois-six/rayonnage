---
type: "Feature Specification"
title: "Book Files"
description: "Writing ComicInfo.xml into the book files, which formats can be written and how, and reading the information back from the files."
tags: [rayonnage, specification, comicinfo, update, refresh]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Book Files

> Part of the [Rayonnage documentation](index.md).
> See also: [Book Information](book-info.md) · [Library §5](library.md#5-what-is-read-from-a-book) · [Conversion](conversion.md)

---

## 1. Overview

Information edited in Rayonnage stays in the library until it is written
into the book file, as in ComicRack CE. A book whose information is not
written yet is marked ✎ in the list and appears under "Unsaved info" in the
sidebar ([Library §7](library.md#7-search-and-filters)).

Settings → Files holds:

| Setting | Effect |
|---|---|
| Allow writing the information (ComicInfo.xml) into the files | Off, book files are never modified by Update Book Files. On by default. |
| Automatically update the files after each change | Writes the files after each edit in the list, the Info dialog, numbering or a Bedetheque fetch, without Update Book Files. |

## 2. Updating book files

Books → Update Book Files (Ctrl+S) writes ComicInfo.xml into the selected
books.

| Format | How ComicInfo.xml is written |
|---|---|
| CBZ, ZIP | Appended at the end of the file (§2.1); the archive is rewritten, copying the pages as they are, only when it is not suitable (ZIP64, several disks). |
| CB7, 7Z | The archive is rewritten with the same pages. |
| CBT, TAR | The archive is rewritten with the same pages. |
| CBR, RAR | Added with the `rar` program ([Quickstart §3](quickstart.md#3-external-programs)); without it, the update fails for that book. |
| PDF, DjVu, EPUB | Not written: the information stays in the library, and the log says so. |

- As in ComicRack, pages are neither renamed nor modified. Page order,
  rotation and deletion are recorded in ComicInfo.xml only
  ([Book Information §3](book-info.md#3-pages)).
- Each file written and each error is recorded in the
  [message log](interface.md#3-message-log).

### 2.1 In-place update of a CBZ

The new ComicInfo.xml, compressed with Deflate like ComicRack's, is appended
at the end of the archive, followed by a new central directory that no
longer lists the old ComicInfo.xml. Nothing before is modified:

- a few kilobytes are written instead of the whole book, which matters on a
  network drive;
- an interrupted update leaves the previous version readable.

## 3. Refreshing from the files

Books → Refresh reads the information of the selected books from their files
again.

- **Refresh**, as in ComicRack, only fills the values that are empty in the
  library, and leaves alone the books whose changes are not written to their
  file yet. The log lists the books left alone.
- **Refresh (force reread)** replaces the library values with those of the
  files, discarding the changes that were not written.
