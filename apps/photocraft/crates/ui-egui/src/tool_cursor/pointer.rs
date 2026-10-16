//! Photoshop's arrow pointers as signed distance fields (negative inside), in logical pixels from
//! the hotspot (the arrow's tip) with y pointing down: the Move tool's black arrowhead with the
//! four-way move cross below right, the Path Selection tool's black arrow, the Direct Selection
//! tool's white one, and the arrow with a small marquee for the Object Selection tool and for
//! dragging a selection outline. Black shapes have a white outline, the white arrow a black one, so
//! each reads on any image. They are OS cursor bitmaps, so they move with the hardware pointer.

use std::sync::Arc;

use egui::CustomCursorImage;

use super::hand::capsule;

/// Which pointer, and what it carries beside the arrowhead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pointer {
    /// The Move tool: black arrowhead and the four-way move cross.
    Move,
    /// The Path Selection tool: black arrowhead.
    Black,
    /// The Direct Selection tool: white arrowhead.
    White,
    /// The Object Selection tool, and a press that drags a selection outline: black arrowhead and
    /// a small marquee.
    Marquee,
}

/// The arrowhead, clockwise from its tip at the hotspot.
const ARROW: [[f32; 2]; 4] = [[0.0, 0.0], [11.5, 11.5], [4.8, 11.7], [0.0, 16.3]];

/// The move cross's centre and its arms' reach (to the tips of their heads).
const CROSS: [f32; 2] = [14.5, 18.0];
const REACH: f32 = 5.5;

/// The marquee's centre and half side.
const MARQUEE: [f32; 2] = [15.0, 18.0];
const MARQUEE_HALF: f32 = 4.0;

/// The largest offset of any shape from the hotspot, outline excluded.
const EXTENT: f32 = 23.5;

/// Signed distance to the closed polygon `pts` (negative inside).
fn polygon(x: f32, y: f32, pts: &[[f32; 2]]) -> f32 {
    let mut d = f32::INFINITY;
    let mut inside = false;
    for (&a, &b) in pts.iter().zip(pts.iter().cycle().skip(1)) {
        d = d.min(capsule(x, y, a, b, 0.0));
        // Even-odd crossing test on a ray towards +x.
        if (a[1] > y) != (b[1] > y) && x < a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            inside = !inside;
        }
    }
    if inside { -d } else { d }
}

fn arrow(x: f32, y: f32) -> f32 {
    polygon(x, y, &ARROW)
}

/// The four-way move cross: thin arms with a solid head on each end.
fn cross(x: f32, y: f32) -> f32 {
    let (x, y) = (x - CROSS[0], y - CROSS[1]);
    let mut d = capsule(x, y, [-REACH + 1.5, 0.0], [REACH - 1.5, 0.0], 0.55).min(capsule(x, y, [0.0, -REACH + 1.5], [0.0, REACH - 1.5], 0.55));
    let (tip, base, half) = (REACH, REACH - 2.6, 2.1);
    for (sx, sy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let at = |along: f32, side: f32| [sx * along - sy * side, sy * along + sx * side];
        d = d.min(polygon(x, y, &[at(tip, 0.0), at(base, half), at(base, -half)]));
    }
    d
}

/// The marquee: a square outline 1 px wide, broken into dashes.
fn marquee(x: f32, y: f32) -> f32 {
    let (x, y) = (x - MARQUEE[0], y - MARQUEE[1]);
    let ring = ((x.abs().max(y.abs())) - MARQUEE_HALF).abs() - 0.6;
    // Gaps in the middle of each side, as a marquee's marching ants.
    let along = if x.abs() > y.abs() { y } else { x };
    if along.abs() < 1.0 { ring.max(1.0 - along.abs()) } else { ring }
}

/// The black shapes of `p` as one distance field (none for the white arrow).
fn dark(p: Pointer, x: f32, y: f32) -> f32 {
    match p {
        Pointer::Move => arrow(x, y).min(cross(x, y)),
        Pointer::Black => arrow(x, y),
        Pointer::White => f32::INFINITY,
        Pointer::Marquee => arrow(x, y).min(marquee(x, y)),
    }
}

/// Coverage (light, dark), each 0 to 1, of the pixel `(lx, ly)` logical pixels from the hotspot.
/// Black shapes get a 1 px white outline; the white arrow a 1 px black one.
fn coverage(p: Pointer, lx: f32, ly: f32, scale: f32) -> (f32, f32) {
    let fill = |d: f32| (0.5 - d * scale).clamp(0.0, 1.0);
    let ring = |d: f32| (scale + 0.5 - d * scale).clamp(0.0, 1.0);
    if p == Pointer::White {
        let d = arrow(lx, ly);
        let light = fill(d);
        return (light, ring(d) * (1.0 - light));
    }
    let d = dark(p, lx, ly);
    let black = fill(d);
    (ring(d) * (1.0 - black), black)
}

/// White over black, in straight RGBA (winit form).
fn pixel(light: f32, dark: f32) -> [u8; 4] {
    let alpha = light + dark * (1.0 - light);
    let white = if alpha > 0.0 { (255.0 * light / alpha).round() as u8 } else { 0 };
    [white, white, white, (255.0 * alpha).round() as u8]
}

/// The pointer's bitmap at `scale` bitmap pixels per point, its hotspot on the arrow's tip.
pub(super) fn rasterize(p: Pointer, scale: f32) -> Option<CustomCursorImage> {
    if !scale.is_finite() || scale <= 0.0 || scale > 16.0 {
        return None;
    }
    // Room for the outline and its antialiasing up and left of the tip.
    let pad = (1.5 * scale + 1.0).ceil();
    let side = ((EXTENT + 1.5) * scale + pad + 1.0).ceil();
    if !side.is_finite() || side > f32::from(super::MAX_SIDE) {
        return None;
    }
    let n = side as usize;
    let mut rgba = vec![0u8; n.checked_mul(n)?.checked_mul(4)?];
    for row in 0..n {
        for col in 0..n {
            let (lx, ly) = ((col as f32 - pad) / scale, (row as f32 - pad) / scale);
            let (light, dark) = coverage(p, lx, ly, scale);
            if let Some(px) = rgba.get_mut((row * n + col) * 4..(row * n + col) * 4 + 4) {
                px.copy_from_slice(&pixel(light, dark));
            }
        }
    }
    let (side, hot) = (side as u16, pad as u16);
    Some(CustomCursorImage { rgba: Arc::from(rgba), size: [side, side], hotspot: [hot, hot] })
}

/// Where the pointer can't be an OS bitmap: the platform cursor closest to it.
pub(super) fn fallback(p: Pointer) -> egui::CursorIcon {
    match p {
        Pointer::Move => egui::CursorIcon::Move,
        _ => egui::CursorIcon::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(image: &CustomCursorImage, x: usize, y: usize) -> u8 {
        let n = usize::from(image.size[0]);
        image.rgba[(y * n + x) * 4 + 3]
    }

    fn rgb(image: &CustomCursorImage, x: usize, y: usize) -> u8 {
        let n = usize::from(image.size[0]);
        image.rgba[(y * n + x) * 4]
    }

    #[test]
    fn the_hotspot_is_the_arrow_tip_at_every_scale() {
        for p in [Pointer::Move, Pointer::Black, Pointer::White, Pointer::Marquee] {
            for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
                let image = rasterize(p, scale).unwrap();
                let [hx, hy] = image.hotspot.map(usize::from);
                assert_eq!(image.rgba.len(), usize::from(image.size[0]) * usize::from(image.size[1]) * 4);
                assert!(alpha(&image, hx, hy) > 0, "{p:?} {scale}: the tip is drawn");
                // Nothing above or left of the tip but its outline.
                assert_eq!(alpha(&image, 0, 0), 0, "{p:?} {scale}");
            }
        }
    }

    #[test]
    fn black_arrows_are_black_inside_with_a_white_outline_and_the_white_arrow_is_the_reverse() {
        let s = 1.0;
        let inside = |p| {
            let image = rasterize(p, s).unwrap();
            let h = usize::from(image.hotspot[0]);
            (rgb(&image, h + 2, h + 6), alpha(&image, h + 2, h + 6), rgb(&image, h - 1, h + 6))
        };
        let (fill, a, edge) = inside(Pointer::Black);
        assert_eq!((fill, a), (0, 255), "black fill");
        assert!(edge > 200, "white outline left of the arrow: {edge}");
        let (fill, a, edge) = inside(Pointer::White);
        assert_eq!((fill, a), (255, 255), "white fill");
        assert!(edge < 60, "black outline: {edge}");
    }

    #[test]
    fn the_move_pointer_carries_its_cross_and_the_others_do_not() {
        let at = |p| {
            let image = rasterize(p, 1.0).unwrap();
            let h = usize::from(image.hotspot[0]);
            alpha(&image, h + CROSS[0] as usize, h + CROSS[1] as usize)
        };
        assert_eq!(at(Pointer::Move), 255);
        assert_eq!(at(Pointer::Black), 0);
        assert_eq!(at(Pointer::White), 0);
    }

    /// Run explicitly for visual QA: native cursors are absent from GPU screenshots.
    #[test]
    #[ignore = "writes target/pointer-cursor-preview.png for visual inspection"]
    fn write_pointer_preview() {
        let cases = [Pointer::Move, Pointer::Black, Pointer::White, Pointer::Marquee];
        let (width, height) = (3 * 160_usize, 2 * 80_usize);
        let mut rgba = vec![255; width * height * 4];
        for (col, background) in [0_u8, 128, 255].into_iter().enumerate() {
            for y in 0..height {
                for x in col * 160..(col + 1) * 160 {
                    rgba[(y * width + x) * 4..(y * width + x) * 4 + 3].fill(background);
                }
            }
            for (i, p) in cases.into_iter().enumerate() {
                let scale = if i % 2 == 0 { 1.0 } else { 2.0 };
                let image = rasterize(p, scale).unwrap();
                let side = usize::from(image.size[0]);
                let (x0, y0) = (col * 160 + (i % 2) * 80 + 10, (i / 2) * 80 + 10);
                for (k, pixel) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
                    let at = ((y0 + k / side) * width + x0 + k % side) * 4;
                    let a = f32::from(pixel[3]) / 255.0;
                    let value = (f32::from(pixel[0]) * a + f32::from(background) * (1.0 - a)).round() as u8;
                    rgba[at..at + 3].fill(value);
                }
            }
        }
        let image = photocraft_codecs::Image::from_u8(width as u32, height as u32, photocraft_codecs::ChannelLayout::Rgba, rgba).unwrap();
        let png = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pointer-cursor-preview.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, png).unwrap();
    }

    #[test]
    fn hostile_scales_give_no_bitmap() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 1e9] {
            assert!(rasterize(Pointer::Move, scale).is_none(), "{scale}");
        }
    }
}
