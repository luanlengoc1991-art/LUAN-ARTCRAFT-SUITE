//! PDF: the first page, rendered with hayro (PdfCraft's renderer, with its patches against
//! hostile files) on white, fitted to the size asked for.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings};

use super::{Source, read_vec};
use crate::image::Rgba;

/// Largest PDF read (the whole file is parsed).
const MAX_FILE: u64 = 512 << 20;

pub fn first_page(src: &mut dyn Source, size: u32) -> Option<Rgba> {
    let len = src.size()?;
    if len > MAX_FILE {
        return None;
    }
    let bytes = read_vec(src, 0, usize::try_from(len).ok()?)?;
    // The renderer is hardened, but a thumbnail must never take its host down.
    catch_unwind(AssertUnwindSafe(|| render(bytes, size)))
        .ok()
        .flatten()
}

fn render(bytes: Vec<u8>, size: u32) -> Option<Rgba> {
    let pdf = Pdf::new(Arc::new(bytes)).ok()?;
    let page = pdf.pages().first()?;
    let (w, h) = page.render_dimensions();
    let longest = w.max(h);
    if !(longest.is_finite() && longest > 0.0) {
        return None;
    }
    let scale = size.clamp(16, 2048) as f32 / longest;
    let rs = RenderSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: WHITE,
        ..Default::default()
    };
    let pm = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &rs,
    );
    let (width, height) = (u32::from(pm.width()), u32::from(pm.height()));
    let pixels = pm
        .take_unpremultiplied()
        .into_iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect();
    Some(Rgba {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page PDF with a filled square, written by hand (offsets computed below).
    pub(crate) fn pdf() -> Vec<u8> {
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>",
            "<< /Length 30 >>\nstream\n1 0 0 rg 0 0 100 100 re f\nendstream",
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![];
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn renders_the_first_page_fitted() {
        let bytes = pdf();
        let mut src = bytes.as_slice();
        let img = first_page(&mut src, 100).unwrap();
        assert_eq!((img.width, img.height), (100, 50));
        let px = |x: u32, y: u32| &img.pixels[((y * img.width + x) * 4) as usize..][..4];
        assert_eq!(px(10, 25), [255, 0, 0, 255], "the red square on the left");
        assert_eq!(px(90, 25), [255, 255, 255, 255], "white paper on the right");
    }

    #[test]
    fn junk_is_not_a_page() {
        let mut src = &b"%PDF-1.4\ngarbage"[..];
        assert!(first_page(&mut src, 100).is_none());
    }
}
