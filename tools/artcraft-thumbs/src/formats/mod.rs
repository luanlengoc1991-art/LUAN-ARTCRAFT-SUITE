//! What an ArtCraft document shows as its thumbnail, read without the app. The file type is
//! told from the content (Explorer hands over a stream, not a name):
//!
//! | Content | App | Thumbnail |
//! |---|---|---|
//! | ZIP with `manifest.json` | PhotoCraft `.pcraft` | `thumb.png` / `composite/preview.png` ([`zip`]) |
//! | ZIP with `word/document.xml` | WordCraft `.docx` … | the package thumbnail ([`ooxml`]) |
//! | ZIP with `xl/workbook.xml` | GridCraft `.xlsx` … | the package thumbnail, else the first sheet's cells ([`ooxml`]) |
//! | JSON with a `preview` member | VectorCraft, FilmCraft, EffectCraft | that PNG ([`json`]) |
//! | `%PDF-` | PdfCraft `.pdf` | the first page, rendered ([`pdf`]) |
//! | `AC10xx` / DXF text | CADCraft `.dwg` / `.dxf` | the drawing's preview image ([`cad`]) |
//!
//! Files are untrusted: every size read from them is capped, and nothing here panics.

pub mod cad;
pub mod json;
pub mod ooxml;
pub mod pdf;
pub mod zip;

use crate::image::Rgba;

/// Which app a file belongs to (decides the badge).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    PhotoCraft,
    VectorCraft,
    FilmCraft,
    EffectCraft,
    PdfCraft,
    WordCraft,
    GridCraft,
    CadCraft,
}

impl App {
    /// The app's id (its exe is `<id>.exe`).
    pub fn id(self) -> &'static str {
        match self {
            App::PhotoCraft => "photocraft",
            App::VectorCraft => "vectorcraft",
            App::FilmCraft => "filmcraft",
            App::EffectCraft => "effectcraft",
            App::PdfCraft => "pdfcraft",
            App::WordCraft => "wordcraft",
            App::GridCraft => "gridcraft",
            App::CadCraft => "cadcraft",
        }
    }

    /// The app's install folder name (`%ProgramFiles%\<name>`).
    pub fn name(self) -> &'static str {
        match self {
            App::PhotoCraft => "PhotoCraft",
            App::VectorCraft => "VectorCraft",
            App::FilmCraft => "FilmCraft",
            App::EffectCraft => "EffectCraft",
            App::PdfCraft => "PdfCraft",
            App::WordCraft => "WordCraft",
            App::GridCraft => "GridCraft",
            App::CadCraft => "CADCraft",
        }
    }
}

/// What a file's thumbnail is made from.
#[derive(Debug, PartialEq)]
pub enum Found {
    /// An image file's bytes (PNG, JPEG or BMP).
    Image(Vec<u8>),
    /// Pixels already rendered.
    Pixels(Rgba),
    /// Spreadsheet cells, drawn by the caller (it has the fonts).
    Sheet(ooxml::Sheet),
}

/// Random access to a file's bytes.
pub trait Source {
    fn size(&mut self) -> Option<u64>;
    /// Exactly `buf.len()` bytes at `offset`, or `None`.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Option<()>;
}

impl Source for &[u8] {
    fn size(&mut self) -> Option<u64> {
        Some(self.len() as u64)
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Option<()> {
        let start = usize::try_from(offset).ok()?;
        buf.copy_from_slice(self.get(start..start.checked_add(buf.len())?)?);
        Some(())
    }
}

/// `len` bytes at `offset` (capped by the caller).
pub(crate) fn read_vec(src: &mut dyn Source, offset: u64, len: usize) -> Option<Vec<u8>> {
    let mut v = vec![0u8; len];
    src.read_at(offset, &mut v)?;
    Some(v)
}

/// Reads a [`Source`] front to back.
pub(crate) struct SourceReader<'a> {
    pub src: &'a mut dyn Source,
    pub at: u64,
    pub len: u64,
}

impl std::io::Read for SourceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n =
            usize::try_from(self.len.saturating_sub(self.at).min(buf.len() as u64)).unwrap_or(0);
        let Some(out) = buf.get_mut(..n) else {
            return Ok(0);
        };
        if n == 0 {
            return Ok(0);
        }
        self.src
            .read_at(self.at, out)
            .ok_or_else(|| std::io::Error::other("read failed"))?;
        self.at += n as u64;
        Ok(n)
    }
}

/// The app a file belongs to and what its thumbnail is made from, `size` being the pixels on
/// the longer side Explorer asks for.
pub fn preview(src: &mut dyn Source, size: u32) -> Option<(App, Found)> {
    let len = src.size()?;
    let mut head = vec![0u8; usize::try_from(len.min(1024)).ok()?];
    src.read_at(0, &mut head)?;
    if head.starts_with(b"PK\x03\x04") {
        let z = zip::Zip::open(src)?;
        if z.has("word/document.xml") {
            return ooxml::thumbnail(src, &z).map(|img| (App::WordCraft, Found::Image(img)));
        }
        if z.has("xl/workbook.xml") {
            return Some((
                App::GridCraft,
                ooxml::thumbnail(src, &z)
                    .map(Found::Image)
                    .or_else(|| ooxml::sheet(src, &z).map(Found::Sheet))?,
            ));
        }
        let names: &[&str] = if size > 256 {
            &["composite/preview.png", "thumb.png"]
        } else {
            &["thumb.png", "composite/preview.png"]
        };
        let png = names.iter().find_map(|n| z.read(src, n))?;
        return Some((App::PhotoCraft, Found::Image(png)));
    }
    if find(&head, b"%PDF-").is_some() {
        return pdf::first_page(src, size).map(|p| (App::PdfCraft, Found::Pixels(p)));
    }
    if cad::is_dwg(&head) {
        return cad::dwg_preview(src).map(|img| (App::CadCraft, Found::Image(img)));
    }
    if cad::is_dxf(&head) {
        return cad::dxf_preview(src).map(|img| (App::CadCraft, Found::Image(img)));
    }
    json::preview(src, head.starts_with(&[0x1f, 0x8b])).map(|(app, png)| (app, Found::Image(png)))
}

pub(crate) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
