---
type: "Quickstart Guide"
title: "Rayonnage Quickstart"
description: "Requirements and procedures for installing a released package on Linux, Windows or macOS, or building Rayonnage from source, with the optional external programs and where the data lives."
tags: [rayonnage, quickstart, install, linux, windows, macos]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Rayonnage Quickstart

> Part of the [Rayonnage documentation](index.md).
> See also: [Development](development.md) · [Features](features.md)

Rayonnage is a desktop application. Download the package of a release (§1),
or build it from source (§2). Nothing else is needed to manage a library;
a few external programs add optional abilities (§3).

| Path | Intended use |
|---|---|
| Release package | Users who do not build from source |
| Source build | Contributors, and systems without a package |

## 1. Download a release

Open the [Rayonnage releases](https://github.com/trois-six/rayonnage/releases)
page and download the package for the system. Every release includes
`SHA256SUMS` to check the downloads.

| Platform | Download | Contents |
|---|---|---|
| Linux x86-64 | `rayonnage-<version>-linux-x86_64.tar.gz` | The executable, a desktop entry, the icon and `install.sh` |
| Windows 10 or later, x86-64 | `rayonnage-<version>-windows-x86_64.zip` | `bin/rayonnage.exe` with GTK and its resources |
| macOS 14 or later, Apple silicon | `Rayonnage-<version>-macos-arm64.zip` | `Rayonnage.app` with GTK and its resources |

These are portable packages, not installers.

### Linux

The executable uses the system's GTK, which must be version 4.12 or later
(Debian 13, Ubuntu 24.04, Fedora 39 and later). On Debian and Ubuntu,
`sudo apt install libgtk-4-1` installs it.

```sh
tar xzf rayonnage-<version>-linux-x86_64.tar.gz
cd rayonnage-<version>-linux-x86_64
./install.sh        # or run ./rayonnage directly
```

`install.sh` copies the executable to `~/.local/bin`, and the desktop entry
and the icon to `~/.local/share`, so Rayonnage appears in the application
menu.

On Linux, Rayonnage uses GTK's Vulkan renderer unless `GSK_RENDERER` is set.
With some drivers (NVIDIA), closing the application can then take a few
seconds; `GSK_RENDERER=ngl` (GTK's OpenGL renderer) closes at once.

### Windows

Extract the archive anywhere and run `bin\rayonnage.exe`; keep the folders
of the archive together, GTK finds its resources next to the executable.
The executable is not signed: Windows SmartScreen may warn about it; choose
**More info**, then **Run anyway**.

### macOS

Extract the archive and move `Rayonnage.app` to Applications. The
application is not signed with an Apple Developer ID, so macOS refuses its
first opening: open it once, close the warning, then open
**System Settings → Privacy & Security** and choose **Open Anyway** beside
Rayonnage's entry. Alternatively, `xattr -cr /Applications/Rayonnage.app`
lifts the quarantine.

## 2. Build from source

### Software requirements

- Rust 1.85 or later (edition 2024).
- GTK 4.12 or later, with its development files.
- A C++ compiler, for the embedded unrar library.

| System | Command |
|---|---|
| Debian, Ubuntu | `sudo apt install libgtk-4-dev build-essential` |
| Fedora | `sudo dnf install gtk4-devel gcc-c++` |
| macOS | `brew install gtk4 pkgconf` |
| Windows | In [MSYS2](https://www.msys2.org/) UCRT64: `pacman -S mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-rust mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-gcc` |

### Build and run

```sh
git clone https://github.com/trois-six/rayonnage
cd rayonnage
cargo build --release
./target/release/rayonnage
```

## 3. External programs

All optional. Rayonnage looks for them in the `PATH`; Settings → Files sets
another path and shows whether each one was found.

| Program | Package | Used for |
|---|---|---|
| `rar` | WinRAR or RAR | Creating CBR files and writing ComicInfo.xml into a CBR. Reading RAR is built in; creating RAR archives needs this proprietary program (ComicRack CE cannot create CBR files at all). |
| `pdftoppm` | poppler | PDF pages that are not a single image. |
| `ddjvu`, `djvused` | DjVuLibre | Reading DjVu files. |
| `curl` | curl | Fallback for bedetheque.com requests ([Bedetheque §5](bedetheque.md#5-network)); present by default on Linux, macOS and Windows 10 and later. |

## 4. Where the data lives

| | Linux | Windows | macOS |
|---|---|---|---|
| Settings (`settings.json`) | `~/.config/rayonnage` | `%APPDATA%\trois-six\rayonnage\config` | `~/Library/Application Support/org.trois-six.rayonnage` |
| Library (`library.db`) | `~/.local/share/rayonnage` | `%APPDATA%\trois-six\rayonnage\data` | `~/Library/Application Support/org.trois-six.rayonnage` |
| Cache (thumbnails, `journal.log`) | `~/.cache/rayonnage` | `%LOCALAPPDATA%\trois-six\rayonnage\cache` | `~/Library/Caches/org.trois-six.rayonnage` |

- The library is an SQLite database; deleting it empties the library, and a
  scan builds it again from the files (information not written into the
  files is then lost).
- The cache can be deleted at any time.
- Settings and library of the former name, `books-organizer`, are copied on
  first start, and a former `library.json` is imported into the database,
  then renamed `library.json.imported`.
