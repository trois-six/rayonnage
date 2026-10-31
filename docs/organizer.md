---
type: "Feature Specification"
title: "File Organization"
description: "Moving, copying or simulating the filing of book files from folder and file name templates, with profiles, exclusions, duplicates and undo."
tags: [rayonnage, specification, organizer, templates, renaming]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
sources:
  - id: library-organizer
    resource: https://github.com/Stonepaw/comicrack-library-organizer
    title: Library Organizer, the ComicRack plugin this feature reproduces
---

# File Organization

> Part of the [Rayonnage documentation](index.md).
> See also: [Library](library.md) · [ComicRack Import](comicrack-import.md)

---

## 1. Overview

File organization moves, copies or renames book files according to folder
and file name templates built from their information, as the Library
Organizer plugin of ComicRack does.[^library-organizer] The library follows
the files it moves.

[^library-organizer]: Library Organizer, the ComicRack plugin this feature reproduces

## 2. Running

- **Books → Organize Files…** (Ctrl+L) opens the dialog: choice of the
  profiles to apply, the templates, and a live preview of the new paths of
  the selected books.
- **Organize Files Again** applies the last profiles used without the dialog.
- **The small arrow next to the toolbar button** lists the profiles;
  choosing one organizes the selected books with it at once, as the
  ComicRack toolbar button does.
- **Undo Last Organization** puts the files of the last run back where they
  were. The undo information is saved after each file moved, so an
  interrupted run can be undone; the files that could not be put back (a
  drive offline, for example) stay in it for a later undo.
- A detailed report lists each file moved, copied, skipped or failed; it is
  also written in the [message log](interface.md#3-message-log).

## 3. Profiles

A profile holds:

| Setting | Effect |
|---|---|
| Mode | Move, Copy (optionally adding the copy to the library), or Simulate (report only). |
| Base folder, folder template | Where the files go, a full path; the folder part can be turned off to rename in place. |
| File name template | The new file name; it can be turned off to only move. |
| Empty values | A replacement per field when its value is empty, and one for an empty folder name (empty drops that level). |
| Fail on empty values | Books with an empty value in chosen fields are left alone, or moved to a "failed" folder. |
| Illegal characters | The replacement of each character not allowed in file names. |
| Months | The names used for month numbers. |
| Exclusions | Folders excluded, and rules on the information (field, operator, value, grouped with all or any), to exclude or to include only the matching books. |
| Empty folders | Removal of the folders emptied by a move, except the ones listed, the base and failed folders, the library folders and the home folder. |
| Duplicates | When the target exists: overwrite, skip or rename. |
| Other | Replace multiple spaces; keep the reading percentage on a copy; do not ask when a multiple value has one item. |

Several profiles can be kept, and imported from the plugin's configuration
([ComicRack Import](comicrack-import.md)); imported profiles are added to
the existing ones, renamed when a name is taken.

### Safety of the files

- A target that is the book file itself (reached through a link, or a name
  that differs only by case on a case-insensitive drive such as an SMB
  share) is never taken for a duplicate: a case-only change is applied, and
  nothing is deleted.
- Overwriting moves the existing file aside, and deletes it only once the
  new one is in place; if the move or copy fails, it is put back.
- A file placed by the same run is never overwritten: the next book with the
  same name is renamed.
- A move to another drive copies the file, checks the copy is complete on
  the disk, and only then deletes the original. A copy is written under a
  temporary name first, so a failed copy never leaves a partial book.
- When a move fails, the folders created for it are removed.

## 4. Templates

Every field is written `{prefix<name args>postfix}`. The block becomes
prefix + value + postfix when the value is not empty, and the profile's
replacement for empty values otherwise. Blocks can be nested and are
resolved from the inside out. In folder templates, `\` and `/` both
separate folders.

```text
{<publisher>}/{<series>}{ (<startyear>)}
{<series>}{ #<number3>}{ - <title>}
```

| Form | Meaning |
|---|---|
| `<number3>` | Padding to 3 digits (`0` for automatic). |
| `<writer(, )(issue)>` | A multiple-value field: separator, and whether to choose the values per issue or per series (a dialog asks which values to keep). |
| `<manga(Manga)(!)>` | A yes/no field: the text inserted when it is set, or when it is not (`!`). |
| `<read(Read)(>)(90)>` | The text inserted when the reading percentage compares to a number. |
| `<counter(1)(1)(3)>` | A counter: start, step, padding. |
| `<first(Series)>` | The first letter of a field. |
| `<AddedDate(dd/MM/yy)>` | A date field with its format. |
| `<Custom(key)>` | A custom value. |
| `<?name(text)>`, `<!name(text)>` | Inserts the prefix and postfix only when the value equals the text (`(!regex)` for a regular expression), or does not. |

Fields include the information fields (series, number, count, volume,
title, year, month, publisher, imprint, format, language, creators, genre,
tags, characters, teams, locations, story arc, series group, scan
information, manga…) and values computed over the series: start year,
start month, first and last issue numbers.
