# Rayonnage

*[Version française](README.fr.md)*

Comic, BD and manga library manager in Rust + GTK 4 (Linux, Windows, macOS).
It reproduces part of [ComicRack CE](https://github.com/maforget/ComicRackCE), together with the
features of the [Bedetheque Scrapper 2](https://github.com/maforget/Bedetheque-Scrapper-2) and
[Library Organizer](https://github.com/Stonepaw/comicrack-library-organizer) plugins.

The interface is available in English and French. It follows the system language, and can be
forced in Settings → Library (applied at the next start).

## Features

### Library
- Scans a list of folders defined in the settings (F5), optionally at startup.
  Books whose file disappeared can be removed automatically.
- Scanning is optimized for network drives (sshfs, SMB, NFS):
  - folders and books are read in parallel (16 simultaneous reads by default, configurable in the
    settings);
  - only new or modified books (size or date) are read again;
  - a CBZ is read with a single small access at the end of the file (central directory and
    ComicInfo.xml), without going through the pages;
  - for a PDF, the page count is looked for at the beginning and the end of the file; the whole
    document is only parsed as a last resort.
- Formats read, like ComicRack CE: `.cbz` `.zip`, `.cbr` `.rar` (RAR4 and RAR5), `.cb7` `.7z`,
  `.cbt` `.tar`, `.pdf`, `.djvu`, plus `.epub`. The real format is detected from the magic bytes,
  so a ZIP renamed to `.cbr` is accepted.
- EPUB: the pages are the images of the spine, in reading order (cover first), and the metadata
  comes from the OPF package: title, creators with their role (writer, artist, colorist…),
  publisher, date, language, summary, subjects, ISBN, calibre or EPUB 3 series and position. EPUB
  books can be converted to CBZ/CB7/CBR/PDF; their information stays in the library.
- Recognized page images: jpg, jpeg, jif, jiff, gif, png, tif, tiff, bmp, webp, heic, heif, avif,
  jp2, j2k, jxl. They are sorted in natural order, and `__MACOSX` is ignored.
- Reads `ComicInfo.xml` from the archive, or from an XML file next to the book.
- Book list with ComicRack's columns: series, number, volume, title, release date, creators,
  publisher, imprint, genre, tags, characters, notes, catalog fields (price, condition, ISBN…),
  file fields (name, path, size, format, dates), etc.
  - Columns can be sorted, resized and reordered by dragging their headers; the layout is saved
    immediately. Visible columns are chosen in Menu → Columns….
  - "Proposed values": when the series and number are empty, they are guessed from the file name
    and shown greyed out, as in ComicRack.
  - In-place editing, as in ComicRack: a slow click on a cell of the selected row (or F2) edits
    it, with suggestions taken from the whole library. Tab / Shift+Tab move to the next /
    previous editable column, Up / Down to the next / previous row, Enter validates, Escape
    cancels. The value applies to all the selected books; editing the file name renames the file.
  - Numeric columns (number, volume, count, year, month, day…) of the selected rows have up/down
    buttons to increment or decrement the value.
  - Quick search (Ctrl+F), filter by folder and by "unsaved info" in the sidebar.
  - Information panel with the cover.
- Activity feedback:
  - During a scan, a progress bar and an activity indicator are shown at the bottom right, and
    books appear as they are found.
  - The **Message log** (Ctrl+J, or the "Log" button at the bottom right) details every operation:
    books added, converted or written, and errors. It is also copied to `journal.log` in the cache
    folder, emptied at each start. A search field (Ctrl+F) highlights the matches; Enter /
    Shift+Enter (or F3 / Shift+F3) go to the next / previous one; "Errors only" hides the rest.
- Light or dark appearance in Settings → Library: "System" keeps the desktop theme, "Light" and
  "Dark" use GTK's built-in theme (desktop themes often have a single variant).

### Editing book info (Ctrl+I)
"Info" dialog modelled on ComicRack's, for one or several books.
- Tabs:
  - **Summary**: cover, ratings, web link.
  - **Details**: series, numbers, story arc, dates, publisher, creators, language, manga, B&W…
  - **Plot & Notes**: summary, notes, review, characters, teams, locations.
  - **Catalog**: ISBN, price, condition, owner…
  - **Custom**: free values.
  - **Pages**: as in ComicRack, a list of the pages with thumbnail, size, dimensions and name, plus
    a preview. The order is changed by drag and drop. The context menu sets the page type,
    rotation, position (default/near/far) and bookmark; it can also move pages to the top or
    bottom, mark them as deleted (Delete), reset or sort the order (by name, alphabetical or
    numeric), select all / invert the selection and copy a page. The order is stored in
    ComicInfo.xml; conversions follow it, apply the rotations and drop the deleted pages.
- With several books selected, only the modified fields are applied.
- Suggestions come from the values already present in the library.
- Previous/Next buttons.

### Updating book files
- **Update Book Files** (Ctrl+S) writes `ComicInfo.xml` into CBZ/ZIP, CB7/7Z and CBT/TAR files,
  and into CBR/RAR files when the `rar` program is installed. Books modified but not yet written are
  marked ✎. An option writes the files automatically after each change.
  - In a CBZ, the new ComicInfo.xml (Deflate, like ComicRack) is appended at the end of the file
    without rewriting the pages: a few KB are written instead of the whole book, and the rest of
    the file is left untouched.
  - As in ComicRack, pages are neither renamed nor modified.
- **Refresh** reads the information from the files again. The "force" variant replaces the
  library values.

### Automatic numbering (Ctrl+Shift+N)
Reproduces the "Autonumber Wizard": numbers the series or the alternate series in list order,
starting from a given number, and can store the total count.

### Conversion / export
- **Convert to CBZ / CB7 / CBR** presets, which replace the book in the library.
- Full **Export…** dialog:
  - **Location**: selected folder, same folder, or replacement.
  - **File options**: delete the original, add to the library, overwrite, combine several books
    into one.
  - **Naming**: file name, book caption, or numbered custom name.
  - **Output format**: CBZ, CBR, CB7, CBT, PDF or image folder; compression level; embedded
    ComicInfo.xml.
  - **Pages**: removal of pages by type, page selection (`1-5,8`), tags to append.
  - **Page names**: as in ComicRack, pages are renamed `P00001.jpg`, `P00002.jpg`… (followed by
    "- bookmark" when the page has one) and placed at the root of the archive. The "Keep original
    image names" option keeps the names without sub-folders; duplicates get `_2`, `_3`…
  - **Images**: conversion to JPEG, PNG, WebP, GIF, TIFF or BMP with quality setting; resizing;
    double pages (split, rotate, adapt); brightness, contrast, saturation, gamma, sharpening,
    automatic contrast.
- Custom presets can be saved.
- **Split Packs into Albums…** turns a pack (an archive holding several albums) into one book per
  album: each archive found inside the pack (cbz, cbr, zip, rar, pdf…) and each folder of images
  (with its ComicInfo.xml) becomes a book, converted to CBZ/CB7/CBR/CBT/PDF or kept as it is. The
  albums go to a folder named after the pack, next to the pack or to a chosen folder; they are
  added to the library, and the pack can be deleted afterwards.

### Fetching book info from Bedetheque (Ctrl+B)
Books menu → "Fetch Info from Bedetheque" (based on the Bedetheque Scrapper 2 plugin). The
series, album and edition are looked up on bedetheque.com, with a choice dialog when the result
is ambiguous.
- Fetched fields: series, title, number, creators, publisher, imprint, legal deposit date, ISBN,
  format, genre, summary, rating, album count, series complete, language, web link.
- Settings → Bedetheque: each field can be enabled or disabled, with options for articles,
  capitalization, "One Shot", delay between books, etc.
- "Fetch Info from a Bedetheque Link…" takes an album, series or magazine URL; the "rescrape"
  option fetches again from the Web field of the books.
- If bedetheque.com rejects direct requests (403 error, timeout), the application retries with
  `curl`, available by default on Linux, macOS and Windows 10+.

### Organizing files (Ctrl+L)
Books menu → "Organize Files…" (based on the Library Organizer plugin): moves, copies or simulates
the filing of book files according to folder and file name templates. "Organize Files Again"
reuses the last profiles; "Undo Last Organization" puts the files back.
- Template example: `{<publisher>}/{<series>}{ (<startyear>)}` and `{<series>}{ #<number3>}`.
- Multiple profiles, live preview.
- Filing options: replacement values for empty fields, illegal characters, month names, exclusions
  by folder or by rules, removal of empty folders, duplicate handling (replace, skip, rename).
- Detailed report and undo of the last run.

### Importing a ComicRack configuration
Menu → "Import ComicRack Settings…" reads a ZIP (or a folder) holding `App.Config` (Bedetheque
Scrapper 2), `losettingsx.dat` (Library Organizer) and `Config.xml` (ComicRack): Bedetheque
options, file organization profiles, scan and conversion preferences, export settings and
presets. The Windows drives used by the profiles (`X:\`…) are mapped to local folders. On Windows,
these PowerShell lines gather the files into `comicrack-config.zip` on the desktop:

```powershell
$out = "$env:USERPROFILE\Desktop\comicrack-config"
New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem "$env:APPDATA\cYo", "$env:ProgramFiles\ComicRack*" -Recurse `
    -Include App.Config, losettingsx.dat, Config.xml -ErrorAction SilentlyContinue |
  ForEach-Object { Copy-Item $_.FullName "$out\$($_.Directory.Name) - $($_.Name)" }
Compress-Archive "$out\*" "$out.zip" -Force
```

## Building

Requires Rust (edition 2024, so rustc ≥ 1.85), GTK ≥ 4.12 and a C++ compiler for the embedded
unrar library.

- **Linux (Debian/Ubuntu)**: `sudo apt install libgtk-4-dev build-essential`
- **Fedora**: `sudo dnf install gtk4-devel gcc-c++`
- **macOS**: `brew install gtk4 pkgconf`
- **Windows**: with [MSYS2](https://www.msys2.org/) (UCRT64 environment), run
  `pacman -S mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-rust mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-gcc`

Then:

```sh
cargo build --release
./target/release/rayonnage
```

The `.github/workflows/ci.yml` workflow builds and tests the application on the three systems.

### External programs (optional)
Their paths are set in Settings → Files.
- `rar` (WinRAR / RAR) is used to create CBR files and to write ComicInfo.xml into a CBR. RAR is a
  proprietary format: reading is built in, but creating archives requires this program. ComicRack
  CE cannot create CBR files at all.
- `pdftoppm` (poppler) is used for PDF pages that are not a single image.
- `ddjvu` and `djvused` (DjVuLibre) are used to read DjVu files.

## Translations
Interface strings are written in English in the code with the `t!` macro (`t!("Text")`,
`t!("{0} book(s)", n)`). French translations live in `src/i18n/fr_*.rs`; a missing entry falls back
to English. A unit test checks that each translation keeps the placeholders of its English text.

## Data
Settings are stored in `settings.json`, and the library in an SQLite database, `library.db` (one
row per book; only the books that changed are written), in the user's configuration and data
folders:
- Linux: `~/.config/rayonnage` and `~/.local/share/rayonnage`
- Windows: `%APPDATA%\trois-six\rayonnage`
- macOS: `~/Library/Application Support/org.trois-six.rayonnage`

Cover thumbnails are cached. Settings and library from the previous name (`books-organizer`) are
copied automatically on first start, and a former `library.json` is imported into the database
(then renamed `library.json.imported`).
