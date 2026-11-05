//! Renders the app icon to an uncompressed TGA file (used by scripts/make-app.sh).
//!
//! Usage: cargo run --example render_icon -- <out.tga> [size]

#[path = "../src/icon.rs"]
mod icon;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: render_icon <out.tga> [size]");
    let size = args.next().map_or(1024, |s| s.parse().expect("size must be a number"));

    let icon = icon::app_icon(size);
    let mut tga = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    tga.extend_from_slice(&(icon.width as u16).to_le_bytes());
    tga.extend_from_slice(&(icon.height as u16).to_le_bytes());
    tga.extend_from_slice(&[32, 0x28]); // 32-bit, 8 alpha bits, top-left origin
    for px in icon.rgba.chunks(4) {
        tga.extend_from_slice(&[px[2], px[1], px[0], px[3]]); // RGBA → BGRA
    }
    std::fs::write(&path, tga).expect("could not write icon");
}
