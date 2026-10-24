//! # artcraft-thumbs
//!
//! A Windows shell thumbnail provider (`IThumbnailProvider`) for the files of the eight ArtCraft
//! apps: Explorer shows what is inside, with the app's own icon in the bottom-right corner, the
//! way a PSD codec shows Photoshop files. See [`formats`] for what each file type shows.
//!
//! The badge is taken from the installed app's exe at run time (no copy of any logo ships
//! here). `scripts/install-thumbs.ps1` registers the DLL for the current user.

pub mod formats;
pub mod image;

#[cfg(windows)]
mod com;

use formats::{App, Found, Source};
use image::Rgba;

/// The thumbnail Explorer gets for a file at `size` pixels: the file's preview fitted to it,
/// badged with `icon(app, side)` when that gives one. `paint_sheet(cells, size)` draws a
/// spreadsheet (it needs fonts, which the caller has).
pub fn thumbnail(
    src: &mut dyn Source,
    size: u32,
    icon: impl FnOnce(App, u32) -> Option<Rgba>,
    paint_sheet: impl FnOnce(&formats::ooxml::Sheet, u32) -> Option<Rgba>,
) -> Option<Rgba> {
    let (app, found) = formats::preview(src, size)?;
    let img = match found {
        Found::Image(bytes) => image::decode(&bytes)?,
        Found::Pixels(p) => p,
        Found::Sheet(s) => paint_sheet(&s, size)?,
    };
    let mut img = image::fit(&img, size);
    let side = image::badge_size(img.width, img.height);
    if side >= 8
        && let Some(b) = icon(app, side)
    {
        image::badge(&mut img, &image::fit(&b, side));
    }
    Some(img)
}
