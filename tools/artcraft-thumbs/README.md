# ArtCraft Thumbnails

Windows Explorer thumbnails for ArtCraft documents, like a PSD codec does for Photoshop files:
Explorer shows what is inside the file, with the app's icon in the bottom-right corner so you can
tell which app it belongs to.

| File | App | Preview read |
|------|-----|--------------|
| `.pcraft` | PhotoCraft | `thumb.png` (256 px), or `composite/preview.png` (1024 px) for larger views |
| `.vectorcraft`, `.drawcraft` | VectorCraft | the `preview` PNG in the file's head (256 px) |

It is a shell extension DLL (`IThumbnailProvider`). It only reads the preview each app embeds
when it saves: it renders nothing and changes no app. VectorCraft embeds the preview by default
since the change that turned it on; older `.vectorcraft` files show one after they are saved again.

The badge is taken at run time from the installed app's exe (`%ProgramFiles%\<App>\<id>.exe`, or
the path the install script recorded under `HKCU\Software\ArtCraft\Thumbnails`). No copy of any
logo ships with the DLL. Without the app the thumbnail shows without a badge.

## Install

```powershell
scripts\install-thumbs.ps1                 # download the newest DLL and register it (this user, no admin)
scripts\install-thumbs.ps1 -SearchDir D:\Apps   # apps installed outside Program Files
scripts\install-thumbs.ps1 -Uninstall
```

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

`src/formats.rs` reads the previews (tested without Windows), `src/image.rs` fits and badges
them, `src/com.rs` is the COM server. Every entry point catches panics and fails with an
HRESULT, so a damaged file never takes Explorer down. Bump `version` in `Cargo.toml` for a new
versioned release.
