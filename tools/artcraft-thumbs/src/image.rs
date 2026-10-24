//! Decoding the preview, fitting it to the size Explorer asks for, and badging it.

/// Straight-alpha RGBA, 8 bits a channel, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Longest side of a preview decoded (PhotoCraft writes 1024 at most).
const MAX_SIDE: u32 = 8192;

/// A PNG as RGBA (any colour type and bit depth).
pub fn decode_png(bytes: &[u8]) -> Option<Rgba> {
    let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().ok()?;
    let (w, h) = (r.info().width, r.info().height);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return None;
    }
    let mut buf = vec![0u8; r.output_buffer_size()?];
    let frame = r.next_frame(&mut buf).ok()?;
    let data = buf.get(..frame.buffer_size())?;
    let n = (w * h) as usize;
    let pixels: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data.get(..n * 4)?.to_vec(),
        png::ColorType::Rgb => data.get(..n * 3)?.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.get(..n * 2)?.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => data.get(..n)?.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(Rgba { width: w, height: h, pixels })
}

/// `img` fitted into `size`×`size` (aspect kept; never enlarged), by area averaging in
/// premultiplied alpha.
pub fn fit(img: &Rgba, size: u32) -> Rgba {
    let longest = img.width.max(img.height);
    if longest <= size || size == 0 {
        return img.clone();
    }
    let scale = f64::from(size) / f64::from(longest);
    let w = ((f64::from(img.width) * scale).round() as u32).max(1);
    let h = ((f64::from(img.height) * scale).round() as u32).max(1);
    let (sx, sy) = (f64::from(img.width) / f64::from(w), f64::from(img.height) / f64::from(h));
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let (y0, y1) = (f64::from(y) * sy, f64::from(y + 1) * sy);
        for x in 0..w {
            let (x0, x1) = (f64::from(x) * sx, f64::from(x + 1) * sx);
            let mut acc = [0f64; 4];
            let mut area = 0f64;
            let mut yy = y0.floor() as u32;
            while f64::from(yy) < y1 && yy < img.height {
                let wy = (y1.min(f64::from(yy + 1)) - y0.max(f64::from(yy))).max(0.0);
                let mut xx = x0.floor() as u32;
                while f64::from(xx) < x1 && xx < img.width {
                    let wx = (x1.min(f64::from(xx + 1)) - x0.max(f64::from(xx))).max(0.0);
                    let i = ((yy * img.width + xx) * 4) as usize;
                    if let Some(p) = img.pixels.get(i..i + 4) {
                        let a = f64::from(p[3]) / 255.0;
                        let k = wx * wy;
                        acc[0] += f64::from(p[0]) * a * k;
                        acc[1] += f64::from(p[1]) * a * k;
                        acc[2] += f64::from(p[2]) * a * k;
                        acc[3] += a * k;
                    }
                    area += wx * wy;
                    xx += 1;
                }
                yy += 1;
            }
            let o = ((y * w + x) * 4) as usize;
            if area > 0.0 && acc[3] > 0.0 {
                out[o] = (acc[0] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[o + 1] = (acc[1] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[o + 2] = (acc[2] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[o + 3] = (acc[3] / area * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Rgba { width: w, height: h, pixels: out }
}

/// The badge's side for a thumbnail of `w`×`h`: about a fifth of the longer side, 16 to 64
/// pixels, never more than half the shorter side.
pub fn badge_size(w: u32, h: u32) -> u32 {
    let s = (w.max(h) as f64 * 0.2).round() as u32;
    s.clamp(16, 64).min(w.min(h) / 2)
}

/// `badge` drawn over the bottom-right corner of `img` (source-over, straight alpha).
pub fn badge(img: &mut Rgba, badge: &Rgba) {
    let margin = (img.width.max(img.height) / 64).max(1);
    let (Some(bx), Some(by)) = (img.width.checked_sub(badge.width + margin), img.height.checked_sub(badge.height + margin)) else { return };
    for y in 0..badge.height {
        for x in 0..badge.width {
            let s = ((y * badge.width + x) * 4) as usize;
            let d = (((by + y) * img.width + bx + x) * 4) as usize;
            let (Some(sp), Some(dp)) = (badge.pixels.get(s..s + 4), img.pixels.get(d..d + 4)) else { continue };
            let sa = f32::from(sp[3]) / 255.0;
            if sa <= 0.0 {
                continue;
            }
            let da = f32::from(dp[3]) / 255.0;
            let oa = sa + da * (1.0 - sa);
            let mut px = [0u8; 4];
            for c in 0..3 {
                let v = (f32::from(sp[c]) * sa + f32::from(dp[c]) * da * (1.0 - sa)) / oa;
                px[c] = v.round().clamp(0.0, 255.0) as u8;
            }
            px[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
            img.pixels[d..d + 4].copy_from_slice(&px);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, p: [u8; 4]) -> Rgba {
        Rgba { width: w, height: h, pixels: p.repeat((w * h) as usize) }
    }

    #[test]
    fn fit_keeps_aspect_and_never_enlarges() {
        let img = solid(1024, 512, [10, 20, 30, 255]);
        let f = fit(&img, 256);
        assert_eq!((f.width, f.height), (256, 128));
        assert_eq!(&f.pixels[..4], &[10, 20, 30, 255]);
        assert_eq!(fit(&solid(40, 30, [0; 4]), 256).width, 40);
    }

    #[test]
    fn fit_averages_in_premultiplied_alpha() {
        // Red opaque next to transparent green: the colour stays red, half covered.
        let img = Rgba { width: 2, height: 1, pixels: vec![255, 0, 0, 255, 0, 255, 0, 0] };
        let f = fit(&img, 1);
        assert_eq!(f.pixels, vec![255, 0, 0, 128]);
    }

    #[test]
    fn badge_goes_bottom_right() {
        let mut img = solid(100, 100, [0, 0, 0, 255]);
        badge(&mut img, &solid(20, 20, [255, 255, 255, 255]));
        let at = |x: u32, y: u32| img.pixels[((y * 100 + x) * 4) as usize];
        assert_eq!((at(97, 97), at(70, 70), at(5, 5)), (255, 0, 0));
        assert_eq!(badge_size(256, 256), 51);
        assert_eq!(badge_size(256, 20), 10);
    }

    #[test]
    fn decodes_png() {
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 2, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&[1, 2, 3, 4, 5, 6]).unwrap();
        }
        let img = decode_png(&bytes).unwrap();
        assert_eq!(img.pixels, vec![1, 2, 3, 255, 4, 5, 6, 255]);
        assert!(decode_png(b"\x89PNG nope").is_none());
    }
}
