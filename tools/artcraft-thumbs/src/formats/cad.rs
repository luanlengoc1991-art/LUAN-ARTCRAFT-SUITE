//! CAD drawings' own preview images (what CADCraft and AutoCAD store when they save):
//!
//! - DWG (Open Design Alliance's public DWG specification): the file header's preview seeker at
//!   0x0D points at a container: 16-byte sentinel, overall size (u32), image count (u8), then per
//!   image a code (2 = BMP as a DIB, 3 = WMF, 6 = PNG), absolute start and size (u32 each).
//! - DXF (Autodesk DXF Reference, THUMBNAILIMAGE section): group 90 the size, groups 310 the
//!   DIB as hex.
//!
//! A DIB (`BITMAPINFOHEADER`, palette, pixels) is returned as a `.bmp` file.

use super::{Source, find, read_vec};

/// Largest preview image read.
const MAX_IMAGE: usize = 16 << 20;
/// DXF bytes searched from the end for the THUMBNAILIMAGE section (it comes last), then the
/// largest DXF searched whole.
const DXF_TAIL: usize = 8 << 20;
const DXF_WHOLE: u64 = 64 << 20;

pub fn is_dwg(head: &[u8]) -> bool {
    head.len() > 0x11
        && head.starts_with(b"AC10")
        && head
            .get(4..6)
            .is_some_and(|v| v.iter().all(u8::is_ascii_digit))
}

/// ASCII DXF: group code 0 (or a 999 comment) first, then `SECTION`.
pub fn is_dxf(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.lines().map(str::trim);
    match lines.next() {
        Some("0") => lines.next() == Some("SECTION"),
        Some("999") => text.contains("SECTION"),
        _ => false,
    }
}

pub fn dwg_preview(src: &mut dyn Source) -> Option<Vec<u8>> {
    let header = read_vec(src, 0, 0x11)?;
    let seeker = u64::from(u32::from_le_bytes(header.get(0x0D..0x11)?.try_into().ok()?));
    let head = read_vec(src, seeker, 21)?;
    let count = usize::from(*head.get(20)?);
    let descriptors = read_vec(src, seeker + 21, count * 9)?;
    let mut found: Option<(u8, u64, usize)> = None;
    for d in descriptors.chunks_exact(9) {
        let code = d[0];
        let start = u64::from(u32::from_le_bytes(d.get(1..5)?.try_into().ok()?));
        let size = u32::from_le_bytes(d.get(5..9)?.try_into().ok()?) as usize;
        // PNG over BMP; WMF and the header block can't be shown.
        if (code == 6 || (code == 2 && found.is_none())) && size > 0 && size <= MAX_IMAGE {
            found = Some((code, start, size));
        }
    }
    let (code, start, size) = found?;
    let data = read_vec(src, start, size)?;
    if code == 6 {
        Some(data)
    } else {
        dib_to_bmp(&data)
    }
}

pub fn dxf_preview(src: &mut dyn Source) -> Option<Vec<u8>> {
    let len = src.size()?;
    let tail_len = usize::try_from(len.min(DXF_TAIL as u64)).ok()?;
    let tail = read_vec(src, len - tail_len as u64, tail_len)?;
    let text = match find(&tail, b"THUMBNAILIMAGE") {
        Some(at) => tail.get(at..)?.to_vec(),
        None if len <= DXF_WHOLE && tail_len as u64 != len => {
            let whole = read_vec(src, 0, usize::try_from(len).ok()?)?;
            whole.get(find(&whole, b"THUMBNAILIMAGE")?..)?.to_vec()
        }
        None => return None,
    };
    let text = String::from_utf8_lossy(&text);
    let mut lines = text.lines().map(str::trim).skip(1);
    let mut hex = String::new();
    while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
        match code {
            "90" => {}
            "310" => hex.push_str(value),
            _ => break,
        }
        if hex.len() > MAX_IMAGE * 2 {
            return None;
        }
    }
    let dib: Option<Vec<u8>> = hex
        .as_bytes()
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect();
    dib_to_bmp(&dib?)
}

/// A DIB as a `.bmp` file: the 14-byte file header, whose pixel offset counts the info header,
/// the bit masks and the palette.
pub fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |at: usize| Some(u32::from_le_bytes(dib.get(at..at + 4)?.try_into().ok()?));
    let info = u32_at(0)?;
    if !(12..=124).contains(&info) || info as usize > dib.len() {
        return None;
    }
    let (bits, compression, colors) = if info == 12 {
        (
            u32::from(u16::from_le_bytes(dib.get(10..12)?.try_into().ok()?)),
            0,
            0,
        )
    } else {
        (
            u32::from(u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?)),
            u32_at(16)?,
            u32_at(32)?,
        )
    };
    let entry = if info == 12 { 3 } else { 4 };
    let palette = if bits <= 8 {
        (if colors > 0 {
            colors.min(256)
        } else {
            1 << bits
        }) * entry
    } else {
        0
    };
    let masks = if info == 40 && (compression == 3 || compression == 6) {
        if compression == 6 { 16 } else { 12 }
    } else {
        0
    };
    let offset = 14 + info + masks + palette;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&u32::try_from(14 + dib.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×1 24-bit DIB: red, blue.
    fn dib() -> Vec<u8> {
        let mut d = Vec::new();
        for v in [40u32, 2, 1] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0; 24]);
        d.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
        d
    }

    fn decodes_to_red_blue(bmp: &[u8]) {
        let img = image::load_from_memory(bmp).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (2, 1));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(1, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn dxf_thumbnail_section() {
        let hex: String = dib().iter().map(|b| format!("{b:02X}")).collect();
        let (a, b) = hex.split_at(20);
        let dxf = format!(
            "  0\r\nSECTION\r\n  2\r\nHEADER\r\n  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nTHUMBNAILIMAGE\r\n 90\r\n{}\r\n310\r\n{a}\r\n310\r\n{b}\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n",
            dib().len()
        );
        assert!(is_dxf(dxf.as_bytes()));
        let mut src = dxf.as_bytes();
        decodes_to_red_blue(&dxf_preview(&mut src).unwrap());
        let mut none = &b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n"[..];
        assert_eq!(dxf_preview(&mut none), None);
    }

    #[test]
    fn dwg_preview_container() {
        let img = dib();
        let mut f = b"AC1032".to_vec();
        f.resize(0x0D, 0);
        let seeker = 0x40u32;
        f.extend_from_slice(&seeker.to_le_bytes());
        f.resize(seeker as usize, 0);
        f.extend_from_slice(&[0xAA; 16]); // sentinel
        f.extend_from_slice(&0u32.to_le_bytes());
        f.push(2);
        let data_at = seeker as usize + 21 + 18;
        f.push(1);
        f.extend_from_slice(&(data_at as u32).to_le_bytes());
        f.extend_from_slice(&80u32.to_le_bytes());
        f.push(2);
        f.extend_from_slice(&((data_at + 80) as u32).to_le_bytes());
        f.extend_from_slice(&(img.len() as u32).to_le_bytes());
        f.extend_from_slice(&[0; 80]);
        f.extend_from_slice(&img);
        assert!(is_dwg(&f));
        let mut src = f.as_slice();
        decodes_to_red_blue(&dwg_preview(&mut src).unwrap());
        // no images
        f[seeker as usize + 20] = 0;
        let mut src = f.as_slice();
        assert_eq!(dwg_preview(&mut src), None);
    }

    #[test]
    fn junk() {
        assert_eq!(dib_to_bmp(b"\x05\x00\x00\x00"), None);
        assert!(!is_dxf(b"hello"));
        assert!(!is_dwg(b"AC10"));
    }
}
