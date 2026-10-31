---
type: "Feature Specification"
title: "Interface"
description: "The main window, the message log, appearance, languages and keyboard shortcuts."
tags: [rayonnage, specification, ui, log, theme, i18n, shortcuts]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Interface

> Part of the [Rayonnage documentation](index.md).
> See also: [Library](library.md) · [Features](features.md)

---

## 1. Overview

Rayonnage is a GTK 4 desktop application with a single main window. Long
operations (scan, update, conversion, Bedetheque, organization) run in the
background: the window stays responsive, and each operation has its own line
at the bottom right, with its progress and a Cancel button. As in ComicRack,
operations of different kinds run at the same time: a Bedetheque search or a
conversion during a scan, for example. Two operations of the same kind, or
two that change the files of the same books (update, conversion,
organization, pack split), run one after the other: the second one is shown
"(waiting)" until its turn, and Cancel removes it from the queue.

## 2. The main window

- **Header bar**, from left to right: Scan, Edit Info, Update Book Files,
  Convert (menu of presets), Fetch Info from Bedetheque, Organize Files with
  the small arrow listing the profiles; the search field in the middle; the
  main menu and the information panel toggle on the right.
- **Sidebar**: the whole library, the books with unsaved information, and
  the library folders ([Library §7](library.md#7-search-and-filters)).
- **Book list** ([Library §6](library.md#6-the-book-list)), with its context
  menu of book actions.
- **Information panel**: cover and main fields of the selected book.
- **Status bar**: the result of the last operation, the progress of the
  current one, and the Log button.
- **Main menu**: Scan Folders, Select All, Columns…, Message Log, Import
  ComicRack Settings…, Settings…, the Books and Convert menus, About.

The size of the window, the columns and the sort are restored at the next
start.

## 3. Message log

The Message Log (Ctrl+J, or the Log button at the bottom right) details
every operation: books added, read, converted or written, values fetched,
files organized, and errors.

- Each line is timed; errors stand out, and the Log button shows their
  number.
- A search field (Ctrl+F) highlights the matches and counts them; Enter and
  Shift+Enter, or F3 and Shift+F3, go to the next and previous match.
- "Errors only" hides the other lines.
- Clear empties it; Save… writes it to a file.
- The log is mirrored to `journal.log` in the cache folder
  ([Quickstart §4](quickstart.md#4-where-the-data-lives)), emptied at each
  start.

## 4. Appearance

Settings → Library → Appearance:

- **System** follows the desktop's light or dark preference.
- **Light** and **Dark** use GTK's built-in theme in that variant (desktop
  themes often have a single variant).

## 5. Languages

The interface is in English and French. It follows the system language;
Settings → Library → Interface language forces one, applied at the next
start.

## 6. Keyboard shortcuts

| Shortcut | Action |
|---|---|
| F5 | Scan the library folders |
| Ctrl+F | Search the list (in the log window: search the log) |
| F2 | Edit the current cell in the list |
| Ctrl+I | Edit Info |
| Ctrl+S | Update Book Files |
| Ctrl+Shift+N | Automatic Numbering |
| Ctrl+B | Fetch Info from Bedetheque |
| Ctrl+L | Organize Files |
| Delete | Remove from Library (in the Pages tab: mark as deleted) |
| Ctrl+J | Message Log |
| Ctrl+, | Settings |
| Tab, Shift+Tab | Next, previous editable column while editing a cell |
| Enter, Escape | Validate, cancel an edit |
