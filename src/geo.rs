//! Rumus bentuk pulau, animasi, dan huruf/angka gambar.
//! Bagian ini murni matematika (tanpa kode Windows).

use std::f32::consts::PI;

pub type Pt = (f32, f32);

// Ukuran area gambar dan bentuk (sama dengan versi Python Fase 1)
pub const W: f32 = 380.0;
pub const H: f32 = 300.0;
pub const CX: f32 = W / 2.0;
pub const MARGIN: f32 = 10.0;
pub const CAP_W: f32 = 170.0;
pub const CAP_H: f32 = 38.0;
pub const ARCH_R: f32 = 110.0;
pub const ARCH_H: f32 = 84.0;
pub const CAP_HALF: f32 = (CAP_W - CAP_H) / 2.0;
pub const CAP_CY: f32 = MARGIN + CAP_H / 2.0;
pub const ARCH_CY: f32 = MARGIN + ARCH_H / 2.0;
pub const HOLD_R: f32 = ARCH_R + ARCH_H / 2.0 + 18.0;
pub const TRIGGER_PAD: f32 = 18.0;

pub const SECTORS: [(f32, f32); 3] = [
    (-1.0, -1.0 / 3.0),
    (-1.0 / 3.0, 1.0 / 3.0),
    (1.0 / 3.0, 1.0),
];
pub const SECTOR_S: [f32; 3] = [-2.0 / 3.0, 0.0, 2.0 / 3.0];

const CAP_POINTS: usize = 14;

/// t = 0 kapsul lurus, t = 1 busur setengah lingkaran (t > 1 = efek membal).
/// s = posisi sepanjang garis tengah, -1 (ujung kiri) sampai +1 (ujung kanan).
pub fn centerline(s: f32, t: f32) -> (f32, f32, f32, f32) {
    let sx = CX + s * CAP_HALF;
    let sy = CAP_CY;
    let phi = s * PI / 2.0;
    let ax = CX + ARCH_R * phi.sin();
    let ay = ARCH_CY + ARCH_R * phi.cos();
    let dsx = CAP_HALF;
    let dsy = 0.0;
    let dax = ARCH_R * phi.cos() * PI / 2.0;
    let day = -ARCH_R * phi.sin() * PI / 2.0;
    (
        sx + (ax - sx) * t,
        sy + (ay - sy) * t,
        dsx + (dax - dsx) * t,
        dsy + (day - dsy) * t,
    )
}

pub fn half_thickness(t: f32) -> f32 {
    ((CAP_H + (ARCH_H - CAP_H) * t) / 2.0).max(1.0)
}

/// (x, y, arahX, arahY, normalX, normalY)
pub fn frame(s: f32, t: f32) -> (f32, f32, f32, f32, f32, f32) {
    let (x, y, dx, dy) = centerline(s, t);
    let n = (dx * dx + dy * dy).sqrt();
    let n = if n == 0.0 { 1.0 } else { n };
    let tx = dx / n;
    let ty = dy / n;
    (x, y, tx, ty, -ty, tx)
}

pub fn edge_points(s: f32, t: f32) -> (Pt, Pt) {
    let (x, y, _tx, _ty, nx, ny) = frame(s, t);
    let h = half_thickness(t);
    ((x - nx * h, y - ny * h), (x + nx * h, y + ny * h))
}

/// Garis tepi pita dari posisi s0 sampai s1, ujungnya bisa dibulatkan.
pub fn band_outline(s0: f32, s1: f32, t: f32, n: usize, cap0: bool, cap1: bool) -> Vec<Pt> {
    let h = half_thickness(t);
    let mut top: Vec<Pt> = Vec::with_capacity(n + 1);
    let mut bot: Vec<Pt> = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let s = s0 + (s1 - s0) * (i as f32) / (n as f32);
        let (a, b) = edge_points(s, t);
        top.push(a);
        bot.push(b);
    }
    let mut pts = top;
    let m = CAP_POINTS as f32;
    if cap1 {
        let (x, y, tx, ty, nx, ny) = frame(s1, t);
        for k in 1..CAP_POINTS {
            let a = -PI / 2.0 + PI * (k as f32) / m;
            pts.push((
                x + nx * h * a.sin() + tx * h * a.cos(),
                y + ny * h * a.sin() + ty * h * a.cos(),
            ));
        }
    }
    for p in bot.into_iter().rev() {
        pts.push(p);
    }
    if cap0 {
        let (x, y, tx, ty, nx, ny) = frame(s0, t);
        for k in 1..CAP_POINTS {
            let a = PI / 2.0 - PI * (k as f32) / m;
            pts.push((
                x + nx * h * a.sin() - tx * h * a.cos(),
                y + ny * h * a.sin() - ty * h * a.cos(),
            ));
        }
    }
    pts
}

pub fn body_points(t: f32) -> Vec<Pt> {
    band_outline(-1.0, 1.0, t, 56, true, true)
}

/// Sektor di bawah kursor saat busur terbuka: 0 kiri, 1 bawah, 2 kanan, -1 tidak ada.
pub fn sector_at(x: f32, y: f32) -> i32 {
    let dx = x - CX;
    let dy = y - ARCH_CY;
    let d = (dx * dx + dy * dy).sqrt();
    if d < ARCH_R - ARCH_H / 2.0 - 4.0 || d > ARCH_R + ARCH_H / 2.0 + 4.0 || dy < -ARCH_H / 2.0 {
        return -1;
    }
    let phi = dx.atan2(dy.max(0.0));
    if phi < -PI / 6.0 {
        0
    } else if phi > PI / 6.0 {
        2
    } else {
        1
    }
}

pub fn in_trigger(x: f32, y: f32) -> bool {
    x >= CX - CAP_W / 2.0 - TRIGGER_PAD
        && x <= CX + CAP_W / 2.0 + TRIGGER_PAD
        && y >= MARGIN - TRIGGER_PAD
        && y <= MARGIN + CAP_H + TRIGGER_PAD
}

pub fn point_in_poly(pts: &[Pt], x: f32, y: f32) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Animasi
// ---------------------------------------------------------------------------

/// Membal elastis (untuk membuka busur).
pub fn out_elastic(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let p = 0.5f32;
    let s = p / 4.0;
    2f32.powf(-10.0 * x) * ((x - s) * 2.0 * PI / p).sin() + 1.0
}

/// Kurva cubic-bezier CSS (untuk menutup busur dengan halus).
pub fn bezier_ease(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let bez = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

// ---------------------------------------------------------------------------
// Garis bantu untuk ikon dan huruf
// ---------------------------------------------------------------------------

pub fn arc_pts(cx: f32, cy: f32, r: f32, a0_deg: f32, a1_deg: f32, n: usize) -> Vec<Pt> {
    let mut v = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = (a0_deg + (a1_deg - a0_deg) * (i as f32) / (n as f32)) * PI / 180.0;
        v.push((cx + r * a.cos(), cy + r * a.sin()));
    }
    v
}

fn circle_pts(cx: f32, cy: f32, r: f32) -> Vec<Pt> {
    arc_pts(cx, cy, r, 0.0, 360.0, 12)
}

/// Angka dan huruf sederhana dari garis, dalam kotak lebar 2 x tinggi 4.
pub fn glyph(c: char) -> Vec<Vec<Pt>> {
    match c {
        '0' => vec![vec![(0.0, 0.0), (2.0, 0.0), (2.0, 4.0), (0.0, 4.0), (0.0, 0.0)]],
        '1' => vec![vec![(0.4, 0.9), (1.1, 0.0), (1.1, 4.0)]],
        '2' => vec![vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 4.0), (2.0, 4.0)]],
        '3' => vec![
            vec![(0.0, 0.0), (2.0, 0.0), (2.0, 4.0), (0.0, 4.0)],
            vec![(0.5, 2.0), (2.0, 2.0)],
        ],
        '4' => vec![
            vec![(0.0, 0.0), (0.0, 2.0), (2.0, 2.0)],
            vec![(2.0, 0.0), (2.0, 4.0)],
        ],
        '5' => vec![vec![(2.0, 0.0), (0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 4.0), (0.0, 4.0)]],
        '6' => vec![vec![(2.0, 0.0), (0.0, 0.0), (0.0, 4.0), (2.0, 4.0), (2.0, 2.0), (0.0, 2.0)]],
        '7' => vec![vec![(0.0, 0.0), (2.0, 0.0), (0.8, 4.0)]],
        '8' => vec![
            vec![(0.0, 0.0), (2.0, 0.0), (2.0, 4.0), (0.0, 4.0), (0.0, 0.0)],
            vec![(0.0, 2.0), (2.0, 2.0)],
        ],
        '9' => vec![vec![(2.0, 2.0), (0.0, 2.0), (0.0, 0.0), (2.0, 0.0), (2.0, 4.0), (0.0, 4.0)]],
        '%' => vec![
            vec![(1.9, 0.1), (0.1, 3.9)],
            circle_pts(0.4, 0.7, 0.4),
            circle_pts(1.6, 3.3, 0.4),
        ],
        'N' => vec![vec![(0.0, 4.0), (0.0, 0.0), (2.0, 4.0), (2.0, 0.0)]],
        '/' => vec![vec![(1.8, 0.0), (0.2, 4.0)]],
        'A' => vec![
            vec![(0.0, 4.0), (1.0, 0.0), (2.0, 4.0)],
            vec![(0.45, 2.7), (1.55, 2.7)],
        ],
        _ => vec![],
    }
}
