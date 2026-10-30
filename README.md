<p align="center">
  <img src="assets/logo.svg" width="128" height="128" alt="Rayonnage logo: comic books on a shelf">
</p>

<h1 align="center">Rayonnage</h1>

<p align="center">
  Organize, tag, convert and file your comics, BD and mangas.
</p>

Rayonnage is a comic, BD and manga library manager written in Rust with
GTK 4, for Linux, Windows and macOS. It brings to every desktop what
[ComicRack CE](https://github.com/maforget/ComicRackCE) does for a library,
together with its Bedetheque Scrapper 2 and Library Organizer plugins as
native features. Its interface is in English and French.

## Highlights

- **A library from your folders**: CBZ, CBR, CB7, CBT, PDF, DjVu and EPUB,
  scanned in parallel, with ComicRack's columns.
- **Edit like in ComicRack**: the Info dialog, in-place editing in the list
  with suggestions, every ComicInfo.xml field, pages reordered, rotated or
  deleted, and automatic numbering.
- **Write it into the files**: ComicInfo.xml updated in CBZ, CB7, CBT and
  CBR files.
- **Convert**: to CBZ, CB7, CBR, CBT, PDF or images, with ComicRack's export
  options and page naming, and packs split into one book per album.
- **Fetch from Bedetheque**: series, album, edition, credits, summary and
  more from bedetheque.com.
- **File it**: move, copy or rename the files from templates, with profiles,
  preview and undo.
- **Come from ComicRack**: import the settings of ComicRack CE and of both
  plugins.

The complete list is in [Features](docs/features.md).

## Quick start

Download the package for your system from the
[releases](https://github.com/trois-six/rayonnage/releases): an archive for
Linux, a self-contained archive for Windows, an application for macOS.
To build from source:

```sh
cargo build --release
./target/release/rayonnage
```

Requirements, installation and build on each system are in the
[Quickstart](docs/quickstart.md).

## Documentation

The documentation in [`docs/`](docs/index.md) describes the application as
it is: one specification per feature area, the architecture, and the
development workflows.

## Development

Tests, translations, development helpers and the release workflow are
described in [Development](docs/development.md).

## License

MIT — see [LICENSE](LICENSE).
