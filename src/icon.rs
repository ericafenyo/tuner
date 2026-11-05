//! App icon: the two-arc logo from the top bar on a charcoal rounded square,
//! rendered in code so it stays sharp at any size.

use eframe::egui::IconData;

/// Arcs as (centre x, centre y, radius, stroke width, from°, to°), in the
/// logo's own units (same numbers as the top-bar logo in `ui.rs`).
const ARCS: [(f32, f32, f32, f32, f32, f32); 2] = [
    (0.0, 0.0, 13.0, 3.0, 200.0, 470.0),
    (3.0, 2.0, 8.0, 2.5, 20.0, 250.0),
];

pub fn app_icon(size: u32) -> IconData {
    let s = size as f32;
    // macOS icon grid: the tile fills ~80% of the canvas.
    let tile = s * 0.805;
    let tile_min = (s - tile) / 2.0;
    let corner = tile * 0.225;
    // Logo units → pixels; the logo is ~32 units across.
    let scale = tile * 0.62 / 32.0;
    let centre = s / 2.0;

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);

            // Tile: rounded square with a subtle vertical gradient and edge.
            let d_tile = rounded_rect_sdf(x, y, tile_min, tile, corner);
            let tile_cov = (0.5 - d_tile).clamp(0.0, 1.0);
            let t = ((y - tile_min) / tile).clamp(0.0, 1.0);
            let mut rgb = lerp([0x2a, 0x2a, 0x2d], [0x18, 0x18, 0x1a], t);
            let edge = (1.0 - (d_tile + s * 0.006).abs() / (s * 0.006)).clamp(0.0, 1.0);
            rgb = mix(rgb, [0x3a, 0x3a, 0x3e], edge * 0.8);

            // Logo arcs.
            let (lx, ly) = ((x - centre) / scale, (y - centre) / scale);
            let mut ink = 0.0f32;
            for &(cx, cy, r, w, from, to) in &ARCS {
                let d = arc_distance(lx - cx, ly - cy, r, from, to) * scale;
                ink = ink.max((w * scale / 2.0 - d + 0.5).clamp(0.0, 1.0));
            }
            rgb = mix(rgb, [0x7c, 0x7c, 0x82], ink);

            rgba.extend_from_slice(&[
                rgb[0].round() as u8,
                rgb[1].round() as u8,
                rgb[2].round() as u8,
                (tile_cov * 255.0).round() as u8,
            ]);
        }
    }
    IconData {
        rgba,
        width: size,
        height: size,
    }
}

/// Signed distance to a rounded square (negative inside).
fn rounded_rect_sdf(x: f32, y: f32, min: f32, size: f32, r: f32) -> f32 {
    let half = size / 2.0;
    let (cx, cy) = (min + half, min + half);
    let qx = (x - cx).abs() - (half - r);
    let qy = (y - cy).abs() - (half - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// Distance from a point (relative to the arc centre) to an arc running
/// clockwise from `from` to `to` degrees (0° = right, y down).
fn arc_distance(x: f32, y: f32, r: f32, from: f32, to: f32) -> f32 {
    let angle = y.atan2(x).to_degrees().rem_euclid(360.0);
    if (angle - from).rem_euclid(360.0) <= to - from {
        ((x * x + y * y).sqrt() - r).abs()
    } else {
        [from, to]
            .iter()
            .map(|a| {
                let a = a.to_radians();
                ((x - r * a.cos()).powi(2) + (y - r * a.sin()).powi(2)).sqrt()
            })
            .fold(f32::MAX, f32::min)
    }
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] as f32 + (b[i] as f32 - a[i] as f32) * t)
}

fn mix(a: [f32; 3], b: [u8; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] as f32 - a[i]) * t)
}
