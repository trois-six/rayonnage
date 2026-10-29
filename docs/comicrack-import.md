---
type: "Feature Specification"
title: "ComicRack Import"
description: "Importing the settings of ComicRack CE and of its Bedetheque Scrapper 2 and Library Organizer plugins, and how Windows paths are mapped."
tags: [rayonnage, specification, comicrack, import, migration]
generated: { by: claude-code/claude-opus-5-5, at: 2026-10-29T20:30:00Z }
---

# ComicRack Import

> Part of the [Rayonnage documentation](index.md).
> See also: [Bedetheque](bedetheque.md) · [File Organization](organizer.md) · [Conversion](conversion.md)

---

## 1. Overview

Library → Import ComicRack Settings… brings the configuration of a ComicRack
CE installation, usually on Windows, into Rayonnage. It reads three files,
recognized by their names, from a ZIP archive or a folder:

| File | Origin | Imported as |
|---|---|---|
| `App.Config` | Bedetheque Scrapper 2 plugin | The fields and options of Settings → Bedetheque ([Bedetheque §4](bedetheque.md#4-options)). |
| `losettingsx.dat` | Library Organizer plugin | The file organization profiles ([File Organization §3](organizer.md#3-profiles)). |
| `Config.xml` | ComicRack CE | Scan at startup, removal of missing files, convert confirmation, and the export settings and presets ([Conversion §3](conversion.md#3-the-export-dialog)). |

The library itself (`ComicDb.xml`) is not imported: a scan of the same
folders reads the information from the files.

## 2. The import dialog

After the file is chosen, the dialog lists what was found and lets the user
pick each part:

- Bedetheque settings (fields and options);
- file organization profiles, with their number and names;
- preferences: scan at startup, removal of missing files, convert
  confirmation;
- export settings and presets.

Imported export presets replace the presets of the same name; the others are
kept.

## 3. Windows paths

Profiles and export settings name Windows folders (`X:\Comics`,
`\\server\share`…). The dialog lists each drive or share they use, and asks
for the local folder that matches it; the paths are rewritten with it on
import, separators included.

## 4. Gathering the files on Windows

These PowerShell lines copy the three files into `comicrack-config.zip` on
the desktop:

```powershell
$out = "$env:USERPROFILE\Desktop\comicrack-config"
New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem "$env:APPDATA\cYo", "$env:ProgramFiles\ComicRack*" -Recurse `
    -Include App.Config, losettingsx.dat, Config.xml -ErrorAction SilentlyContinue |
  ForEach-Object { Copy-Item $_.FullName "$out\$($_.Directory.Name) - $($_.Name)" }
Compress-Archive "$out\*" "$out.zip" -Force
```
