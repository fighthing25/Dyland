//! Alat gambar: bentuk, ikon, tulisan.

use crate::geo::{arc_pts, glyph, Pt};
use std::collections::HashMap;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

pub const BODY: [u8; 4] = [8, 8, 10, 255];
pub const BORDER: [u8; 4] = [255, 255, 255, 30];

// ---------------------------------------------------------------------------
// Bentuk dasar
// ---------------------------------------------------------------------------
fn paint_of(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

pub fn fill(px: &mut Pixmap, path: &Path, c: [u8; 4], tf: Transform) {
    px.fill_path(path, &paint_of(c), FillRule::Winding, tf, None);
}

pub fn stroke(px: &mut Pixmap, path: &Path, c: [u8; 4], width: f32, tf: Transform) {
    let mut s = Stroke::default();
    s.width = width;
    s.line_cap = LineCap::Round;
    s.line_join = LineJoin::Round;
    px.stroke_path(path, &paint_of(c), &s, tf, None);
}

pub fn poly_path(pts: &[Pt], close: bool) -> Option<Path> {
    if pts.len() < 2 {
        return None;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pb.line_to(p.0, p.1);
    }
    if close {
        pb.close();
    }
    pb.finish()
}

pub fn circle_path(cx: f32, cy: f32, r: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    pb.finish()
}

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let k = 0.5523 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn line(px: &mut Pixmap, tf: Transform, c: [u8; 4], w: f32, a: Pt, b: Pt) {
    if let Some(p) = poly_path(&[a, b], false) {
        stroke(px, &p, c, w, tf);
    }
}

fn tri(px: &mut Pixmap, tf: Transform, c: [u8; 4], p: [Pt; 3]) {
    if let Some(path) = poly_path(&p, true) {
        fill(px, &path, c, tf);
    }
}

fn polyline(px: &mut Pixmap, tf: Transform, c: [u8; 4], w: f32, pts: &[Pt], close: bool) {
    if let Some(p) = poly_path(pts, close) {
        stroke(px, &p, c, w, tf);
    }
}

// ---------------------------------------------------------------------------
// Ikon (titik tengah di 0,0; ukuran dasar sekitar 24x24)
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    None,
    Prev,
    PlayPause,
    Next,
    Mic,
    MicOff,
    Speaker,
    Camera,
    Coffee,
    Lock,
    Moon,
    Restart,
    Power,
    Close,
    Left,
    Right,
    Note,
    Sliders,
    Calendar,
    Sun,
    SpeakerOff,
    Clipboard,
    Mosque,
    Chart,
    Puzzle,
    Gear,
    Plus,
    Check,
    Shelf,
    Phone,
}

pub fn draw_icon(px: &mut Pixmap, ic: Icon, tf: Transform, c: [u8; 4]) {
    match ic {
        Icon::None => {}
        Icon::Prev => {
            tri(px, tf, c, [(7.0, -7.0), (-3.0, 0.0), (7.0, 7.0)]);
            line(px, tf, c, 2.2, (-7.0, -7.0), (-7.0, 7.0));
        }
        Icon::Next => {
            tri(px, tf, c, [(-7.0, -7.0), (3.0, 0.0), (-7.0, 7.0)]);
            line(px, tf, c, 2.2, (7.0, -7.0), (7.0, 7.0));
        }
        Icon::PlayPause => {
            tri(px, tf, c, [(-10.0, -7.0), (-1.0, 0.0), (-10.0, 7.0)]);
            line(px, tf, c, 2.4, (3.5, -7.0), (3.5, 7.0));
            line(px, tf, c, 2.4, (8.5, -7.0), (8.5, 7.0));
        }
        Icon::Mic | Icon::MicOff => {
            if let Some(p) = rounded_rect(-3.0, -9.0, 6.0, 11.0, 3.0) {
                stroke(px, &p, c, 1.8, tf);
            }
            let u = arc_pts(0.0, -1.0, 6.5, 10.0, 170.0, 10);
            polyline(px, tf, c, 1.8, &u, false);
            line(px, tf, c, 1.8, (0.0, 5.5), (0.0, 9.0));
            if ic == Icon::MicOff {
                line(px, tf, [255, 90, 90, c[3]], 2.2, (-9.0, -9.0), (9.0, 9.0));
            }
        }
        Icon::Speaker => {
            let body: [Pt; 6] = [(-8.0, -3.0), (-4.0, -3.0), (1.0, -7.0), (1.0, 7.0), (-4.0, 3.0), (-8.0, 3.0)];
            if let Some(p) = poly_path(&body, true) {
                fill(px, &p, c, tf);
            }
            for r in [4.5f32, 8.5f32] {
                let pts = arc_pts(1.0, 0.0, r, -55.0, 55.0, 8);
                polyline(px, tf, c, 1.8, &pts, false);
            }
        }
        Icon::SpeakerOff => {
            let body: [Pt; 6] = [(-8.0, -3.0), (-4.0, -3.0), (1.0, -7.0), (1.0, 7.0), (-4.0, 3.0), (-8.0, 3.0)];
            if let Some(p) = poly_path(&body, true) {
                fill(px, &p, c, tf);
            }
            line(px, tf, [255, 90, 90, c[3]], 2.2, (-9.0, -9.0), (9.0, 9.0));
        }
        Icon::Clipboard => {
            if let Some(p) = rounded_rect(-7.0, -8.0, 14.0, 17.0, 2.5) {
                stroke(px, &p, c, 1.8, tf);
            }
            if let Some(p) = rounded_rect(-3.5, -10.5, 7.0, 4.5, 1.8) {
                fill(px, &p, c, tf);
            }
            line(px, tf, c, 1.6, (-4.0, -1.0), (4.0, -1.0));
            line(px, tf, c, 1.6, (-4.0, 3.0), (4.0, 3.0));
        }
        Icon::Mosque => {
            let dome = arc_pts(0.0, 2.0, 6.5, 180.0, 360.0, 12);
            polyline(px, tf, c, 1.8, &dome, false);
            line(px, tf, c, 1.8, (-8.5, 2.0), (8.5, 2.0));
            line(px, tf, c, 1.8, (-8.0, 2.0), (-8.0, 9.0));
            line(px, tf, c, 1.8, (8.0, 2.0), (8.0, 9.0));
            line(px, tf, c, 1.8, (-8.0, 9.0), (8.0, 9.0));
            line(px, tf, c, 1.6, (0.0, -4.5), (0.0, -9.5));
        }
        Icon::Chart => {
            line(px, tf, c, 3.4, (-6.0, 9.0), (-6.0, 3.0));
            line(px, tf, c, 3.4, (0.0, 9.0), (0.0, -3.0));
            line(px, tf, c, 3.4, (6.0, 9.0), (6.0, -8.0));
        }
        Icon::Puzzle => {
            for (x, y) in [(-9.5f32, -9.5f32), (1.5f32, -9.5f32), (-9.5f32, 1.5f32)] {
                if let Some(p) = rounded_rect(x, y, 8.0, 8.0, 2.0) {
                    stroke(px, &p, c, 1.7, tf);
                }
            }
            line(px, tf, c, 1.8, (5.5, 2.5), (5.5, 9.5));
            line(px, tf, c, 1.8, (2.0, 6.0), (9.0, 6.0));
        }
        Icon::Gear => {
            if let Some(p) = circle_path(0.0, 0.0, 5.6) {
                stroke(px, &p, c, 1.8, tf);
            }
            if let Some(p) = circle_path(0.0, 0.0, 2.0) {
                fill(px, &p, c, tf);
            }
            for k in 0..8 {
                let a = (k as f32) * std::f32::consts::PI / 4.0;
                line(
                    px,
                    tf,
                    c,
                    3.0,
                    (7.4 * a.cos(), 7.4 * a.sin()),
                    (9.2 * a.cos(), 9.2 * a.sin()),
                );
            }
        }
        Icon::Plus => {
            line(px, tf, c, 2.0, (-6.0, 0.0), (6.0, 0.0));
            line(px, tf, c, 2.0, (0.0, -6.0), (0.0, 6.0));
        }
        Icon::Check => {
            polyline(px, tf, c, 2.2, &[(-5.0, 0.0), (-1.5, 4.0), (5.0, -4.5)], false);
        }
        Icon::Shelf => {
            polyline(px, tf, c, 1.9, &[(-9.0, -1.0), (-9.0, 8.0), (9.0, 8.0), (9.0, -1.0)], false);
            line(px, tf, c, 1.9, (0.0, -9.0), (0.0, 3.0));
            polyline(px, tf, c, 1.9, &[(-4.5, -1.5), (0.0, 3.0), (4.5, -1.5)], false);
        }
        Icon::Phone => {
            if let Some(p) = rounded_rect(-5.5, -9.5, 11.0, 19.0, 2.8) {
                stroke(px, &p, c, 1.8, tf);
            }
            line(px, tf, c, 1.8, (-2.0, 6.0), (2.0, 6.0));
            line(px, tf, c, 1.4, (-2.0, -6.5), (2.0, -6.5));
        }
        Icon::Camera => {
            if let Some(p) = rounded_rect(-10.0, -5.0, 20.0, 14.0, 3.0) {
                stroke(px, &p, c, 1.8, tf);
            }
            if let Some(p) = circle_path(0.0, 2.0, 3.8) {
                stroke(px, &p, c, 1.8, tf);
            }
            line(px, tf, c, 1.8, (-4.0, -8.0), (2.0, -8.0));
        }
        Icon::Coffee => {
            let cup: [Pt; 4] = [(-7.0, -3.0), (5.0, -3.0), (4.0, 8.0), (-6.0, 8.0)];
            polyline(px, tf, c, 1.8, &cup, true);
            let handle = arc_pts(5.0, 2.5, 3.8, -90.0, 90.0, 8);
            polyline(px, tf, c, 1.8, &handle, false);
            line(px, tf, c, 1.6, (-3.0, -9.0), (-3.0, -6.0));
            line(px, tf, c, 1.6, (1.0, -9.0), (1.0, -6.0));
        }
        Icon::Lock => {
            if let Some(p) = rounded_rect(-7.0, -1.0, 14.0, 10.0, 2.0) {
                stroke(px, &p, c, 1.8, tf);
            }
            let shackle = arc_pts(0.0, -1.0, 4.5, 180.0, 360.0, 10);
            polyline(px, tf, c, 1.8, &shackle, false);
        }
        Icon::Moon => {
            let mut pts = arc_pts(0.0, 0.0, 8.0, 60.0, 300.0, 16);
            let inner = arc_pts(4.0, 0.0, 6.93, 270.0, 90.0, 16);
            pts.extend(inner);
            if let Some(p) = poly_path(&pts, true) {
                fill(px, &p, c, tf);
            }
        }
        Icon::Restart => {
            let arc = arc_pts(0.0, 0.0, 7.0, -40.0, 250.0, 18);
            polyline(px, tf, c, 1.9, &arc, false);
            tri(px, tf, c, [(9.5, -6.5), (3.5, -8.5), (6.5, -1.5)]);
        }
        Icon::Power => {
            let arc = arc_pts(0.0, 0.0, 7.5, 305.0, 595.0, 20);
            polyline(px, tf, c, 1.9, &arc, false);
            line(px, tf, c, 1.9, (0.0, -9.5), (0.0, -1.0));
        }
        Icon::Close => {
            line(px, tf, c, 1.8, (-5.0, -5.0), (5.0, 5.0));
            line(px, tf, c, 1.8, (5.0, -5.0), (-5.0, 5.0));
        }
        Icon::Left => {
            polyline(px, tf, c, 2.0, &[(3.0, -6.0), (-3.0, 0.0), (3.0, 6.0)], false);
        }
        Icon::Right => {
            polyline(px, tf, c, 2.0, &[(-3.0, -6.0), (3.0, 0.0), (-3.0, 6.0)], false);
        }
        Icon::Note => {
            if let Some(p) = circle_path(-4.0, 6.0, 3.4) {
                fill(px, &p, c, tf);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(-0.6, 5.5);
            pb.line_to(-0.6, -8.0);
            pb.cubic_to(-0.6, -4.0, 7.4, -5.0, 6.4, 0.0);
            if let Some(p) = pb.finish() {
                stroke(px, &p, c, 2.0, tf);
            }
        }
        Icon::Sliders => {
            let rows: [(f32, f32); 3] = [(-6.0, -3.0), (0.0, 4.0), (6.0, -1.0)];
            for (y, kx) in rows.iter() {
                line(px, tf, c, 2.0, (-9.0, *y), (9.0, *y));
                if let Some(p) = circle_path(*kx, *y, 2.6) {
                    fill(px, &p, BODY, tf);
                    stroke(px, &p, c, 1.8, tf);
                }
            }
        }
        Icon::Calendar => {
            if let Some(p) = rounded_rect(-9.0, -7.0, 18.0, 16.0, 3.0) {
                stroke(px, &p, c, 2.0, tf);
            }
            line(px, tf, c, 2.0, (-9.0, -2.0), (9.0, -2.0));
            line(px, tf, c, 2.0, (-4.0, -10.0), (-4.0, -5.0));
            line(px, tf, c, 2.0, (4.0, -10.0), (4.0, -5.0));
        }
        Icon::Sun => {
            if let Some(p) = circle_path(0.0, 0.0, 3.8) {
                stroke(px, &p, c, 1.8, tf);
            }
            for k in 0..8 {
                let a = (k as f32) * std::f32::consts::PI / 4.0;
                line(
                    px,
                    tf,
                    c,
                    1.8,
                    (6.5 * a.cos(), 6.5 * a.sin()),
                    (9.5 * a.cos(), 9.5 * a.sin()),
                );
            }
        }
    }
}

/// Angka dari garis (dipakai indikator volume/kecerahan), rata kanan di `right`.
pub fn draw_digits(px: &mut Pixmap, tf: Transform, text: &str, right: f32, cy: f32, c: [u8; 4]) {
    let u = 2.9f32;
    let adv = 3.5 * u;
    let n = text.chars().count() as f32;
    let left = right - (n * adv - 1.5 * u);
    let top = cy - 2.0 * u;
    for (i, ch) in text.chars().enumerate() {
        let ox = left + (i as f32) * adv;
        for strokes in glyph(ch) {
            let pts: Vec<Pt> = strokes.iter().map(|p| (ox + p.0 * u, top + p.1 * u)).collect();
            if let Some(path) = poly_path(&pts, false) {
                stroke(px, &path, c, 1.7, tf);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mesin tulisan: memakai font Segoe UI yang sudah ada di Windows
// ---------------------------------------------------------------------------
struct Glyph {
    xmin: i32,
    ymin: i32,
    w: usize,
    h: usize,
    adv: f32,
    bmp: Vec<u8>,
}

pub struct TextEngine {
    font: Option<fontdue::Font>,
    cache: HashMap<(char, u32), Glyph>,
}

impl TextEngine {
    pub fn new() -> TextEngine {
        let win = std::env::var("WINDIR").unwrap_or_else(|_| String::from("C:\\Windows"));
        let mut font = None;
        for name in ["segoeui.ttf", "tahoma.ttf", "arial.ttf"] {
            let path = format!("{}\\Fonts\\{}", win, name);
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(f) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                    font = Some(f);
                    break;
                }
            }
        }
        TextEngine { font, cache: HashMap::new() }
    }

    fn glyph(&mut self, ch: char, px: u32) -> Option<&Glyph> {
        let font = self.font.as_ref()?;
        let g: &Glyph = self.cache.entry((ch, px)).or_insert_with(|| {
            let (m, bmp) = font.rasterize(ch, px as f32);
            Glyph {
                xmin: m.xmin,
                ymin: m.ymin,
                w: m.width,
                h: m.height,
                adv: m.advance_width,
                bmp,
            }
        });
        Some(g)
    }

    /// Lebar tulisan dalam piksel (px = ukuran huruf dalam piksel).
    pub fn width(&mut self, s: &str, px: f32) -> f32 {
        let pxi = px.round().max(1.0) as u32;
        let mut w = 0.0f32;
        for ch in s.chars() {
            if let Some(g) = self.glyph(ch, pxi) {
                w += g.adv;
            }
        }
        w
    }

    /// Tulis teks mulai dari x (kiri) pada garis dasar `baseline` (piksel).
    pub fn draw(&mut self, pm: &mut Pixmap, s: &str, x: f32, baseline: f32, px: f32, c: [u8; 4]) {
        let pxi = px.round().max(1.0) as u32;
        let pw = pm.width() as i32;
        let ph = pm.height() as i32;
        let mut pen = x;
        for ch in s.chars() {
            let g = match self.glyph(ch, pxi) {
                Some(g) => g,
                None => continue,
            };
            let gx = (pen + g.xmin as f32).round() as i32;
            let gy = (baseline - g.ymin as f32 - g.h as f32).round() as i32;
            let data = pm.data_mut();
            for row in 0..g.h {
                for col in 0..g.w {
                    let cov = g.bmp[row * g.w + col] as u32;
                    if cov == 0 {
                        continue;
                    }
                    let xx = gx + col as i32;
                    let yy = gy + row as i32;
                    if xx < 0 || yy < 0 || xx >= pw || yy >= ph {
                        continue;
                    }
                    let a = cov * c[3] as u32 / 255;
                    let inv = 255 - a;
                    let i = ((yy * pw + xx) * 4) as usize;
                    data[i] = ((c[0] as u32 * a + data[i] as u32 * inv) / 255) as u8;
                    data[i + 1] = ((c[1] as u32 * a + data[i + 1] as u32 * inv) / 255) as u8;
                    data[i + 2] = ((c[2] as u32 * a + data[i + 2] as u32 * inv) / 255) as u8;
                    data[i + 3] = ((255 * a + data[i + 3] as u32 * inv) / 255) as u8;
                }
            }
            pen += g.adv;
        }
    }
}
