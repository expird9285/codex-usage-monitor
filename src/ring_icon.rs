//! Software-rendered tray icon: a split ring gauge with a rounded diamond in
//! the middle. The upper half of the ring shows the 5-hour quota remaining,
//! the lower half the weekly quota remaining. Rendered with 4x4 supersampling
//! so it stays crisp at 16/20/24/32 px.

use std::f64::consts::PI;

/// Remaining fractions (0.0..=1.0); `None` draws only the empty track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingValues {
    pub session: Option<f64>,
    pub weekly: Option<f64>,
}

#[derive(Clone, Copy)]
struct Rgba {
    r: f64,
    g: f64,
    b: f64,
    a: f64,
}

const fn rgba(r: u8, g: u8, b: u8, a: f64) -> Rgba {
    Rgba {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a,
    }
}

struct Palette {
    fill: Rgba,
    track: Rgba,
    diamond: Rgba,
}

fn palette(dark_taskbar: bool) -> Palette {
    if dark_taskbar {
        Palette {
            fill: rgba(0xD2, 0xD2, 0xD2, 1.0),
            track: rgba(0xFF, 0xFF, 0xFF, 0.20),
            diamond: rgba(0x9C, 0x9C, 0x9C, 1.0),
        }
    } else {
        Palette {
            fill: rgba(0x3A, 0x3A, 0x3A, 1.0),
            track: rgba(0x00, 0x00, 0x00, 0.18),
            diamond: rgba(0x70, 0x70, 0x70, 1.0),
        }
    }
}

struct Geometry {
    c: f64,
    ring_mid: f64,
    half_w: f64,
    half_gap: f64,
    diamond_half_side: f64,
    diamond_radius: f64,
}

fn geometry(size: u32) -> Geometry {
    let s = size as f64;
    let half_w = (s * 0.065).max(0.95);
    let outer = s * 0.5 - 0.35;
    let ring_mid = outer - half_w;
    // Gap between the two halves at 3 and 9 o'clock: room for both round caps
    // plus a visible sliver of empty space.
    let gap_px = 2.0 * half_w + (s * 0.07).max(1.1);
    let half_gap = (gap_px / ring_mid) / 2.0;
    let diamond_half_diag = s * 0.23;
    let diamond_half_side = diamond_half_diag / 2f64.sqrt();
    Geometry {
        c: s / 2.0,
        ring_mid,
        half_w,
        half_gap,
        diamond_half_side,
        diamond_radius: diamond_half_side * 0.32,
    }
}

/// Angle measured clockwise from 12 o'clock, in [0, 2π).
fn clock_angle(dx: f64, dy: f64) -> f64 {
    let a = dx.atan2(-dy);
    if a < 0.0 {
        a + 2.0 * PI
    } else {
        a
    }
}

/// Is (dx, dy) on a round-capped arc starting at `start` running clockwise
/// for `len` radians?
fn on_arc(g: &Geometry, dx: f64, dy: f64, start: f64, len: f64) -> bool {
    if len <= 0.0 {
        return false;
    }
    let r = (dx * dx + dy * dy).sqrt();
    let rel = (clock_angle(dx, dy) - start).rem_euclid(2.0 * PI);
    if rel <= len {
        return (r - g.ring_mid).abs() <= g.half_w;
    }
    let cap = |angle: f64| {
        let px = g.ring_mid * angle.sin();
        let py = -g.ring_mid * angle.cos();
        ((dx - px).powi(2) + (dy - py).powi(2)).sqrt() <= g.half_w
    };
    cap(start) || cap(start + len)
}

fn in_diamond(g: &Geometry, dx: f64, dy: f64) -> bool {
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let x = ((dx + dy) * k).abs();
    let y = ((dy - dx) * k).abs();
    let inner = g.diamond_half_side - g.diamond_radius;
    let qx = x - inner;
    let qy = y - inner;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - g.diamond_radius <= 0.0
}

fn sample(g: &Geometry, pal: &Palette, values: RingValues, x: f64, y: f64) -> Option<Rgba> {
    let dx = x - g.c;
    let dy = y - g.c;
    if in_diamond(g, dx, dy) {
        return Some(pal.diamond);
    }
    let half_len = PI - 2.0 * g.half_gap;
    // Upper half: 9 o'clock -> 12 -> 3 o'clock. Lower half: 3 -> 6 -> 9.
    for (start, value) in [
        (1.5 * PI + g.half_gap, values.session),
        (0.5 * PI + g.half_gap, values.weekly),
    ] {
        let fill_len = value.map_or(0.0, |v| v.clamp(0.0, 1.0) * half_len);
        if fill_len > 0.0 && on_arc(g, dx, dy, start, fill_len) {
            return Some(pal.fill);
        }
        if on_arc(g, dx, dy, start, half_len) {
            return Some(pal.track);
        }
    }
    None
}

/// Render a `size`x`size` icon as top-down BGRA with straight (non-premultiplied)
/// alpha, the format `CreateIconIndirect` expects for 32-bit icons.
pub fn render_bgra(size: u32, values: RingValues, dark_taskbar: bool) -> Vec<u8> {
    const SS: u32 = 4;
    let g = geometry(size);
    let pal = palette(dark_taskbar);
    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let (mut r, mut gg, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f64 + (sx as f64 + 0.5) / SS as f64;
                    let y = py as f64 + (sy as f64 + 0.5) / SS as f64;
                    if let Some(c) = sample(&g, &pal, values, x, y) {
                        r += c.r * c.a;
                        gg += c.g * c.a;
                        b += c.b * c.a;
                        a += c.a;
                    }
                }
            }
            let n = (SS * SS) as f64;
            let i = ((py * size + px) * 4) as usize;
            if a > 0.0 {
                out[i] = (b / a).round() as u8;
                out[i + 1] = (gg / a).round() as u8;
                out[i + 2] = (r / a).round() as u8;
                out[i + 3] = (a / n * 255.0).round().min(255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_and_empty_halves_differ() {
        let full = render_bgra(
            16,
            RingValues {
                session: Some(1.0),
                weekly: Some(1.0),
            },
            true,
        );
        let empty = render_bgra(
            16,
            RingValues {
                session: Some(0.0),
                weekly: None,
            },
            true,
        );
        assert_ne!(full, empty);
        // The centre pixel is always the diamond.
        let i = ((8 * 16 + 8) * 4) as usize;
        assert_eq!(full[i + 3], 255);
    }

    /// `cargo test write_ring_previews -- --ignored` writes enlarged BMP
    /// previews to RING_PREVIEW_DIR (default: target/ring-previews).
    #[test]
    #[ignore]
    fn write_ring_previews() {
        let dir = std::env::var("RING_PREVIEW_DIR")
            .unwrap_or_else(|_| "target/ring-previews".to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let cases = [
            ("85-77", Some(0.85), Some(0.77)),
            ("40-100", Some(0.40), Some(1.0)),
            ("10-55", Some(0.10), Some(0.55)),
            ("none", None, None),
        ];
        for dark in [true, false] {
            let bg: [f64; 3] = if dark { [32.0; 3] } else { [238.0; 3] };
            let sizes = [16u32, 20, 24, 32];
            let scale = 8u32;
            let cell = 32 * scale + 16;
            let width = cell * sizes.len() as u32;
            let height = cell * cases.len() as u32;
            let mut img = vec![0u8; (width * height * 3) as usize];
            for p in img.chunks_mut(3) {
                p.copy_from_slice(&[bg[0] as u8; 3]);
            }
            for (row, (_, s, w)) in cases.iter().enumerate() {
                for (col, &size) in sizes.iter().enumerate() {
                    let px = render_bgra(
                        size,
                        RingValues {
                            session: *s,
                            weekly: *w,
                        },
                        dark,
                    );
                    for y in 0..size * scale {
                        for x in 0..size * scale {
                            let si = (((y / scale) * size + x / scale) * 4) as usize;
                            let a = px[si + 3] as f64 / 255.0;
                            let ox = col as u32 * cell + 8 + x;
                            let oy = row as u32 * cell + 8 + y;
                            let di = ((oy * width + ox) * 3) as usize;
                            for ch in 0..3 {
                                img[di + ch] =
                                    (px[si + ch] as f64 * a + bg[ch] * (1.0 - a)).round() as u8;
                            }
                        }
                    }
                }
            }
            let name = format!("{dir}/ring-{}.bmp", if dark { "dark" } else { "light" });
            write_bmp(&name, width, height, &img);
        }
    }

    fn write_bmp(path: &str, w: u32, h: u32, bgr: &[u8]) {
        let row = (w * 3 + 3) & !3;
        let size = 54 + row * h;
        let mut f = Vec::with_capacity(size as usize);
        f.extend_from_slice(b"BM");
        f.extend_from_slice(&size.to_le_bytes());
        f.extend_from_slice(&[0; 4]);
        f.extend_from_slice(&54u32.to_le_bytes());
        f.extend_from_slice(&40u32.to_le_bytes());
        f.extend_from_slice(&(w as i32).to_le_bytes());
        f.extend_from_slice(&(-(h as i32)).to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&24u16.to_le_bytes());
        f.extend_from_slice(&[0; 24]);
        for y in 0..h {
            let start = (y * w * 3) as usize;
            f.extend_from_slice(&bgr[start..start + (w * 3) as usize]);
            f.resize(f.len() + (row - w * 3) as usize, 0);
        }
        std::fs::write(path, f).unwrap();
    }
}
