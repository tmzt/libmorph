//! CPU pixel conversions for the camera, BT.601 full range (as camera
//! YUV_420_888 is): YUV to RGB in the same 16.16 fixed point as libmobile's
//! `camera::convert`, RGB to YUV for frames made from RGBA (tests, scripted
//! runs), and the viewfinder's turn and fit geometry, which its shader
//! repeats.

/// 16.16 fixed-point BT.601 full-range coefficients (libmobile's).
const R_V: i32 = 91_881; // 1.402
const G_U: i32 = 22_554; // 0.344136
const G_V: i32 = 46_802; // 0.714136
const B_U: i32 = 116_130; // 1.772
const HALF: i32 = 1 << 15;

/// One pixel: `r = y + 1.402 (v−128)`, `g = y − 0.344136 (u−128) −
/// 0.714136 (v−128)`, `b = y + 1.772 (u−128)`.
pub(crate) fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let y = i32::from(y) << 16;
    let u = i32::from(u) - 128;
    let v = i32::from(v) - 128;
    let clamp = |x: i32| ((x + HALF) >> 16).clamp(0, 255) as u8;
    [
        clamp(y + R_V * v),
        clamp(y - G_U * u - G_V * v),
        clamp(y + B_U * u),
    ]
}

/// The viewfinder shader's conversion (`viewfinder.wgsl`, `yuv_to_rgb`),
/// in floats: texel values 0.0..=1.0 in, RGB 0.0..=1.0 out (gamma-encoded,
/// as the camera's are).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn yuv_to_rgb_unit(y: f32, u: f32, v: f32) -> [f32; 3] {
    let u = u - 128.0 / 255.0;
    let v = v - 128.0 / 255.0;
    [
        (y + 1.402 * v).clamp(0.0, 1.0),
        (y - 0.344_136 * u - 0.714_136 * v).clamp(0.0, 1.0),
        (y + 1.772 * u).clamp(0.0, 1.0),
    ]
}

/// One pixel's luma.
pub(crate) fn luma(r: u8, g: u8, b: u8) -> u8 {
    let y = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    y.round().clamp(0.0, 255.0) as u8
}

/// One pixel's chroma, `(u, v)`.
fn chroma(r: f32, g: f32, b: f32) -> (u8, u8) {
    let u = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
    let v = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
    let q = |x: f32| x.round().clamp(0.0, 255.0) as u8;
    (q(u), q(v))
}

/// Packed I420 planes as RGBA rows, alpha 255; chroma nearest (the sample
/// of each 2 × 2 block), as libmobile's `to_rgba`.
pub(crate) fn i420_to_rgba(width: usize, height: usize, y: &[u8], u: &[u8], v: &[u8]) -> Vec<u8> {
    let cw = width.div_ceil(2);
    let mut out = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for x in 0..width {
            let i = (row / 2) * cw + x / 2;
            let (cu, cv) = (
                u.get(i).copied().unwrap_or(128),
                v.get(i).copied().unwrap_or(128),
            );
            let [r, g, b] = yuv_to_rgb(y.get(row * width + x).copied().unwrap_or(0), cu, cv);
            out.extend_from_slice(&[r, g, b, 255]);
        }
    }
    out
}

/// RGBA rows as packed I420: luma per pixel, chroma from the average RGB
/// of each 2 × 2 block (the pixels that exist, at odd edges).
pub(crate) fn rgba_to_i420(
    width: usize,
    height: usize,
    rgba: &[u8],
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let px = |x: usize, y: usize| {
        let i = (y * width + x) * 4;
        (rgba[i], rgba[i + 1], rgba[i + 2])
    };
    let mut luma_plane = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = px(x, y);
            luma_plane.push(luma(r, g, b));
        }
    }
    let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
    let (mut u, mut v) = (Vec::with_capacity(cw * ch), Vec::with_capacity(cw * ch));
    for cy in 0..ch {
        for cx in 0..cw {
            let (mut sum, mut n) = ([0.0f32; 3], 0.0f32);
            for y in (cy * 2)..(cy * 2 + 2).min(height) {
                for x in (cx * 2)..(cx * 2 + 2).min(width) {
                    let (r, g, b) = px(x, y);
                    sum[0] += f32::from(r);
                    sum[1] += f32::from(g);
                    sum[2] += f32::from(b);
                    n += 1.0;
                }
            }
            let (cu, cv) = chroma(sum[0] / n, sum[1] / n, sum[2] / n);
            u.push(cu);
            v.push(cv);
        }
    }
    (luma_plane, u, v)
}

/// Where a point of the upright image comes from in the frame as stored,
/// both as fractions 0.0..=1.0, for a frame needing `turns` clockwise
/// quarter turns (the shader's `source_uv`).
#[cfg_attr(not(test), allow(dead_code))] // The shader repeats it; tested here.
pub(crate) fn source_uv(turns: u8, (u, v): (f32, f32)) -> (f32, f32) {
    match turns % 4 {
        0 => (u, v),
        1 => (v, 1.0 - u),
        2 => (1.0 - u, 1.0 - v),
        _ => (1.0 - v, u),
    }
}

/// How the image fills the viewfinder's rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fit {
    /// Fill the rectangle, cropping the image's longer side (a camera
    /// app's viewfinder).
    #[default]
    Cover,
    /// Show the whole image inside the rectangle, centred; the rest of the
    /// rectangle is not drawn.
    Contain,
    /// Fill the rectangle, ignoring the image's aspect.
    Stretch,
}

/// The viewport to draw into (`[x, y, width, height]`, pixels) and the
/// part of the upright image shown there (`[scale_u, scale_v, offset_u,
/// offset_v]`: `uv' = uv × scale + offset`).
pub(crate) fn layout(rect: [f32; 4], image: (u32, u32), fit: Fit) -> ([f32; 4], [f32; 4]) {
    let [x, y, w, h] = rect;
    let full = [1.0, 1.0, 0.0, 0.0];
    if w <= 0.0 || h <= 0.0 || image.0 == 0 || image.1 == 0 {
        return (rect, full);
    }
    let rect_aspect = w / h;
    let image_aspect = image.0 as f32 / image.1 as f32;
    match fit {
        Fit::Stretch => (rect, full),
        Fit::Cover if image_aspect > rect_aspect => {
            let s = rect_aspect / image_aspect;
            (rect, [s, 1.0, (1.0 - s) / 2.0, 0.0])
        }
        Fit::Cover => {
            let s = image_aspect / rect_aspect;
            (rect, [1.0, s, 0.0, (1.0 - s) / 2.0])
        }
        Fit::Contain if image_aspect > rect_aspect => {
            let inner = w / image_aspect;
            ([x, y + (h - inner) / 2.0, w, inner], full)
        }
        Fit::Contain => {
            let inner = h * image_aspect;
            ([x + (w - inner) / 2.0, y, inner, h], full)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BT.601 full-range primaries, from the forward matrix.
    const KNOWN: [([u8; 3], [u8; 3]); 6] = [
        ([0, 128, 128], [0, 0, 0]),
        ([255, 128, 128], [255, 255, 255]),
        ([128, 128, 128], [128, 128, 128]),
        // Red: y = 76.245, u = 84.972, v = 255.5.
        ([76, 85, 255], [254, 0, 0]),
        // Green: y = 149.685, u = 43.527, v = 21.234.
        ([150, 44, 21], [0, 255, 1]),
        // Blue: y = 29.07, u = 255.5, v = 107.27.
        ([29, 255, 107], [0, 0, 254]),
    ];

    #[test]
    fn known_values() {
        for (yuv, rgb) in KNOWN {
            let got = yuv_to_rgb(yuv[0], yuv[1], yuv[2]);
            for c in 0..3 {
                assert!(
                    got[c].abs_diff(rgb[c]) <= 1,
                    "{yuv:?} -> {got:?}, want {rgb:?}"
                );
            }
        }
    }

    /// The shader's float conversion matches the fixed point within one
    /// step, over a grid of the whole cube.
    #[test]
    fn shader_math_matches_the_fixed_point() {
        for y in (0..=255u8).step_by(15) {
            for u in (0..=255u8).step_by(15) {
                for v in (0..=255u8).step_by(15) {
                    let fixed = yuv_to_rgb(y, u, v);
                    let unit = yuv_to_rgb_unit(
                        f32::from(y) / 255.0,
                        f32::from(u) / 255.0,
                        f32::from(v) / 255.0,
                    );
                    for c in 0..3 {
                        let float = (unit[c] * 255.0).round() as u8;
                        assert!(
                            float.abs_diff(fixed[c]) <= 1,
                            "({y}, {u}, {v}) channel {c}: {float} vs {fixed:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn rgb_round_trips_through_i420() {
        // A 2 × 2 block per colour: chroma is exact.
        let colours = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255], [200, 120, 40]];
        let (w, h) = (4, 4);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let c = colours[(y / 2) * 2 + x / 2];
                rgba[(y * w + x) * 4..][..4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let (py, pu, pv) = rgba_to_i420(w, h, &rgba);
        assert_eq!((py.len(), pu.len(), pv.len()), (16, 4, 4));
        let back = i420_to_rgba(w, h, &py, &pu, &pv);
        for (a, b) in rgba.iter().zip(&back) {
            assert!(a.abs_diff(*b) <= 2, "{a} vs {b}");
        }
    }

    #[test]
    fn odd_sizes_round_chroma_up() {
        let rgba = [255u8, 255, 255, 255].repeat(3);
        let (y, u, v) = rgba_to_i420(3, 1, &rgba);
        assert_eq!((y, u, v), (vec![255; 3], vec![128; 2], vec![128; 2]));
    }

    /// A clockwise turn moves the stored top-left to the upright top-right,
    /// and the four turns are each other's inverses in order.
    #[test]
    fn turns_follow_clockwise() {
        assert_eq!(source_uv(1, (1.0, 0.0)), (0.0, 0.0));
        assert_eq!(source_uv(1, (0.0, 0.0)), (0.0, 1.0));
        assert_eq!(source_uv(2, (0.0, 0.0)), (1.0, 1.0));
        assert_eq!(source_uv(3, (0.0, 0.0)), (1.0, 0.0));
        assert_eq!(source_uv(4, (0.25, 0.75)), (0.25, 0.75));
        // Turning a 3 × 2 image once: stored (x, y) lands at (H-1-y, x).
        let (w, h) = (3.0f32, 2.0f32);
        for (sx, sy) in [(0.0f32, 0.0f32), (2.0, 0.0), (1.0, 1.0)] {
            let (ux, uy) = (h - 1.0 - sy, sx);
            // Pixel centres as fractions of the upright (2 × 3) image.
            let uv = ((ux + 0.5) / h, (uy + 0.5) / w);
            let (su, sv) = source_uv(1, uv);
            assert_eq!(((su * w - 0.5).round(), (sv * h - 0.5).round()), (sx, sy));
        }
    }

    #[test]
    fn layout_fits() {
        // A 640 × 480 image in a 100 × 100 square.
        let rect = [10.0, 20.0, 100.0, 100.0];
        let (view, crop) = layout(rect, (640, 480), Fit::Cover);
        assert_eq!(view, rect);
        assert!((crop[0] - 0.75).abs() < 1e-6 && crop[1] == 1.0);
        assert!((crop[2] - 0.125).abs() < 1e-6 && crop[3] == 0.0);
        let (view, crop) = layout(rect, (640, 480), Fit::Contain);
        assert_eq!(view, [10.0, 32.5, 100.0, 75.0]);
        assert_eq!(crop, [1.0, 1.0, 0.0, 0.0]);
        // Tall image: crop and letterbox the other way.
        let (_, crop) = layout(rect, (480, 640), Fit::Cover);
        assert!((crop[1] - 0.75).abs() < 1e-6 && (crop[3] - 0.125).abs() < 1e-6);
        let (view, _) = layout(rect, (480, 640), Fit::Contain);
        assert_eq!(view, [22.5, 20.0, 75.0, 100.0]);
        assert_eq!(layout(rect, (640, 480), Fit::Stretch).0, rect);
    }
}
