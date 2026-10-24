//! # artcraft-thumbs
//!
//! A Windows shell thumbnail provider (`IThumbnailProvider`) for ArtCraft documents: Explorer
//! shows what is inside a `.pcraft` or `.vectorcraft` file, with the app's own icon in the
//! bottom-right corner, the way a PSD codec shows Photoshop files.
//!
//! It only reads the preview each app already embeds when it saves ([`formats`]); it renders
//! nothing and changes no app. The badge is taken from the installed app's exe at run time (no
//! copy of any logo ships here). `scripts/install-thumbs.ps1` registers the DLL for the current
//! user.

pub mod formats;
pub mod image;

#[cfg(windows)]
mod com;

use formats::{App, Source};
use image::Rgba;

/// The thumbnail Explorer gets for a file at `size` pixels: the file's preview fitted to it,
/// badged with `icon(app, side)` when that gives one.
pub fn thumbnail(src: &mut dyn Source, size: u32, icon: impl FnOnce(App, u32) -> Option<Rgba>) -> Option<Rgba> {
    let (app, png) = formats::preview(src, size)?;
    let mut img = image::fit(&image::decode_png(&png)?, size);
    let side = image::badge_size(img.width, img.height);
    if side >= 8
        && let Some(b) = icon(app, side)
    {
        image::badge(&mut img, &image::fit(&b, side));
    }
    Some(img)
}
