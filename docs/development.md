---
type: "Development Specification"
title: "Development Environment and Workflows"
description: "Local development, tests, translations, development helpers, continuous integration and release automation."
tags: [rayonnage, specification, development, rust, ci, release]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# Development Environment and Workflows

> Part of the [Rayonnage documentation](index.md).
> See also: [Architecture](architecture.md) · [Quickstart §2](quickstart.md#2-build-from-source)

---

## 1. Prerequisites

The build requirements are those of a source build
([Quickstart §2](quickstart.md#2-build-from-source)): Rust 1.85 or later,
GTK 4.12 or later with its development files, and a C++ compiler. The
external programs ([Quickstart §3](quickstart.md#3-external-programs)) are
needed only to exercise the features that use them.

## 2. Command Reference

| Command | Purpose |
|---|---|
| `cargo run` | Build in debug mode and start the application. |
| `cargo build --release` | Optimized build, in `target/release/rayonnage`. |
| `cargo test` | Unit tests. |
| `cargo clippy --all-targets` | Lints. |

### 2.1 Tests

Unit tests sit next to the code (`#[cfg(test)]` modules): ComicInfo.xml
round trips, archive reading and writing, page naming, the organizer's
template engine and rules, the ComicRack configuration import, EPUB
parsing, the library database, and the translation catalogues.

Two benchmarks are ignored by default. They only read the books they are
given:

```sh
# Full scan of a folder into an empty library.
BENCH_DIR=/path/to/comics BENCH_THREADS=16 \
  cargo test --release bench_scan -- --ignored --nocapture

# Reads of the first BENCH_N books listed in a file, one path per line.
BENCH_LIST=books.txt BENCH_N=10 BENCH_THREADS=16 \
  cargo test --release bench_read_content -- --ignored --nocapture
```

## 3. Development Helpers

Debug builds read a few environment variables to drive the interface
without a person, for instance to take screenshots of a change:

| Variable | Effect |
|---|---|
| `BO_SHOT=<prefix>` | After a delay, saves every visible window as `<prefix>-<n>.png`, then quits. The run does not hand over to an instance already running. |
| `BO_SHOT_DELAY=<ms>` | The delay before the screenshots, 5000 ms by default. |
| `BO_ACTIONS=a,b:param,…` | Window actions activated one after the other before the screenshots (`select-all`, `edit`, `journal`, `convert:cbz`…); `find:TEXT` types TEXT in the search field of the log window. |
| `BO_TAB=<n>` | The tab the Info dialog opens on. |

```sh
BO_ACTIONS=select-all,edit BO_TAB=5 BO_SHOT=/tmp/shot cargo run
```

To work on a copy of the data, point the XDG variables at a scratch folder
on Linux (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`).

## 4. Translations

Interface strings are written in English in the code with the `t!` macro:
`t!("Text")`, `t!("{0} book(s)", n)`. The French translations live in
`src/i18n/fr_*.rs`, one catalogue per area (core, ui, dialogs, bedetheque,
organizer). A string missing from the catalogue is shown in English.

A unit test checks that each translation keeps the placeholders of its
English text. To add a language, add a `Language` value and its catalogues
in `src/i18n/`, and the choice in Settings → Library.

## 5. Continuous Integration

`.github/workflows/ci.yml` runs on every push and pull request: on Linux
(Ubuntu 24.04, GTK from apt), macOS (macOS 14, GTK from Homebrew) and
Windows (MSYS2 UCRT64), it runs the tests, builds the release executable
and keeps it as a workflow artifact.

## 6. Release Automation

Pushing a tag `v<version>` starts `.github/workflows/release.yml`; the tag
must match the version of `Cargo.toml`. Running the workflow by hand builds
the packages as workflow artifacts and publishes nothing.

| Package | Script | Contents |
|---|---|---|
| `rayonnage-<version>-linux-x86_64.tar.gz` | `packaging/package-linux.sh` | The executable, `org.troissix.Rayonnage.desktop`, the icon, `install.sh`, README and licence. It uses the system's GTK. |
| `rayonnage-<version>-windows-x86_64.zip` | `packaging/package-windows.sh` | `bin/` with the executable and every MSYS2 DLL it needs (found recursively with `ldd`), the gdk-pixbuf loaders and their cache, the compiled GSettings schemas and the Adwaita and hicolor icon themes, laid out as GTK expects. |
| `Rayonnage-<version>-macos-arm64.zip` | `packaging/package-macos.sh` | `Rayonnage.app`: the executable and the Homebrew libraries rewritten by `dylibbundler`, the pixbuf loaders, schemas and icon themes in `Resources`, a launcher script that points GTK at them, `Info.plist` (bundle identifier `org.troissix.Rayonnage`, minimum macOS 14), the icon made from `packaging/icon-1024.png`, and an ad hoc signature. |

Once the three packages are built, the release job creates the GitHub
release with them, `SHA256SUMS`, and notes generated from the commits.

Neither the Windows executable nor the macOS application is signed with a
publisher certificate; the user lifts the warning on first opening
([Quickstart §1](quickstart.md#1-download-a-release)).

## 7. Documentation

The documentation in `docs/` is an [Open Knowledge Format](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
0.2 bundle:

- every document but `index.md` and `log.md` starts with a YAML frontmatter
  carrying at least `type`, and here also `title`, `description`, `tags`
  and `generated` (`{ by, at }`, `by` being `human:<id>` for a person or
  `<tool>/<version>` for an agent);
- `index.md` lists every document with its description; it is the only
  file with an `okf_version` key;
- `log.md` records the changes, newest first, under `YYYY-MM-DD` headings;
- documents describe the application as it is; each feature specification
  is linked from the [Features](features.md) catalogue.

A change to a feature updates its specification, its line in
[Features](features.md), the `generated` date of the documents touched, and
`log.md`.
