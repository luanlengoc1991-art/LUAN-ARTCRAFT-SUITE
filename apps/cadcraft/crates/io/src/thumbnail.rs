//! The preview image DXF and DWG files carry for file browsers and Open dialogs: model space
//! fitted to its extents, as a Windows DIB (a `BITMAPINFOHEADER` and 24-bit pixels, without the
//! `.bmp` file header). DXF stores it hex-encoded in a `THUMBNAILIMAGE` section (DXF Reference,
//! "THUMBNAILIMAGE Section"); DWG in the preview container (the DWG bridge's `preview`).

use cadcraft_doc::{Drawing, Space};
use cadcraft_dxf::{Tag, Value};

/// Most pixels on the preview's longer side.
const MAX_SIDE: u32 = 256;
/// Hex digits per group-310 line (the DXF limit is 254).
const HEX_LINE: usize = 254;

/// The drawing's preview as a DIB, `None` when model space is empty.
pub(crate) fn dib(d: &Drawing) -> Option<Vec<u8>> {
    let list = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options { transparency: true, ..cadcraft_render::Options::default() });
    let b = &list.bounds;
    if b.is_empty() {
        return None;
    }
    let (bw, bh) = (b.width(), b.height());
    if !(bw.is_finite() && bh.is_finite()) {
        return None;
    }
    // The image has the drawing's shape (within 4:1 either way).
    let aspect = if bh > 0.0 { (bw / bh).clamp(0.25, 4.0) } else { 4.0 };
    let (w, h) = if aspect >= 1.0 {
        (MAX_SIDE, (f64::from(MAX_SIDE) / aspect).round() as u32)
    } else {
        ((f64::from(MAX_SIDE) * aspect).round() as u32, MAX_SIDE)
    };
    let view = cadcraft_render::raster::View::fit(b, w.max(1), h.max(1), 0.05);
    let pm = cadcraft_render::raster::render(&list, &view, &cadcraft_render::raster::RasterOptions::default())?;
    Some(to_dib(pm.width(), pm.height(), pm.data()))
}

/// Opaque RGBA rows (top first) as a bottom-up 24-bit DIB.
fn to_dib(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(40 + stride * h as usize);
    for v in [40u32, w, h] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((stride * h as usize) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution, palette
    for y in (0..h as usize).rev() {
        let row_start = out.len();
        let row = rgba.get(y * w as usize * 4..(y + 1) * w as usize * 4).unwrap_or_default();
        for p in row.chunks_exact(4) {
            out.extend_from_slice(&[p[2], p[1], p[0]]);
        }
        out.resize(row_start + stride, 0);
    }
    out
}

/// The `THUMBNAILIMAGE` section holding `dib`.
fn section(dib: &[u8]) -> Vec<Tag> {
    let hex: String = dib.iter().map(|b| format!("{b:02X}")).collect();
    let mut tags = vec![Tag::s(0, "SECTION"), Tag::s(2, "THUMBNAILIMAGE"), Tag::i(90, dib.len() as i64)];
    tags.extend(hex.as_bytes().chunks(HEX_LINE).map(|c| Tag::new(310, Value::Hex(String::from_utf8_lossy(c).into_owned()))));
    tags.push(Tag::s(0, "ENDSEC"));
    tags
}

/// `tags` (a whole DXF file) with the drawing's preview before the end of file. Unchanged when
/// there is no preview or no `EOF`.
pub(crate) fn add_to_dxf(d: &Drawing, mut tags: Vec<Tag>) -> Vec<Tag> {
    let eof = tags.iter().rposition(|t| t.code == 0 && matches!(&t.value, Value::Str(s) if s == "EOF"));
    if let (Some(at), Some(dib)) = (eof, dib(d)) {
        tags.splice(at..at, section(&dib));
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_is_bottom_up_24_bit() {
        // 2×1: red, blue
        let dib = to_dib(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 24);
        assert_eq!(&dib[40..48], &[0, 0, 255, 255, 0, 0, 0, 0], "BGR, padded to 4 bytes");
    }

    #[test]
    fn section_splits_hex_into_lines() {
        let tags = section(&[0xAB; 300]);
        let lines: Vec<&Tag> = tags.iter().filter(|t| t.code == 310).collect();
        assert_eq!(lines.len(), 3); // 600 hex digits
        assert!(matches!(&lines[0].value, Value::Hex(h) if h.len() == 254 && h.starts_with("ABAB")));
    }
}
