---
type: "Product Specification"
title: "Features — What Rayonnage Does Today"
description: "The complete list of what Rayonnage does today, grouped by area, each feature in one line linked to the specification that defines it."
tags: [rayonnage, specification, product, features]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Features — What Rayonnage Does Today

> Part of the [Rayonnage documentation](index.md).
> See also: [Quickstart](quickstart.md) · [Architecture](architecture.md)

---

## 1. Scope

This is the catalogue of delivered features, one line each. Every line links
to the section of the specification that defines the feature; the
specification is authoritative, this list only points at it.

Rayonnage reproduces part of [ComicRack CE](https://github.com/maforget/ComicRackCE)
together with its [Bedetheque Scrapper 2](https://github.com/maforget/Bedetheque-Scrapper-2)
and [Library Organizer](https://github.com/Stonepaw/comicrack-library-organizer)
plugins, as native features, on Linux, Windows and macOS.

## 2. Library

- A library built from a list of folders, scanned on demand or at startup ([Library §2](library.md#2-library-folders)).
- Incremental, parallel scanning designed for network drives: a CBZ is read in one small access at its end ([Library §3](library.md#3-scanning)).
- The formats of ComicRack CE — CBZ, CBR (RAR4 and RAR5), CB7, CBT, PDF, DjVu — plus EPUB, detected from their content ([Library §4](library.md#4-formats)).
- ComicInfo.xml read from the archive or next to the book, EPUB metadata read from the OPF package ([Library §5](library.md#5-what-is-read-from-a-book)).
- Series and number proposed from the file name when they are empty, as in ComicRack ([Library §5](library.md#5-what-is-read-from-a-book)).
- ComicRack's columns, sortable, resizable and reordered by drag and drop, saved at once ([Library §6](library.md#6-the-book-list)).
- Quick search and filters by folder and by unsaved information ([Library §7](library.md#7-search-and-filters)).
- In-place editing in the list with suggestions, Tab navigation and up/down buttons on numbers ([Library §8](library.md#8-in-place-editing)).

## 3. Book information

- The Info dialog with ComicRack's tabs, for one book or several at once ([Book Information §2](book-info.md#2-the-info-dialog)).
- Every field of ComicRack CE's ComicInfo.xml, in its element order ([Book Information §1](book-info.md#1-overview)).
- A Pages tab to reorder, sort, rotate, position, bookmark and delete pages, and set their type ([Book Information §3](book-info.md#3-pages)).
- Automatic numbering of the series or the alternate series, with the total count ([Book Information §4](book-info.md#4-automatic-numbering)).

## 4. Book files

- ComicInfo.xml written into CBZ, CB7, CBT and CBR files, by hand or after each change ([Book Files §2](book-files.md#2-updating-book-files)).
- CBZ updated in place: a few kilobytes appended instead of rewriting the book ([Book Files §2.1](book-files.md#21-in-place-update-of-a-cbz)).
- Refresh from the files, filling empty values or forcing a full reread ([Book Files §3](book-files.md#3-refreshing-from-the-files)).

## 5. Conversion

- One-click conversion to CBZ, CB7 or CBR, replacing the original ([Conversion §2](conversion.md#2-presets)).
- ComicRack's Export dialog: location, naming, format, compression, page selection, image format, resizing, double pages and image processing, with saved presets ([Conversion §3](conversion.md#3-the-export-dialog)).
- Pages written in the book's order, rotated, without the deleted ones, and named like ComicRack's ([Conversion §4](conversion.md#4-pages-and-page-names)).
- Packs split into one book per album ([Conversion §5](conversion.md#5-split-packs-into-albums)).

## 6. Bedetheque

- Information fetched from bedetheque.com for the selected books, with a choice when the result is ambiguous ([Bedetheque §2](bedetheque.md#2-fetching)).
- Fetching from an album, series or magazine link, and rescraping from the books' Web field ([Bedetheque §2](bedetheque.md#2-fetching)).
- Each fetched field and the naming options set in the settings ([Bedetheque §3](bedetheque.md#3-fields-fetched), [Bedetheque §4](bedetheque.md#4-options)).

## 7. File organization

- Files moved, copied or simulated from folder and file name templates, with a live preview and undo ([File Organization §2](organizer.md#2-running)).
- Several profiles, one click away from the toolbar ([File Organization §3](organizer.md#3-profiles)).
- The Library Organizer template language: prefixes, padding, multiple values, conditions, counters and dates ([File Organization §4](organizer.md#4-templates)).

## 8. ComicRack migration

- The Bedetheque, organization, preference and export settings of a ComicRack CE installation imported from a ZIP or a folder ([ComicRack Import](comicrack-import.md)).
- Windows drives and shares mapped to local folders ([ComicRack Import §3](comicrack-import.md#3-windows-paths)).

## 9. Interface

- Background operations with progress and cancel; the window can close during one ([Interface §1](interface.md#1-overview)).
- A searchable message log of every operation, mirrored to a file ([Interface §3](interface.md#3-message-log)).
- Light, dark or system appearance ([Interface §4](interface.md#4-appearance)).
- English and French interface ([Interface §5](interface.md#5-languages)).
- Keyboard shortcuts for every main action ([Interface §6](interface.md#6-keyboard-shortcuts)).

## 10. Distribution

- Packages for Linux, Windows (with GTK included) and macOS (an application) on each release ([Quickstart §1](quickstart.md#1-download-a-release)).
- The library in a local SQLite database, the settings in a JSON file ([Quickstart §4](quickstart.md#4-where-the-data-lives)).
