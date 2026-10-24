# ArtCraft Thumbnails

Windows Explorer thumbnails for the files of the eight ArtCraft apps, like a PSD codec does for
Photoshop files: Explorer shows what is inside the file, with the app's icon in the bottom-right
corner so you can tell which app it belongs to.

| File | App | Thumbnail |
|------|-----|-----------|
| `.pcraft` | PhotoCraft | `thumb.png` (256 px), or `composite/preview.png` (1024 px) for larger views |
| `.vectorcraft`, `.drawcraft` | VectorCraft | the `preview` PNG in the file's head |
| `.fcproj` | FilmCraft | the `preview` PNG in the envelope (the frame at the playhead when saved) |
| `.ecproj` | EffectCraft | the `preview` PNG the file starts with (the active comp when saved) |
| `.pdf` | PdfCraft | the first page, rendered with PdfCraft's patched hayro |
| `.docx` `.docm` `.dotx` `.dotm` | WordCraft | the package thumbnail (WordCraft writes the first page) |
| `.xlsx` `.xlsm` `.xltx` `.xltm` | GridCraft | the package thumbnail, else the first sheet's cells drawn as a grid |
| `.dwg` `.dwt` / `.dxf` | CADCraft | the DWG preview image / the DXF THUMBNAILIMAGE section |

It is a shell extension DLL (`IThumbnailProvider`) that tells the type from the file's content.
Files saved before an app embedded a preview show one once they are saved again with a version
that does (PhotoCraft always has; VectorCraft, FilmCraft, EffectCraft, WordCraft and CADCraft
from the commits that added it). DWG/DXF files from AutoCAD and Office files saved with a
thumbnail show theirs too.

The badge is taken at run time from the installed app's exe (`%ProgramFiles%\<App>\<id>.exe`, or
the path the install script recorded under `HKCU\Software\ArtCraft\Thumbnails`). No copy of any
logo ships with the DLL. Without the app the thumbnail shows without a badge.

## Install

```powershell
scripts\install-thumbs.ps1                 # download the newest DLL and register it (this user, no admin)
scripts\install-thumbs.ps1 -SearchDir D:\Apps   # apps installed outside Program Files
scripts\install-thumbs.ps1 -Uninstall
```

It registers on each extension and on the program it opens with, in HKCU only, so for this user
it takes over from another thumbnail handler of the type (a PDF reader's, Office's);
`-Uninstall` removes only its own entries, and the other handler is back. Run it again after
changing which program opens a type.

Fixed download link (the rolling `artcraft-thumbs` release, updated by
`.github/workflows/artcraft-thumbs.yml` on every push that changes this folder):
https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/download/artcraft-thumbs/ArtCraft-Thumbs.dll

Explorer caches thumbnails: files it already showed keep their old picture until the cache is
cleared (Disk Cleanup > Thumbnails) or the file changes.

## Develop

```powershell
cd tools\artcraft-thumbs
cargo test
cargo build --release
..\..\scripts\install-thumbs.ps1 -Dll target\release\artcraft_thumbs.dll
```

`src/formats/` reads each type (tested without Windows), `src/image.rs` decodes, fits and
badges, `src/com.rs` is the COM server (and draws spreadsheets with GDI). Every entry point
catches panics and fails with an HRESULT, so a damaged file never takes Explorer down. Bump
`version` in `Cargo.toml` for a new versioned release.
