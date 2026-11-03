//! DYLAND - Dynamic Island untuk Windows (Fase 1, versi Rust)
//!
//! - Jendela transparan, selalu di atas, area kosong otomatis klik-tembus
//! - Kapsul hitam di tengah atas layar -> busur 3 sektor saat kursor mendekat
//! - Scroll sisi KANAN = volume Windows, sisi KIRI = kecerahan layar
//! - Klik kanan pada pulau = menu "Keluar"
//! Semua gambar digambar lewat kode. Tanpa WebView2, tanpa file gambar.

#![windows_subsystem = "windows"]

mod geo;

use geo::*;
use std::cell::RefCell;
use std::mem;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{BOOL, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HDC, HWND, POINT, SIZE};
use winapi::shared::wtypesbase::CLSCTX_ALL;
use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx};
use winapi::um::endpointvolume::IAudioEndpointVolume;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::mmdeviceapi::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator};
use winapi::um::objbase::COINIT_APARTMENTTHREADED;
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateDIBSection, GetDeviceCaps, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BLENDFUNCTION, DIB_RGB_COLORS, LOGPIXELSX,
};
use winapi::um::winuser::*;
use winapi::Interface;

// ---------------------------------------------------------------------------
// PENGATURAN
// ---------------------------------------------------------------------------
const LEAVE_DELAY: f32 = 0.18; // detik sebelum busur menutup setelah kursor pergi
const VOLUME_STEP: f32 = 4.0; // persen per 1 klik roda mouse
const BRIGHT_STEP: f32 = 5.0; // persen per 1 klik roda mouse
const HUD_MS: u64 = 1300; // lama indikator volume/kecerahan tampil

const BODY: [u8; 4] = [8, 8, 10, 255];
const BORDER: [u8; 4] = [255, 255, 255, 30];

// ---------------------------------------------------------------------------
// VOLUME (Windows Core Audio)
// ---------------------------------------------------------------------------
static CLSID_MMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xBCDE0395,
    Data2: 0xE52F,
    Data3: 0x467C,
    Data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
};

/// Ubah volume utama sebesar `delta` persen. Hasilnya volume baru (0-100) atau None.
fn change_volume(delta: f32) -> Option<i32> {
    unsafe {
        let mut en: *mut IMMDeviceEnumerator = null_mut();
        let hr = CoCreateInstance(
            &CLSID_MMDEVICE_ENUMERATOR,
            null_mut(),
            CLSCTX_ALL,
            &IMMDeviceEnumerator::uuidof(),
            &mut en as *mut *mut IMMDeviceEnumerator as *mut _,
        );
        if hr < 0 || en.is_null() {
            return None;
        }
        let mut dev: *mut IMMDevice = null_mut();
        let hr = (*en).GetDefaultAudioEndpoint(eRender, eConsole, &mut dev);
        if hr < 0 || dev.is_null() {
            (*en).Release();
            return None;
        }
        let mut ep: *mut IAudioEndpointVolume = null_mut();
        let hr = (*dev).Activate(
            &IAudioEndpointVolume::uuidof(),
            CLSCTX_ALL,
            null_mut(),
            &mut ep as *mut *mut IAudioEndpointVolume as *mut _,
        );
        let mut result = None;
        if hr >= 0 && !ep.is_null() {
            let mut cur = 0.0f32;
            if (*ep).GetMasterVolumeLevelScalar(&mut cur) >= 0 {
                let new = (cur * 100.0 + delta).clamp(0.0, 100.0);
                if delta > 0.0 {
                    let mut muted: BOOL = 0;
                    (*ep).GetMute(&mut muted);
                    if muted != 0 {
                        (*ep).SetMute(0, null());
                    }
                }
                (*ep).SetMasterVolumeLevelScalar(new / 100.0, null());
                result = Some(new.round() as i32);
            }
            (*ep).Release();
        }
        (*dev).Release();
        (*en).Release();
        result
    }
}

// ---------------------------------------------------------------------------
// KECERAHAN (lewat PowerShell bawaan Windows, dikerjakan di latar belakang)
// ---------------------------------------------------------------------------
fn run_powershell(script: &str) -> Option<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000) // tanpa jendela hitam
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

struct Brightness {
    level: f32,
    available: Arc<AtomicBool>,
    initial: Arc<AtomicI32>,
    tx: Sender<i32>,
}

impl Brightness {
    fn new() -> Brightness {
        let available = Arc::new(AtomicBool::new(true));
        let initial = Arc::new(AtomicI32::new(-1));
        let (tx, rx) = channel::<i32>();
        let avail2 = available.clone();
        let init2 = initial.clone();
        std::thread::spawn(move || {
            let read = run_powershell(
                "(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness",
            )
            .and_then(|s| s.lines().next().and_then(|l| l.trim().parse::<i32>().ok()));
            match read {
                Some(v) => init2.store(v, Ordering::Relaxed),
                None => avail2.store(false, Ordering::Relaxed),
            }
            while let Ok(mut v) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    v = newer; // hanya nilai terbaru yang dikerjakan
                }
                let cmd = format!(
                    "(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1,{})",
                    v
                );
                if run_powershell(&cmd).is_none() {
                    avail2.store(false, Ordering::Relaxed);
                }
            }
        });
        Brightness { level: -1.0, available, initial, tx }
    }

    fn change(&mut self, delta: f32) -> Option<i32> {
        if !self.available.load(Ordering::Relaxed) {
            return None;
        }
        if self.level < 0.0 {
            let i = self.initial.load(Ordering::Relaxed);
            self.level = if i >= 0 { i as f32 } else { 50.0 };
        }
        self.level = (self.level + delta).clamp(0.0, 100.0);
        let v = self.level.round() as i32;
        let _ = self.tx.send(v);
        Some(v)
    }
}

// ---------------------------------------------------------------------------
// ALAT GAMBAR (tiny-skia)
// ---------------------------------------------------------------------------
fn paint_of(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

fn fill(px: &mut Pixmap, path: &Path, c: [u8; 4], tf: Transform) {
    px.fill_path(path, &paint_of(c), FillRule::Winding, tf, None);
}

fn stroke(px: &mut Pixmap, path: &Path, c: [u8; 4], width: f32, tf: Transform) {
    let mut s = Stroke::default();
    s.width = width;
    s.line_cap = LineCap::Round;
    s.line_join = LineJoin::Round;
    px.stroke_path(path, &paint_of(c), &s, tf, None);
}

fn poly_path(pts: &[Pt], close: bool) -> Option<Path> {
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

fn circle_path(cx: f32, cy: f32, r: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    pb.finish()
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
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

fn line(px: &mut Pixmap, tf: Transform, c: [u8; 4], w: f32, a: Pt, b: Pt) {
    if let Some(p) = poly_path(&[a, b], false) {
        stroke(px, &p, c, w, tf);
    }
}

// Ikon (titik tengah di 0,0; ukuran dasar sekitar 24x24)
fn icon_note(px: &mut Pixmap, tf: Transform, c: [u8; 4]) {
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

fn icon_sliders(px: &mut Pixmap, tf: Transform, c: [u8; 4]) {
    let rows: [(f32, f32); 3] = [(-6.0, -3.0), (0.0, 4.0), (6.0, -1.0)];
    for (y, kx) in rows.iter() {
        line(px, tf, c, 2.0, (-9.0, *y), (9.0, *y));
        if let Some(p) = circle_path(*kx, *y, 2.6) {
            fill(px, &p, BODY, tf);
            stroke(px, &p, c, 1.8, tf);
        }
    }
}

fn icon_calendar(px: &mut Pixmap, tf: Transform, c: [u8; 4]) {
    if let Some(p) = rounded_rect(-9.0, -7.0, 18.0, 16.0, 3.0) {
        stroke(px, &p, c, 2.0, tf);
    }
    line(px, tf, c, 2.0, (-9.0, -2.0), (9.0, -2.0));
    line(px, tf, c, 2.0, (-4.0, -10.0), (-4.0, -5.0));
    line(px, tf, c, 2.0, (4.0, -10.0), (4.0, -5.0));
}

fn icon_speaker(px: &mut Pixmap, tf: Transform, c: [u8; 4]) {
    let body: [Pt; 6] = [(-8.0, -3.0), (-4.0, -3.0), (1.0, -7.0), (1.0, 7.0), (-4.0, 3.0), (-8.0, 3.0)];
    if let Some(p) = poly_path(&body, true) {
        fill(px, &p, c, tf);
    }
    for r in [4.5f32, 8.5f32] {
        let pts = arc_pts(1.0, 0.0, r, -55.0, 55.0, 8);
        if let Some(p) = poly_path(&pts, false) {
            stroke(px, &p, c, 1.8, tf);
        }
    }
}

fn icon_sun(px: &mut Pixmap, tf: Transform, c: [u8; 4]) {
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

/// Tulisan dari garis, rata kanan di `right`, tengah vertikal di `cy`.
fn draw_text(px: &mut Pixmap, tf: Transform, text: &str, right: f32, cy: f32, c: [u8; 4]) {
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
// STATUS APLIKASI
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Volume,
    Brightness,
}

#[derive(Clone, Copy)]
struct Anim {
    from: f32,
    to: f32,
    start: Instant,
    dur: f32,
    elastic: bool,
}

struct App {
    hwnd: HWND,
    x0: i32,
    y0: i32,
    wpx: i32,
    hpx: i32,
    scale: f32,
    screen_dc: HDC,
    mem_dc: HDC,
    bits: *mut u8,
    pixmap: Pixmap,
    t: f32,
    open: bool,
    anim: Option<Anim>,
    leave_at: Option<Instant>,
    body_pts: Vec<Pt>,
    hl: [f32; 3],
    hud: Option<(Kind, Option<i32>, Instant)>,
    bright: Brightness,
    interval: u32,
    dirty: bool,
}

impl App {
    unsafe fn new(hwnd: HWND, x0: i32, y0: i32, wpx: i32, hpx: i32, scale: f32) -> App {
        let screen_dc = GetDC(null_mut());
        let mem_dc = CreateCompatibleDC(screen_dc);
        let mut bmi: BITMAPINFO = mem::zeroed();
        bmi.bmiHeader.biSize = mem::size_of::<winapi::um::wingdi::BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = wpx;
        bmi.bmiHeader.biHeight = -hpx; // negatif = baris dari atas
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = 0;
        let mut bits: *mut winapi::ctypes::c_void = null_mut();
        let bmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        SelectObject(mem_dc, bmp as *mut _);
        App {
            hwnd,
            x0,
            y0,
            wpx,
            hpx,
            scale,
            screen_dc,
            mem_dc,
            bits: bits as *mut u8,
            pixmap: Pixmap::new(wpx as u32, hpx as u32).unwrap(),
            t: 0.0,
            open: false,
            anim: None,
            leave_at: None,
            body_pts: body_points(0.0),
            hl: [0.0; 3],
            hud: None,
            bright: Brightness::new(),
            interval: 30,
            dirty: true,
        }
    }

    fn cursor_local(&self) -> (f32, f32) {
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut p);
        }
        (
            (p.x - self.x0) as f32 / self.scale,
            (p.y - self.y0) as f32 / self.scale,
        )
    }

    fn set_interval(&mut self, ms: u32) {
        if ms != self.interval {
            self.interval = ms;
            unsafe {
                SetTimer(self.hwnd, 1, ms, None);
            }
        }
    }

    fn set_open(&mut self, want: bool) {
        if want == self.open {
            return;
        }
        self.open = want;
        self.anim = Some(Anim {
            from: self.t,
            to: if want { 1.0 } else { 0.0 },
            start: Instant::now(),
            dur: if want { 0.8 } else { 0.28 },
            elastic: want,
        });
    }

    fn step_anim(&mut self) {
        let a = match self.anim {
            Some(a) => a,
            None => return,
        };
        let p = (a.start.elapsed().as_secs_f32() / a.dur).min(1.0);
        let e = if a.elastic { out_elastic(p) } else { bezier_ease(0.65, 0.0, 0.35, 1.0, p) };
        self.t = (a.from + (a.to - a.from) * e).max(0.0);
        if p >= 1.0 {
            self.t = a.to;
            self.anim = None;
        }
        self.body_pts = body_points(self.t);
        self.dirty = true;
    }

    /// Dipanggil oleh timer: cek kursor, jalankan animasi, gambar ulang bila perlu.
    fn tick(&mut self) {
        let now = Instant::now();
        self.step_anim();
        let (mx, my) = self.cursor_local();

        let in_body = point_in_poly(&self.body_pts, mx, my);
        let dist = ((mx - CX) * (mx - CX) + (my - ARCH_CY) * (my - ARCH_CY)).sqrt();
        let in_hold = self.open && dist <= HOLD_R;
        let hovering = if self.open { in_body || in_hold } else { in_trigger(mx, my) };

        if hovering {
            self.leave_at = None;
            self.set_open(true);
        } else if self.open {
            match self.leave_at {
                None => {
                    self.leave_at = Some(now);
                }
                Some(t0) => {
                    if now.duration_since(t0).as_secs_f32() > LEAVE_DELAY {
                        self.leave_at = None;
                        self.set_open(false);
                    }
                }
            }
        }

        // sorotan sektor yang disentuh kursor
        let sec = if self.open && self.t > 0.85 { sector_at(mx, my) } else { -1 };
        let mut moving = false;
        for i in 0..3 {
            let target = if sec == i as i32 { 1.0 } else { 0.0 };
            let mut nv = self.hl[i] + (target - self.hl[i]) * 0.35;
            if (nv - target).abs() < 0.01 {
                nv = target;
            }
            if nv != self.hl[i] {
                self.hl[i] = nv;
                self.dirty = true;
            }
            if nv != target {
                moving = true;
            }
        }

        if let Some((_, _, until)) = self.hud {
            if now >= until {
                self.hud = None;
            }
            self.dirty = true;
        }

        let fast = self.anim.is_some() || self.hud.is_some() || moving;
        self.set_interval(if fast { 10 } else { 30 });
        if self.dirty {
            self.refresh();
        }
    }

    /// Scroll: sisi kanan = volume, sisi kiri = kecerahan.
    fn on_wheel(&mut self, delta: i16) {
        let (mx, _) = self.cursor_local();
        let steps = delta as f32 / 120.0;
        let (kind, val) = if mx >= CX {
            (Kind::Volume, change_volume(steps * VOLUME_STEP))
        } else {
            (Kind::Brightness, self.bright.change(steps * BRIGHT_STEP))
        };
        self.hud = Some((kind, val, Instant::now() + Duration::from_millis(HUD_MS)));
        self.dirty = true;
        self.set_interval(10);
        self.refresh();
    }

    fn refresh(&mut self) {
        self.render();
        unsafe {
            self.present();
        }
        self.dirty = false;
    }

    // --- menggambar ---------------------------------------------------------
    fn render(&mut self) {
        self.pixmap.fill(Color::TRANSPARENT);
        let base = Transform::from_scale(self.scale, self.scale);
        let t = self.t;

        // area "hampir transparan" di ruang tengah busur supaya scroll tetap diterima
        if self.open {
            if let Some(p) = circle_path(CX, ARCH_CY, HOLD_R) {
                fill(&mut self.pixmap, &p, [0, 0, 0, 2], base);
            }
        }

        if let Some(body) = poly_path(&self.body_pts, true) {
            fill(&mut self.pixmap, &body, BODY, base);
            if t > 0.05 {
                self.paint_sectors(base, t);
            }
            stroke(&mut self.pixmap, &body, BORDER, 1.0, base);
        }
        self.paint_hud(base);
    }

    fn paint_sectors(&mut self, base: Transform, t: f32) {
        let fade = ((t - 0.5) / 0.4).clamp(0.0, 1.0);
        if fade <= 0.0 {
            return;
        }
        // sorotan
        for i in 0..3 {
            if self.hl[i] < 0.01 {
                continue;
            }
            let (s0, s1) = SECTORS[i];
            let pts = band_outline(s0, s1, t, 20, i == 0, i == 2);
            if let Some(p) = poly_path(&pts, true) {
                let a = (38.0 * self.hl[i] * fade) as u8;
                fill(&mut self.pixmap, &p, [255, 255, 255, a], base);
            }
        }
        // garis pemisah
        let seam = (40.0 * fade) as u8;
        for s in [-1.0f32 / 3.0, 1.0f32 / 3.0] {
            let (a, b) = edge_points(s, t);
            line(&mut self.pixmap, base, [255, 255, 255, seam], 1.4, a, b);
        }
        // ikon
        for i in 0..3 {
            let (x, y, _, _) = centerline(SECTOR_S[i], t);
            let hl = self.hl[i];
            let col = [255u8, 255, 255, ((170.0 + 85.0 * hl) * fade) as u8];
            let k = t.min(1.15) * (1.0 + 0.14 * hl);
            let tf = base.pre_translate(x, y).pre_scale(k, k);
            match i {
                0 => icon_note(&mut self.pixmap, tf, col),
                1 => icon_sliders(&mut self.pixmap, tf, col),
                _ => icon_calendar(&mut self.pixmap, tf, col),
            }
        }
    }

    fn paint_hud(&mut self, base: Transform) {
        let (kind, value, until) = match self.hud {
            Some(h) => h,
            None => return,
        };
        let now = Instant::now();
        if now >= until {
            return;
        }
        let left = until.duration_since(now).as_secs_f32();
        let fade = (left / 0.3).min(1.0);
        let al = |v: f32| (v * fade) as u8;

        let mut bottom = 0.0f32;
        for p in &self.body_pts {
            if p.1 > bottom {
                bottom = p.1;
            }
        }
        let (rx, ry, rw, rh) = (CX - 92.0, bottom + 10.0, 184.0f32, 34.0f32);
        let cy = ry + rh / 2.0;

        if let Some(p) = rounded_rect(rx, ry, rw, rh, 17.0) {
            fill(&mut self.pixmap, &p, [8, 8, 10, al(245.0)], base);
            stroke(&mut self.pixmap, &p, [255, 255, 255, al(30.0)], 1.0, base);
        }

        let white = [255u8, 255, 255, al(235.0)];
        let icon_tf = base.pre_translate(rx + 24.0, cy);
        if kind == Kind::Volume {
            icon_speaker(&mut self.pixmap, icon_tf, white);
        } else {
            icon_sun(&mut self.pixmap, icon_tf, white);
        }

        let track_x = rx + 44.0;
        if let Some(p) = rounded_rect(track_x, cy - 3.0, 80.0, 6.0, 3.0) {
            fill(&mut self.pixmap, &p, [255, 255, 255, al(40.0)], base);
        }
        let text = match value {
            Some(v) => {
                let w = (80.0 * (v as f32) / 100.0).max(6.0);
                if let Some(p) = rounded_rect(track_x, cy - 3.0, w, 6.0, 3.0) {
                    fill(&mut self.pixmap, &p, white, base);
                }
                format!("{}%", v)
            }
            None => String::from("N/A"),
        };
        draw_text(&mut self.pixmap, base, &text, rx + 172.0, cy, white);
    }

    /// Kirim gambar ke jendela (per-piksel transparan).
    unsafe fn present(&mut self) {
        let src = self.pixmap.data();
        let dst = std::slice::from_raw_parts_mut(self.bits, src.len());
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let mut pt_dst = POINT { x: self.x0, y: self.y0 };
        let mut size = SIZE { cx: self.wpx, cy: self.hpx };
        let mut pt_src = POINT { x: 0, y: 0 };
        let mut blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA,
        };
        UpdateLayeredWindow(
            self.hwnd,
            self.screen_dc,
            &mut pt_dst,
            &mut size,
            self.mem_dc,
            &mut pt_src,
            0,
            &mut blend,
            ULW_ALPHA,
        );
    }
}

// ---------------------------------------------------------------------------
// JENDELA WINDOWS
// ---------------------------------------------------------------------------
thread_local! {
    static APP: RefCell<Option<App>> = RefCell::new(None);
}

fn with_app<F: FnOnce(&mut App)>(f: F) {
    APP.with(|cell| {
        if let Ok(mut guard) = cell.try_borrow_mut() {
            if let Some(app) = guard.as_mut() {
                f(app);
            }
        }
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    let text = wide("Keluar dari DYLAND");
    AppendMenuW(menu, MF_STRING, 1, text.as_ptr());
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
    DestroyMenu(menu);
    if cmd == 1 {
        PostQuitMessage(0);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            with_app(|a| a.tick());
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16;
            with_app(|a| a.on_wheel(delta));
            0
        }
        WM_RBUTTONUP => {
            show_menu(hwnd);
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    unsafe {
        SetProcessDPIAware();
        CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED);

        let dc = GetDC(null_mut());
        let dpi = GetDeviceCaps(dc, LOGPIXELSX);
        ReleaseDC(null_mut(), dc);
        let scale = (dpi as f32 / 96.0).max(1.0);

        let wpx = (W * scale).ceil() as i32;
        let hpx = (H * scale).ceil() as i32;
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let x0 = (screen_w - wpx) / 2;
        let y0 = 0;

        let hinst = GetModuleHandleW(null());
        let class = wide("DylandIsland");
        let wc = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);

        let title = wide("DYLAND");
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x0,
            y0,
            wpx,
            hpx,
            null_mut(),
            null_mut(),
            hinst,
            null_mut(),
        );
        if hwnd.is_null() {
            return;
        }

        let app = App::new(hwnd, x0, y0, wpx, hpx, scale);
        APP.with(|cell| {
            *cell.borrow_mut() = Some(app);
        });
        with_app(|a| a.refresh());
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, 1, 30, None);

        let mut msg: MSG = mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
