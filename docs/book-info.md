---
type: "Feature Specification"
title: "Book Information"
description: "The Info dialog for one or several books, its tabs, the Pages tab that reorders, rotates and deletes pages, and automatic numbering."
tags: [rayonnage, specification, comicinfo, editing, pages, numbering]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Book Information

> Part of the [Rayonnage documentation](index.md).
> See also: [Library §8](library.md#8-in-place-editing) · [Book Files](book-files.md) · [Bedetheque](bedetheque.md)

---

## 1. Overview

A book's information is the content of its ComicInfo.xml: every field
ComicRack CE knows, in the same element order. It is edited in the list
([Library §8](library.md#8-in-place-editing)), in the Info dialog (§2), by
automatic numbering (§4), or fetched from Bedetheque
([Bedetheque](bedetheque.md)). Edits are kept in the library; they reach the
book file when it is updated ([Book Files](book-files.md)).

## 2. The Info dialog

Books → Edit Info… (Ctrl+I) opens the Info dialog, modelled on ComicRack's,
for the selected books.

| Tab | Content |
|---|---|
| Summary | Cover, file type, location, page count, size, added and modified dates, community and personal ratings, web link. |
| Details | Series, number, count, volume, alternate series, story arc, series group, dates, publisher, imprint, creators (writer, penciller, inker, colorist, letterer, cover artist, editor, translator), genre, tags, language, format, age rating, manga, black and white. |
| Plot & Notes | Summary, notes, review, main character or team, characters, teams, locations, scan information. |
| Catalog | ISBN, GTIN, location, price, age, collection status, condition, store, owner, notes, release, added and opened dates. |
| Custom | Free name and value pairs; the names offered are set in Settings → Custom. |
| Pages | The page list (§3); with one book only. |

- With several books selected, the title shows their number, fields whose
  values differ show "(multiple values)", and only the fields the user
  changes are applied to every book.
- Text fields offer suggestions taken from the values already present in
  the library.
- "◀ Previous" and "Next ▶" move to the previous and next book of the list
  without closing the dialog.

## 3. Pages

The Pages tab, as in ComicRack, lists the pages of the book with a
thumbnail, their type, image size, dimensions and file name, beside a
preview of the selected page.

- **Order**: pages are reordered by drag and drop, or with Move to Top and
  Move to Bottom. The page order of the book is the order of the `<Page>`
  elements of ComicInfo.xml.
- **Sorting**: Reset original Order, Order by Name, Order by Name (numeric).
- **Page type**: Front Cover, Inner Cover, Roundup, Story, Advertisement,
  Editorial, Letters, Preview, Back Cover, Other, Deleted. The page marked
  Front Cover is the cover shown in the library.
- **Rotation**: none, 90°, 180°, 270°.
- **Position**: Default, Near, Far (how a double-page viewer places it).
- **Bookmark**: Set Bookmark…, Remove Bookmark.
- **Deletion**: Mark as Deleted (Delete) gives the page the Deleted type;
  the file keeps it.
- **Selection**: Select All, Invert Selection; Copy Page copies the image to
  the clipboard.

The changes are stored in ComicInfo.xml when the book is updated. The images
are never changed in place: conversions follow the order, apply the
rotations and drop the deleted pages
([Conversion §4](conversion.md#4-pages-and-page-names)).

## 4. Automatic numbering

Books → Automatic Numbering… (Ctrl+Shift+N) reproduces ComicRack's
"Autonumber Wizard" on the selected books, in the order of the list.

| Option | Effect |
|---|---|
| Mode | Number the series, or number the alternate series. |
| Start at number | The number of the first book; by default, the current number of the first book. |
| Set the total count | Also stores the count (or alternate count); by default the start plus the number of books, minus one. |

The log records the number of books numbered.
