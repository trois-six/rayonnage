---
type: "Feature Specification"
title: "Bedetheque"
description: "Fetching book information from bedetheque.com: the lookup, the fields fetched, the options and the fallbacks."
tags: [rayonnage, specification, bedetheque, metadata, scraper]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
sources:
  - id: bds2
    resource: https://github.com/maforget/Bedetheque-Scrapper-2
    title: Bedetheque Scrapper 2, the ComicRack CE plugin this feature reproduces
---

# Bedetheque

> Part of the [Rayonnage documentation](index.md).
> See also: [Book Information](book-info.md) · [Book Files](book-files.md) · [ComicRack Import](comicrack-import.md)

---

## 1. Overview

Rayonnage fetches the information of French-language comics from
[bedetheque.com](https://www.bedetheque.com), as the Bedetheque Scrapper 2
plugin of ComicRack CE does.[^bds2] The fetched values are written into the
library like any edit, then into the files when they are updated
([Book Files](book-files.md)).

[^bds2]: Bedetheque Scrapper 2, the ComicRack CE plugin this feature reproduces

## 2. Fetching

- **Books → Fetch Info from Bedetheque** (Ctrl+B) processes the selected
  books one after the other: the series is looked up from the book's series
  name, then the album from its number, then the edition.
- When the result is ambiguous (several series, albums or editions), a
  dialog asks the user to choose, skips the book, or chooses on its own
  after a delay, depending on the settings (§4).
- **Books → Fetch Info from a Bedetheque Link…** takes the URL of an album,
  a series or a magazine on bedetheque.com and applies it to the selected
  books.
- With the "rescrape" option, the books are fetched again from the link in
  their Web field; books without a link are skipped.
- A progress bar follows the run, which can be cancelled; every book and
  every value fetched is recorded in the
  [message log](interface.md#3-message-log).

## 3. Fields fetched

Each field can be turned on or off in Settings → Bedetheque.

| Field | Source on bedetheque.com |
|---|---|
| Series | Series name, with the article and capitalization options (§4). |
| Title | Album title. |
| Number, Count | Album number, padded if asked; total album count, from the volume numbering or only when the series is finished, if asked. |
| Writer, Penciller, Inker, Colorist, Letterer, Cover artist | The album's credits (writer includes storyboard); colorist also sets Black and white. |
| Publisher, Imprint | Publisher and collection. |
| Release date | Legal deposit, otherwise printing date. |
| ISBN | ISBN or EAN of the album. |
| Format | Album format, or "One Shot" when applicable and asked. |
| Genre, Language | Series genre and language. |
| Summary | Series and album summaries; the series summary on every album or on the first only. |
| Rating | Community rating. |
| Series complete | Finished or ongoing series. |
| Notes | "Bedetheque.com - date" and the edition information. |
| Web | Album link, long or short (`www.bedetheque.com/BD--id.html`). |
| Tags | Left unchanged, cleared (`DEL`), or replaced by a given value. |

## 4. Options

Settings → Bedetheque also holds:

- the articles to move to the end of series names ("Les 5 Terres" → "5
  Terres (Les)") and Pascal Case for series and album names;
- the series name ending to remove before searching;
- generic artist values (`<Collectif>`, `<Indéterminé>`…) accepted or not;
- the language of the texts added to the notes, French or English;
- choosing the edition automatically, or always choosing the series by hand;
- what to do with an ambiguous result: skip, ask, or ask with a timeout;
- the maximum number of books per run and the pause between two books;
- a detailed log of the requests and the values extracted.

These options can be imported from the plugin's configuration
([ComicRack Import](comicrack-import.md)).

## 5. Network

Requests go to bedetheque.com only, and only when the user starts a fetch.
If the site rejects direct requests (a 403 error or a timeout), the request
is made again with `curl`, available by default on Linux, macOS and
Windows 10 and later.
