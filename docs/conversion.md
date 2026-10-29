---
type: "Feature Specification"
title: "Conversion"
description: "Conversion presets and the Export dialog to CBZ, CB7, CBR, CBT, PDF or an image folder, page naming, image processing, and the split of packs into albums."
tags: [rayonnage, specification, conversion, export, cbz, cbr, cb7, packs]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Conversion

> Part of the [Rayonnage documentation](index.md).
> See also: [Book Information §3](book-info.md#3-pages) · [Book Files](book-files.md) · [ComicRack Import](comicrack-import.md)

---

## 1. Overview

Conversion writes a new book file from the selected books: another archive
format, a PDF or a folder of images, with the book information embedded,
pages chosen and ordered, and images optionally processed. It follows
ComicRack CE's Export dialog. Any format read ([Library §4](library.md#4-formats))
can be converted, EPUB included.

## 2. Presets

Books → Convert offers:

- **Convert to CBZ**, **Convert to CB7**, **Convert to CBR**: each writes the
  new file next to the original and replaces the original, in the folder
  and in the library.
- The user's own presets, saved from the Export dialog (§3).
- **Export Books…**, the full dialog.

Before a conversion deletes or replaces original files, a confirmation is
asked. It can be turned off, for example by importing a ComicRack
configuration where it was hidden ([ComicRack Import](comicrack-import.md)).

Creating CBR files needs the `rar` program
([Quickstart §3](quickstart.md#3-external-programs)).

## 3. The Export dialog

Export Books… shows the export settings, starting from the last ones used
(the "current settings"), or from a chosen preset. "Save as Preset…" saves
them under a name.

| Section | Settings |
|---|---|
| Export Location | Selected folder, same folder as the original, or replacement of the original; delete the original after a successful export; add the new book to the library; overwrite existing files; combine all selected books into one. |
| File Names | The file name, the book caption, or a custom name numbered from a starting number. |
| Export Format | Same as original, CBZ, CBR, CB7, CBT, PDF or image folder; compression (none, medium, strong); embed the book information (ComicInfo.xml). |
| Pages | Remove pages of the chosen types (Deleted by default); included pages (`1-5,8`, empty for all); copy unreadable pages as they are instead of failing; tags to add to the books. |
| File | Image format (original, JPEG, PNG, WebP, WebP lossless, GIF, TIFF, BMP) with the quality; resizing to a width, a height or both, without enlarging; double pages kept, split, rotated or fitted to the width; keep the original image names. |
| Image Processing | Brightness, contrast, saturation, gamma, sharpening, automatic contrast enhancement. |

Each converted book and each error is written in the
[message log](interface.md#3-message-log).

## 4. Pages and page names

- Pages are written in the book's page order
  ([Book Information §3](book-info.md#3-pages)), with their rotation applied
  and the deleted pages dropped.
- As in ComicRack, pages are renamed `P00001.jpg`, `P00002.jpg`…, followed
  by " - bookmark" when the page has a bookmark, and placed at the root of
  the archive.
- With "Keep original image names", the names of the images are kept
  without their sub-folders; duplicates get `_2`, `_3`…
- Images are copied byte for byte unless an image setting asks for a change.
- The written ComicInfo.xml describes the new pages.

## 5. Split packs into albums

A pack is an archive holding several albums. Books → Convert → Split Packs
into Albums… turns each selected pack into one book per album.

- Every archive found in the pack (cbz, cbr, zip, rar, cb7, pdf…) is an
  album, and so is every folder of images, with its ComicInfo.xml when it
  has one.
- **Format**: the albums are converted to CBZ, CB7, CBR, CBT or PDF, or kept
  as they are (a `.zip` album becomes `.cbz`, a `.rar` album `.cbr`; a folder
  of images becomes a CBZ).
- **Destination**: a folder named after the pack, next to it (the default);
  the pack's own folder; or a chosen folder.
- The albums are added to the library; the pack can be deleted once every
  album is written.
