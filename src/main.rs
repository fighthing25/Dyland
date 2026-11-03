//! DYLAND - Dynamic Island untuk Windows (Rust)
//!
//! Tahap 2: sektor bisa diklik dan membuka panel (Media, Sistem, Pusat data),
//! wajah Venom di kapsul, serta fitur-fitur sistem. Semua digambar lewat kode.
//! Tanpa WebView2, tanpa file gambar.

#![windows_subsystem = "windows"]

mod cal;
mod geo;
mod sys;
mod ui;

use geo::*;
use std::cell::RefCell;
use std::mem;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use tiny_skia::{Color, Pixmap, Transform};
use ui::*;
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HDC, HWND, POINT, SIZE};
use winapi::um::combaseapi::CoInitializeEx;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::objbase::COINIT_APARTMENTTHREADED;
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateDIBSection, GetDeviceCaps, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BLENDFUNCTION, DIB_RGB_COLORS, LOGPIXELSX,
};
use winapi::um::winuser::*;

// ---------------------------------------------------------------------------
// PENGATURAN
// ---------------------------------------------------------------------------
const LEAVE_DELAY: f32 = 0.18; // detik sebelum busur menutup
const PANEL_LEAVE: f32 = 0.5; // detik sebelum panel menutup setelah kursor pergi
const VOLUME_STEP: f32 = 4.0;
const BRIGHT_STEP: f32 = 5.0;
const HUD_MS: u64 = 1300;
const PANEL_W: f32 = 340.0;
const ACCENT: [u8; 3] = [110, 185, 255];

fn page_h(p: Page) -> f32 {
    match p {
        Page::Media => 196.0,
        Page::System => 196.0,
        Page::Hub => 266.0,
    }
}

fn page_title(p: Page) -> &'static str {
    match p {
        Page::Media => "Media",
        Page::System => "Sistem",
        Page::Hub => "Pusat data",
    }
}

// ---------------------------------------------------------------------------
// TIPE DATA
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Media,
    System,
    Hub,
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    None,
    Close,
    Prev,
    PlayPause,
    Next,
    MicToggle,
    Sound,
    Screenshot,
    Awake,
    Lock,
    Sleep,
    Restart,
    Shutdown,
    Tab(u8),
    CalPrev,
    CalNext,
    ClipCopy(usize),
}

#[derive(Clone)]
struct Widget {
    r: (f32, f32, f32, f32),
    act: Act,
    label: String,
    icon: Icon,
    on: bool,
    enabled: bool,
}

fn wd(r: (f32, f32, f32, f32), act: Act, label: &str, icon: Icon) -> Widget {
    Widget { r, act, label: label.to_string(), icon, on: false, enabled: true }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Volume,
    Brightness,
}

#[derive(Clone, Copy, PartialEq)]
enum Face {
    Normal,
    Low,
    Critical,
    Charging,
    Full,
    Hot,
}

#[derive(Clone, Copy)]
struct Anim {
    from: f32,
    to: f32,
    start: Instant,
    dur: f32,
    elastic: bool,
}

fn eval_anim(a: &Anim) -> (f32, bool) {
    let p = (a.start.elapsed().as_secs_f32() / a.dur).min(1.0);
    let e = if a.elastic { out_elastic(p) } else { bezier_ease(0.65, 0.0, 0.35, 1.0, p) };
    ((a.from + (a.to - a.from) * e).max(0.0), p >= 1.0)
}

struct ClipItem {
    text: String,
    at: Instant,
    otp: bool,
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
    text: TextEngine,
    epoch: Instant,

    t: f32,
    open: bool,
    anim: Option<Anim>,
    leave_at: Option<Instant>,
    body_pts: Vec<Pt>,
    hl: [f32; 3],

    view: Option<Page>,
    panel_open: bool,
    pv: f32,
    panel_anim: Option<Anim>,
    panel_leave: Option<Instant>,
    hover_w: Option<usize>,
    confirm: Option<(Act, Instant)>,

    hub_tab: u8,
    cal_year: i32,
    cal_month: u32,

    hud: Option<(Kind, Option<i32>, Instant)>,
    toast: Option<(String, Instant)>,
    bright: sys::Brightness,
    awake: bool,
    mic_muted: Option<bool>,
    caps: bool,
    clips: Vec<ClipItem>,

    prayer: sys::PrayerShared,
    city: String,
    country: String,
    prayer_try: Option<Instant>,
    notified: String,
    last_sec: u32,

    power: sys::PowerInfo,
    cpu: sys::CpuMeter,
    hot_count: u32,
    last_sys: Instant,

    face_dx: f32,
    face_dy: f32,
    blink_until: Option<Instant>,
    next_blink: Instant,
    rng: u32,
    crit_phase: bool,

    interval: u32,
    dirty: bool,
}

fn load_config() -> (String, String) {
    let mut city = String::from("Jakarta");
    let mut country = String::from("Indonesia");
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = format!("{}\\Dyland", appdata);
        let path = format!("{}\\config.txt", dir);
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                for line in content.lines() {
                    if let Some((k, v)) = line.split_once('=') {
                        let v = v.trim();
                        if v.is_empty() {
                            continue;
                        }
                        match k.trim() {
                            "kota" => city = v.to_string(),
                            "negara" => country = v.to_string(),
                            _ => {}
                        }
                    }
                }
            }
            Err(_) => {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(&path, "kota=Jakarta\r\nnegara=Indonesia\r\n");
            }
        }
    }
    (city, country)
}

impl App {
    unsafe fn new(hwnd: HWND, x0: i32, y0: i32, wpx: i32, hpx: i32, scale: f32) -> App {
        let screen_dc = GetDC(null_mut());
        let mem_dc = CreateCompatibleDC(screen_dc);
        let mut bmi: BITMAPINFO = mem::zeroed();
        bmi.bmiHeader.biSize = mem::size_of::<winapi::um::wingdi::BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = wpx;
        bmi.bmiHeader.biHeight = -hpx;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = 0;
        let mut bits: *mut winapi::ctypes::c_void = null_mut();
        let bmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        SelectObject(mem_dc, bmp as *mut _);

        let lt = sys::now_local();
        let (city, country) = load_config();
        let now = Instant::now();
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
            text: TextEngine::new(),
            epoch: now,
            t: 0.0,
            open: false,
            anim: None,
            leave_at: None,
            body_pts: body_points(0.0),
            hl: [0.0; 3],
            view: None,
            panel_open: false,
            pv: 0.0,
            panel_anim: None,
            panel_leave: None,
            hover_w: None,
            confirm: None,
            hub_tab: 0,
            cal_year: lt.year,
            cal_month: lt.month,
            hud: None,
            toast: None,
            bright: sys::Brightness::new(),
            awake: false,
            mic_muted: None,
            caps: sys::caps_on(),
            clips: Vec::new(),
            prayer: sys::new_prayer(),
            city,
            country,
            prayer_try: None,
            notified: String::new(),
            last_sec: 99,
            power: sys::power_info(),
            cpu: sys::CpuMeter::new(),
            hot_count: 0,
            last_sys: now,
            face_dx: 0.0,
            face_dy: 0.0,
            blink_until: None,
            next_blink: now + Duration::from_millis(2500),
            rng: 2463534242,
            crit_phase: false,
            interval: 30,
            dirty: true,
        }
    }

    // --- utilitas -------------------------------------------------------------
    fn cursor_phys(&self) -> (i32, i32) {
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut p);
        }
        (p.x, p.y)
    }

    fn cursor_local(&self) -> (f32, f32) {
        let (px, py) = self.cursor_phys();
        ((px - self.x0) as f32 / self.scale, (py - self.y0) as f32 / self.scale)
    }

    fn rnd(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x % 10000) as f32 / 10000.0
    }

    fn set_interval(&mut self, ms: u32) {
        if ms != self.interval {
            self.interval = ms;
            unsafe {
                SetTimer(self.hwnd, 1, ms, None);
            }
        }
    }

    fn say(&mut self, s: &str, secs: f32) {
        self.toast = Some((s.to_string(), Instant::now() + Duration::from_millis((secs * 1000.0) as u64)));
        self.dirty = true;
    }

    fn txt(&mut self, s: &str, x: f32, base_y: f32, size: f32, c: [u8; 4]) {
        let sc = self.scale;
        self.text.draw(&mut self.pixmap, s, x * sc, base_y * sc, size * sc, c);
    }

    fn txt_w(&mut self, s: &str, size: f32) -> f32 {
        let sc = self.scale;
        self.text.width(s, size * sc) / sc
    }

    fn txt_c(&mut self, s: &str, cx: f32, base_y: f32, size: f32, c: [u8; 4]) {
        let w = self.txt_w(s, size);
        self.txt(s, cx - w / 2.0, base_y, size, c);
    }

    fn fit(&mut self, s: &str, size: f32, maxw: f32) -> String {
        if self.txt_w(s, size) <= maxw {
            return s.to_string();
        }
        let mut out = s.to_string();
        while !out.is_empty() {
            out.pop();
            let cand = format!("{}…", out);
            if self.txt_w(&cand, size) <= maxw {
                return cand;
            }
        }
        String::new()
    }

    // --- bentuk panel ---------------------------------------------------------
    fn panel_rect(&self) -> (f32, f32, f32, f32) {
        let ph = match self.view {
            Some(p) => page_h(p),
            None => 196.0,
        };
        let w = CAP_W + (PANEL_W - CAP_W) * self.pv;
        let h = CAP_H + (ph - CAP_H) * self.pv;
        (CX - w / 2.0, MARGIN, w, h)
    }

    fn shape_bottom(&self) -> f32 {
        let mut bottom = 0.0f32;
        for p in &self.body_pts {
            if p.1 > bottom {
                bottom = p.1;
            }
        }
        if self.view.is_some() {
            let (_, y, _, h) = self.panel_rect();
            if y + h > bottom {
                bottom = y + h;
            }
        }
        bottom
    }

    // --- animasi busur/panel ----------------------------------------------------
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

    fn open_page(&mut self, p: Page) {
        self.view = Some(p);
        self.panel_open = true;
        self.panel_leave = None;
        self.hover_w = None;
        self.set_open(false);
        self.panel_anim = Some(Anim {
            from: self.pv,
            to: 1.0,
            start: Instant::now(),
            dur: 0.75,
            elastic: true,
        });
        match p {
            Page::Media => {
                self.mic_muted = sys::mic_muted();
            }
            Page::Hub => {
                let lt = sys::now_local();
                self.cal_year = lt.year;
                self.cal_month = lt.month;
            }
            Page::System => {}
        }
        self.dirty = true;
    }

    fn close_panel(&mut self) {
        if !self.panel_open {
            return;
        }
        self.panel_open = false;
        self.panel_anim = Some(Anim {
            from: self.pv,
            to: 0.0,
            start: Instant::now(),
            dur: 0.26,
            elastic: false,
        });
    }

    fn step_anims(&mut self) {
        if let Some(a) = self.anim {
            let (v, done) = eval_anim(&a);
            self.t = v;
            if done {
                self.t = a.to;
                self.anim = None;
            }
            self.body_pts = body_points(self.t);
            self.dirty = true;
        }
        if let Some(a) = self.panel_anim {
            let (v, done) = eval_anim(&a);
            self.pv = v;
            if done {
                self.pv = a.to;
                self.panel_anim = None;
                if a.to < 0.01 {
                    self.view = None;
                    self.hover_w = None;
                    self.confirm = None;
                }
            }
            self.dirty = true;
        }
    }

    // --- daftar tombol di panel ---------------------------------------------------
    fn widgets(&self) -> Vec<Widget> {
        let mut v: Vec<Widget> = Vec::new();
        let page = match self.view {
            Some(p) => p,
            None => return v,
        };
        let x0 = CX - PANEL_W / 2.0;
        let y0 = MARGIN;
        v.push(wd((x0 + PANEL_W - 38.0, y0 + 8.0, 28.0, 26.0), Act::Close, "", Icon::Close));
        match page {
            Page::Media => {
                let bw = 64.0;
                let gap = 12.0;
                let sx = CX - (3.0 * bw + 2.0 * gap) / 2.0;
                let y = y0 + 46.0;
                v.push(wd((sx, y, bw, 54.0), Act::Prev, "", Icon::Prev));
                v.push(wd((sx + bw + gap, y, bw, 54.0), Act::PlayPause, "", Icon::PlayPause));
                v.push(wd((sx + 2.0 * (bw + gap), y, bw, 54.0), Act::Next, "", Icon::Next));
                let y2 = y + 54.0 + 12.0;
                let w2 = 152.0;
                let muted = self.mic_muted == Some(true);
                let mut m = wd(
                    (CX - w2 - 6.0, y2, w2, 52.0),
                    Act::MicToggle,
                    if muted { "Mic: bisu" } else { "Mic: aktif" },
                    if muted { Icon::MicOff } else { Icon::Mic },
                );
                m.on = muted;
                v.push(m);
                v.push(wd((CX + 6.0, y2, w2, 52.0), Act::Sound, "Output suara", Icon::Speaker));
            }
            Page::System => {
                let bw = 74.0;
                let gap = 8.0;
                let sx = CX - (4.0 * bw + 3.0 * gap) / 2.0;
                let y = y0 + 48.0;
                v.push(wd((sx, y, bw, 62.0), Act::Screenshot, "Screenshot", Icon::Camera));
                let mut a = wd((sx + (bw + gap), y, bw, 62.0), Act::Awake, "Awake", Icon::Coffee);
                a.on = self.awake;
                v.push(a);
                let mut c = wd(
                    (sx + 2.0 * (bw + gap), y, bw, 62.0),
                    Act::None,
                    if self.caps { "Caps: ON" } else { "Caps: OFF" },
                    Icon::None,
                );
                c.on = self.caps;
                c.enabled = false;
                v.push(c);
                v.push(wd((sx + 3.0 * (bw + gap), y, bw, 62.0), Act::Lock, "Kunci", Icon::Lock));
                let bw2 = 104.0;
                let sx2 = CX - (3.0 * bw2 + 2.0 * gap) / 2.0;
                let y2 = y + 62.0 + 8.0;
                v.push(wd((sx2, y2, bw2, 62.0), Act::Sleep, "Tidur", Icon::Moon));
                v.push(wd((sx2 + (bw2 + gap), y2, bw2, 62.0), Act::Restart, "Restart", Icon::Restart));
                v.push(wd((sx2 + 2.0 * (bw2 + gap), y2, bw2, 62.0), Act::Shutdown, "Matikan", Icon::Power));
            }
            Page::Hub => {
                let tw = 100.0;
                let gap = 8.0;
                let sx = CX - (3.0 * tw + 2.0 * gap) / 2.0;
                let names = ["Shalat", "Kalender", "Clipboard"];
                for i in 0..3 {
                    let mut w = wd((sx + (i as f32) * (tw + gap), y0 + 44.0, tw, 26.0), Act::Tab(i as u8), names[i], Icon::None);
                    w.on = self.hub_tab == i as u8;
                    v.push(w);
                }
                if self.hub_tab == 1 {
                    v.push(wd((CX - 158.0, y0 + 78.0, 30.0, 24.0), Act::CalPrev, "", Icon::Left));
                    v.push(wd((CX + 128.0, y0 + 78.0, 30.0, 24.0), Act::CalNext, "", Icon::Right));
                } else if self.hub_tab == 2 {
                    let n = self.clips.len().min(5);
                    for i in 0..n {
                        v.push(wd((CX - 158.0, y0 + 82.0 + (i as f32) * 30.0, 316.0, 26.0), Act::ClipCopy(i), "", Icon::None));
                    }
                }
            }
        }
        v
    }

    fn widget_at(&self, mx: f32, my: f32) -> Option<usize> {
        let ws = self.widgets();
        for (i, w) in ws.iter().enumerate() {
            let (x, y, wdt, ht) = w.r;
            if w.enabled && mx >= x && mx <= x + wdt && my >= y && my <= y + ht {
                return Some(i);
            }
        }
        None
    }

    // --- klik dan aksi ----------------------------------------------------------------
    fn on_click(&mut self) {
        let (mx, my) = self.cursor_local();
        if self.view.is_some() {
            if self.panel_open && self.pv > 0.85 {
                if let Some(i) = self.widget_at(mx, my) {
                    let act = self.widgets()[i].act;
                    self.perform(act);
                }
            }
            return;
        }
        if self.open && self.t > 0.85 {
            let s = sector_at(mx, my);
            if s == 0 {
                self.open_page(Page::Media);
            } else if s == 1 {
                self.open_page(Page::System);
            } else if s == 2 {
                self.open_page(Page::Hub);
            }
        }
    }

    fn confirm_or(&mut self, act: Act, run: fn()) {
        let ok = match self.confirm {
            Some((a, t0)) => a == act && t0.elapsed().as_secs_f32() < 3.0,
            None => false,
        };
        if ok {
            self.confirm = None;
            run();
        } else {
            self.confirm = Some((act, Instant::now()));
        }
        self.dirty = true;
    }

    fn perform(&mut self, act: Act) {
        match act {
            Act::None => {}
            Act::Close => self.close_panel(),
            Act::Prev => sys::media_key(0xB1),
            Act::PlayPause => sys::media_key(0xB3),
            Act::Next => sys::media_key(0xB0),
            Act::MicToggle => {
                self.mic_muted = sys::mic_toggle();
                match self.mic_muted {
                    Some(true) => self.say("Mikrofon dibisukan", 2.0),
                    Some(false) => self.say("Mikrofon aktif", 2.0),
                    None => self.say("Mikrofon tidak ditemukan", 2.5),
                }
            }
            Act::Sound => sys::open_sound_settings(),
            Act::Screenshot => {
                let r = sys::screenshot(self.hwnd);
                match r {
                    Some(_) => self.say("Tersimpan di Gambar\\Dyland", 3.0),
                    None => self.say("Screenshot gagal", 3.0),
                }
            }
            Act::Awake => {
                self.awake = !self.awake;
                sys::set_awake(self.awake);
                if self.awake {
                    self.say("Awake aktif: layar tidak akan tidur", 3.0);
                } else {
                    self.say("Awake dimatikan", 2.0);
                }
            }
            Act::Lock => sys::lock_pc(),
            Act::Sleep => self.confirm_or(Act::Sleep, sys::sleep_pc),
            Act::Restart => self.confirm_or(Act::Restart, sys::restart_pc),
            Act::Shutdown => self.confirm_or(Act::Shutdown, sys::shutdown_pc),
            Act::Tab(i) => {
                self.hub_tab = i;
                self.hover_w = None;
            }
            Act::CalPrev => {
                if self.cal_month == 1 {
                    self.cal_month = 12;
                    self.cal_year -= 1;
                } else {
                    self.cal_month -= 1;
                }
            }
            Act::CalNext => {
                if self.cal_month == 12 {
                    self.cal_month = 1;
                    self.cal_year += 1;
                } else {
                    self.cal_month += 1;
                }
            }
            Act::ClipCopy(i) => {
                let t = self.clips.get(i).map(|c| c.text.clone());
                if let Some(t) = t {
                    sys::clipboard_set(&t);
                    self.say("Disalin", 1.5);
                }
            }
        }
        self.dirty = true;
    }

    fn on_clipboard(&mut self) {
        let raw = match sys::clipboard_get(self.hwnd) {
            Some(t) => t,
            None => return,
        };
        let cleaned: String = raw.replace('\r', " ").replace('\n', " ").replace('\t', " ");
        let trimmed = cleaned.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let otp = trimmed.len() >= 4 && trimmed.len() <= 8 && trimmed.chars().all(|c| c.is_ascii_digit());
        self.clips.retain(|c| c.text != trimmed);
        self.clips.insert(0, ClipItem { text: trimmed, at: Instant::now(), otp });
        self.clips.truncate(10);
        self.dirty = true;
    }

    /// Scroll: sisi kanan = volume, sisi kiri = kecerahan.
    fn on_wheel(&mut self, delta: i16) {
        let (mx, _) = self.cursor_local();
        let steps = delta as f32 / 120.0;
        let (kind, val) = if mx >= CX {
            (Kind::Volume, sys::change_volume(steps * VOLUME_STEP))
        } else {
            (Kind::Brightness, self.bright.change(steps * BRIGHT_STEP))
        };
        self.hud = Some((kind, val, Instant::now() + Duration::from_millis(HUD_MS)));
        self.dirty = true;
        self.set_interval(10);
        self.refresh();
    }

    // --- wajah Venom ---------------------------------------------------------------------
    fn face_alpha(&self) -> f32 {
        ((1.0 - self.t * 1.8).clamp(0.0, 1.0)) * ((1.0 - self.pv * 3.0).clamp(0.0, 1.0))
    }

    fn face_mode(&self) -> Face {
        let p = self.power;
        if p.has_battery {
            if p.plugged {
                return if p.percent >= 100 { Face::Full } else { Face::Charging };
            }
            if p.percent < 10 {
                return Face::Critical;
            }
            if p.percent <= 20 {
                return Face::Low;
            }
        }
        if self.hot_count >= 2 {
            return Face::Hot;
        }
        Face::Normal
    }

    fn update_face(&mut self, now: Instant) {
        if self.face_alpha() < 0.02 {
            return;
        }
        let cxp = self.x0 as f32 + CX * self.scale;
        let cyp = self.y0 as f32 + CAP_CY * self.scale;
        let (px, py) = self.cursor_phys();
        let dx = px as f32 - cxp;
        let dy = py as f32 - cyp;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let k = (dist / (150.0 * self.scale)).min(1.0) * 2.4;
        let tx = dx / dist * k;
        let ty = dy / dist * k * 0.7;
        let nx = self.face_dx + (tx - self.face_dx) * 0.3;
        let ny = self.face_dy + (ty - self.face_dy) * 0.3;
        let mut changed = (nx - self.face_dx).abs() > 0.05 || (ny - self.face_dy).abs() > 0.05;
        self.face_dx = nx;
        self.face_dy = ny;

        let bu = self.blink_until;
        match bu {
            Some(u) => {
                if now >= u {
                    self.blink_until = None;
                    let r = self.rnd();
                    self.next_blink = now + Duration::from_millis(1800 + (r * 3500.0) as u64);
                    changed = true;
                }
            }
            None => {
                if now >= self.next_blink {
                    self.blink_until = Some(now + Duration::from_millis(130));
                    changed = true;
                }
            }
        }
        if self.face_mode() == Face::Critical {
            let phase = (self.epoch.elapsed().as_millis() / 280) % 2 == 0;
            if phase != self.crit_phase {
                self.crit_phase = phase;
                changed = true;
            }
        }
        if changed {
            self.dirty = true;
        }
    }

    // --- pemantauan berkala ---------------------------------------------------------------
    fn update_sys(&mut self, now: Instant) {
        let c = sys::caps_on();
        if c != self.caps {
            self.caps = c;
            self.dirty = true;
        }
        if now.duration_since(self.last_sys).as_secs_f32() > 2.0 {
            self.last_sys = now;
            self.power = sys::power_info();
            let u = self.cpu.usage();
            if u > 90.0 {
                self.hot_count += 1;
            } else {
                self.hot_count = 0;
            }
            self.dirty = true;
        }
        let before = self.clips.len();
        self.clips.retain(|c| !(c.otp && now.duration_since(c.at).as_secs() > 60));
        if self.clips.len() != before {
            self.dirty = true;
        }
        let expired = match self.confirm {
            Some((_, t0)) => t0.elapsed().as_secs_f32() >= 3.0,
            None => false,
        };
        if expired {
            self.confirm = None;
            self.dirty = true;
        }
    }

    fn per_second(&mut self, now: Instant) {
        let lt = sys::now_local();
        if lt.sec == self.last_sec {
            return;
        }
        self.last_sec = lt.sec;
        if self.view == Some(Page::Hub) && self.hub_tab == 0 {
            self.dirty = true;
        }

        let daykey = (lt.year as u32) * 10000 + lt.month * 100 + lt.day;
        let need = match self.prayer.lock() {
            Ok(g) => g.day != daykey,
            Err(_) => false,
        };
        let can = match self.prayer_try {
            None => true,
            Some(t0) => now.duration_since(t0).as_secs() > 300,
        };
        if need && can {
            self.prayer_try = Some(now);
            sys::fetch_prayer(self.prayer.clone(), self.city.clone(), self.country.clone(), daykey);
        }

        let times: Vec<(String, u32)> = match self.prayer.lock() {
            Ok(g) => {
                if g.status == 1 {
                    g.times.clone()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        };
        let nowmin = lt.hour * 60 + lt.min;
        let days = cal::days_from_civil(lt.year, lt.month, lt.day);
        let ramadan = cal::hijri(days).1 == 9;
        for (i, (name, m)) in times.iter().enumerate() {
            if *m != nowmin || (i == 0 && !ramadan) {
                continue;
            }
            let key = format!("{}-{}", daykey, name);
            if key != self.notified {
                self.notified = key;
                sys::beep();
                let msg = if ramadan && i == 4 {
                    "Waktu berbuka puasa (Maghrib)".to_string()
                } else if i == 0 {
                    "Waktu imsak".to_string()
                } else {
                    format!("Waktunya shalat {}", name)
                };
                self.say(&msg, 10.0);
            }
        }
    }

    // --- tick utama (dipanggil timer) ---------------------------------------------------------
    fn tick(&mut self) {
        let now = Instant::now();
        self.step_anims();
        let (mx, my) = self.cursor_local();
        self.update_sys(now);
        self.update_face(now);
        self.per_second(now);

        if self.view.is_some() {
            let (px, py, pw, ph) = self.panel_rect();
            let inside = mx >= px - 14.0 && mx <= px + pw + 14.0 && my >= py - 14.0 && my <= py + ph + 14.0;
            if self.panel_open {
                if inside {
                    self.panel_leave = None;
                } else {
                    match self.panel_leave {
                        None => {
                            self.panel_leave = Some(now);
                        }
                        Some(t0) => {
                            if now.duration_since(t0).as_secs_f32() > PANEL_LEAVE {
                                self.panel_leave = None;
                                self.close_panel();
                            }
                        }
                    }
                }
            }
            let hw = if self.pv > 0.85 { self.widget_at(mx, my) } else { None };
            if hw != self.hover_w {
                self.hover_w = hw;
                self.dirty = true;
            }
        } else {
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
        }

        let sec = if self.view.is_none() && self.open && self.t > 0.85 { sector_at(mx, my) } else { -1 };
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
        let toast_expired = match &self.toast {
            Some((_, u)) => now >= *u,
            None => false,
        };
        if toast_expired {
            self.toast = None;
            self.dirty = true;
        }

        let fast = self.anim.is_some()
            || self.panel_anim.is_some()
            || self.hud.is_some()
            || self.toast.is_some()
            || moving;
        self.set_interval(if fast { 10 } else { 30 });
        if self.dirty {
            self.refresh();
        }
    }

    fn refresh(&mut self) {
        self.render();
        unsafe {
            self.present();
        }
        self.dirty = false;
    }

    // --- menggambar ---------------------------------------------------------------------------
    fn render(&mut self) {
        self.pixmap.fill(Color::TRANSPARENT);
        let base = Transform::from_scale(self.scale, self.scale);
        let t = self.t;

        if self.open {
            if let Some(p) = circle_path(CX, ARCH_CY, HOLD_R) {
                fill(&mut self.pixmap, &p, [0, 0, 0, 2], base);
            }
        }

        let show_arch = self.view.is_none() || t > 0.02;
        if show_arch {
            if let Some(body) = poly_path(&self.body_pts, true) {
                fill(&mut self.pixmap, &body, BODY, base);
                if t > 0.05 {
                    self.paint_sectors(base, t);
                }
                if self.pv < 0.05 {
                    stroke(&mut self.pixmap, &body, BORDER, 1.0, base);
                }
            }
        }

        let fa = self.face_alpha();
        if fa > 0.02 {
            self.paint_face(base, fa);
        }
        self.paint_panel(base);
        self.paint_hud(base);
        self.paint_toast(base);
    }

    fn paint_face(&mut self, base: Transform, fa: f32) {
        let mode = self.face_mode();
        let col: [u8; 3] = match mode {
            Face::Low | Face::Critical => [255, 80, 80],
            Face::Full => [90, 255, 150],
            _ => [255, 255, 255],
        };
        let a = (255.0 * fa) as u8;
        let blink = if mode == Face::Critical {
            if self.crit_phase {
                1.0
            } else {
                0.15
            }
        } else if self.blink_until.is_some() {
            0.12
        } else {
            1.0
        };
        let happy = mode == Face::Charging || mode == Face::Full;
        let (dxp, dyp) = (self.face_dx, self.face_dy);

        for sx in [-13.0f32, 13.0f32] {
            let ex = CX + sx;
            let ey = CAP_CY;
            if happy {
                let pts = arc_pts(ex, ey + 3.0, 5.5, 205.0, 335.0, 10);
                if let Some(p) = poly_path(&pts, false) {
                    stroke(&mut self.pixmap, &p, [col[0], col[1], col[2], a], 2.6, base);
                }
            } else {
                if let Some(p) = circle_path(ex, ey, 10.0) {
                    fill(&mut self.pixmap, &p, [col[0], col[1], col[2], (30.0 * fa) as u8], base);
                }
                let tf = base.pre_translate(ex, ey).pre_scale(0.88, 1.2 * blink);
                if let Some(p) = circle_path(0.0, 0.0, 6.0) {
                    fill(&mut self.pixmap, &p, [col[0], col[1], col[2], a], tf);
                }
                if blink > 0.5 {
                    let tf2 = base.pre_translate(ex, ey);
                    if let Some(p) = circle_path(dxp * 0.9, dyp * 0.9, 2.7) {
                        fill(&mut self.pixmap, &p, [8, 8, 10, a], tf2);
                    }
                }
            }
        }
        if mode == Face::Full {
            let pts = arc_pts(CX, CAP_CY + 4.0, 6.0, 25.0, 155.0, 10);
            if let Some(p) = poly_path(&pts, false) {
                stroke(&mut self.pixmap, &p, [col[0], col[1], col[2], a], 2.2, base);
            }
        }
        if mode == Face::Hot {
            let dx = CX + 31.0;
            let dy = CAP_CY - 3.0;
            let drop = [(dx, dy - 6.0), (dx - 3.0, dy), (dx + 3.0, dy)];
            if let Some(p) = poly_path(&drop, true) {
                fill(&mut self.pixmap, &p, [120, 200, 255, a], base);
            }
            if let Some(p) = circle_path(dx, dy + 1.5, 3.0) {
                fill(&mut self.pixmap, &p, [120, 200, 255, a], base);
            }
        }
    }

    fn paint_sectors(&mut self, base: Transform, t: f32) {
        let fade = ((t - 0.5) / 0.4).clamp(0.0, 1.0);
        if fade <= 0.0 {
            return;
        }
        for i in 0..3 {
            if self.hl[i] < 0.01 {
                continue;
            }
            let (s0, s1) = SECTORS[i];
            let pts = band_outline(s0, s1, t, 20, i == 0, i == 2);
            if let Some(p) = poly_path(&pts, true) {
                let al = (38.0 * self.hl[i] * fade) as u8;
                fill(&mut self.pixmap, &p, [255, 255, 255, al], base);
            }
        }
        let seam = (40.0 * fade) as u8;
        for s in [-1.0f32 / 3.0, 1.0f32 / 3.0] {
            let (a, b) = edge_points(s, t);
            line(&mut self.pixmap, base, [255, 255, 255, seam], 1.4, a, b);
        }
        let icons = [Icon::Note, Icon::Sliders, Icon::Calendar];
        for i in 0..3 {
            let (x, y, _, _) = centerline(SECTOR_S[i], t);
            let hl = self.hl[i];
            let col = [255u8, 255, 255, ((170.0 + 85.0 * hl) * fade) as u8];
            let k = t.min(1.15) * (1.0 + 0.14 * hl);
            let tf = base.pre_translate(x, y).pre_scale(k, k);
            draw_icon(&mut self.pixmap, icons[i], tf, col);
        }
    }

    fn paint_panel(&mut self, base: Transform) {
        let page = match self.view {
            Some(p) => p,
            None => return,
        };
        if self.pv < 0.01 {
            return;
        }
        let (x, y, w, h) = self.panel_rect();
        let r = (19.0 + 4.0 * self.pv.min(1.0)).min(h / 2.0).min(w / 2.0);
        if let Some(p) = rounded_rect(x, y, w, h, r) {
            fill(&mut self.pixmap, &p, BODY, base);
            stroke(&mut self.pixmap, &p, BORDER, 1.0, base);
        }
        let ca = ((self.pv - 0.7) / 0.25).clamp(0.0, 1.0);
        if ca < 0.02 {
            return;
        }
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let x0 = CX - PANEL_W / 2.0;
        self.txt(page_title(page), x0 + 16.0, MARGIN + 27.0, 14.0, white);
        self.paint_widgets(base, ca);
        match page {
            Page::Media => {
                let dim = [255u8, 255, 255, (120.0 * ca) as u8];
                self.txt_c("Scroll sisi kiri: kecerahan, sisi kanan: volume", CX, MARGIN + 182.0, 11.0, dim);
            }
            Page::System => {}
            Page::Hub => match self.hub_tab {
                0 => self.paint_prayer(ca),
                1 => self.paint_calendar(base, ca),
                _ => self.paint_clips(ca),
            },
        }
    }

    fn paint_widgets(&mut self, base: Transform, ca: f32) {
        let ws = self.widgets();
        let al = |v: f32| (v * ca) as u8;
        for (i, w) in ws.iter().enumerate() {
            let hov = self.hover_w == Some(i);
            let pending = match self.confirm {
                Some((a, t0)) => a == w.act && t0.elapsed().as_secs_f32() < 3.0,
                None => false,
            };
            let bg = if pending {
                [255, 80, 80, al(120.0)]
            } else if w.on {
                [ACCENT[0], ACCENT[1], ACCENT[2], al(80.0)]
            } else if hov && w.enabled {
                [255, 255, 255, al(42.0)]
            } else {
                [255, 255, 255, al(if w.enabled { 18.0 } else { 9.0 })]
            };
            let (x, y, wdt, ht) = w.r;
            if let Some(p) = rounded_rect(x, y, wdt, ht, 10.0) {
                fill(&mut self.pixmap, &p, bg, base);
            }
            let white = [255u8, 255, 255, al(235.0)];
            let has_label = !w.label.is_empty();
            if w.icon != Icon::None {
                let icon_cy = if has_label { y + ht * 0.38 } else { y + ht / 2.0 };
                draw_icon(&mut self.pixmap, w.icon, base.pre_translate(x + wdt / 2.0, icon_cy), white);
            }
            if has_label {
                let label = if pending { "Yakin?".to_string() } else { w.label.clone() };
                let by = if w.icon == Icon::None { y + ht / 2.0 + 4.0 } else { y + ht - 9.0 };
                self.txt_c(&label, x + wdt / 2.0, by, 11.0, white);
            }
        }
    }

    fn paint_prayer(&mut self, ca: f32) {
        let y0 = MARGIN;
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        let accent = [ACCENT[0], ACCENT[1], ACCENT[2], (255.0 * ca) as u8];
        let (status, times) = match self.prayer.lock() {
            Ok(g) => (g.status, g.times.clone()),
            Err(_) => (2u8, Vec::new()),
        };
        if times.len() < 6 {
            let msg = if status == 2 { "Gagal memuat jadwal. Periksa internet." } else { "Memuat jadwal shalat…" };
            self.txt_c(msg, CX, y0 + 140.0, 13.0, dim);
            let city = format!("Kota: {}", self.city);
            self.txt_c(&city, CX, y0 + 164.0, 11.0, dim);
            return;
        }
        let lt = sys::now_local();
        let nowsec = lt.hour * 3600 + lt.min * 60 + lt.sec;
        let days = cal::days_from_civil(lt.year, lt.month, lt.day);
        let ramadan = cal::hijri(days).1 == 9;
        let mut next: Option<(usize, u32)> = None;
        for (i, (_, m)) in times.iter().enumerate() {
            if i == 0 && !ramadan {
                continue;
            }
            let ts = *m * 60;
            if ts > nowsec {
                next = Some((i, ts - nowsec));
                break;
            }
        }
        let (ni, secs) = match next {
            Some(v) => v,
            None => {
                let i = if ramadan { 0 } else { 1 };
                (i, times[i].1 * 60 + 86400 - nowsec)
            }
        };
        let head = format!("Menuju {}", times[ni].0);
        self.txt_c(&head, CX, y0 + 100.0, 13.0, dim);
        let clock = format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60);
        self.txt_c(&clock, CX, y0 + 142.0, 36.0, white);
        for i in 0..6 {
            let col = i % 3;
            let row = i / 3;
            let x = CX - 150.0 + (col as f32) * 104.0;
            let by = y0 + 178.0 + (row as f32) * 24.0;
            let s = format!("{} {:02}:{:02}", times[i].0, times[i].1 / 60, times[i].1 % 60);
            let c = if i == ni { accent } else if i == 0 && !ramadan { dim } else { white };
            self.txt(&s, x, by, 12.0, c);
        }
        let city = format!("{} • metode Kemenag RI", self.city);
        self.txt_c(&city, CX, y0 + 248.0, 11.0, dim);
    }

    fn paint_calendar(&mut self, base: Transform, ca: f32) {
        let y0 = MARGIN;
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        let red = [255u8, 130, 130, (235.0 * ca) as u8];
        let lt = sys::now_local();
        let (yy, mm) = (self.cal_year, self.cal_month);
        let title = format!("{} {}", cal::BULAN[(mm - 1) as usize], yy);
        self.txt_c(&title, CX, y0 + 96.0, 14.0, white);

        let heads = ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"];
        for i in 0..7 {
            let cx = CX - 132.0 + (i as f32) * 44.0;
            let c = if i == 6 { red } else { dim };
            self.txt_c(heads[i], cx, y0 + 120.0, 10.0, c);
        }
        let first = cal::weekday(cal::days_from_civil(yy, mm, 1));
        let first_idx = (first + 6) % 7;
        let dim_m = cal::days_in_month(yy, mm);
        for d in 1..=dim_m {
            let idx = first_idx + (d as usize) - 1;
            let row = idx / 7;
            let col = idx % 7;
            let cx = CX - 132.0 + (col as f32) * 44.0;
            let by = y0 + 142.0 + (row as f32) * 17.0;
            let is_today = yy == lt.year && mm == lt.month && d == lt.day;
            if is_today {
                if let Some(p) = rounded_rect(cx - 12.0, by - 13.0, 24.0, 17.0, 6.0) {
                    fill(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], (200.0 * ca) as u8], base);
                }
            }
            let c = if col == 6 { red } else { white };
            self.txt_c(&d.to_string(), cx, by, 12.0, c);
        }

        let days = cal::days_from_civil(lt.year, lt.month, lt.day);
        let (hy, hm, hd) = cal::hijri(days);
        let info = format!(
            "{} {} • {} {} {} H",
            cal::HARI[cal::weekday(days)],
            cal::PASARAN[cal::pasaran(days)],
            hd,
            cal::HIJRI[((hm - 1).max(0).min(11)) as usize],
            hy
        );
        self.txt_c(&info, CX, y0 + 252.0, 12.0, white);
    }

    fn paint_clips(&mut self, ca: f32) {
        let y0 = MARGIN;
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        if self.clips.is_empty() {
            self.txt_c("Belum ada yang disalin", CX, y0 + 140.0, 13.0, dim);
            return;
        }
        let n = self.clips.len().min(5);
        for i in 0..n {
            let (text, otp, age) = {
                let c = &self.clips[i];
                (c.text.clone(), c.otp, c.at.elapsed().as_secs())
            };
            let shown = if otp {
                let mask: String = text.chars().map(|_| '•').collect();
                format!("Kode OTP {}  (hapus {} dtk)", mask, 60u64.saturating_sub(age))
            } else {
                text
            };
            let fitted = self.fit(&shown, 12.0, 292.0);
            self.txt(&fitted, CX - 150.0, y0 + 82.0 + (i as f32) * 30.0 + 18.0, 12.0, white);
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
        let bottom = self.shape_bottom();
        let (rx, ry, rw, rh) = (CX - 92.0, bottom + 10.0, 184.0f32, 34.0f32);
        let cy = ry + rh / 2.0;
        if let Some(p) = rounded_rect(rx, ry, rw, rh, 17.0) {
            fill(&mut self.pixmap, &p, [8, 8, 10, al(245.0)], base);
            stroke(&mut self.pixmap, &p, [255, 255, 255, al(30.0)], 1.0, base);
        }
        let white = [255u8, 255, 255, al(235.0)];
        let ic = if kind == Kind::Volume { Icon::Speaker } else { Icon::Sun };
        draw_icon(&mut self.pixmap, ic, base.pre_translate(rx + 24.0, cy), white);
        let track_x = rx + 44.0;
        if let Some(p) = rounded_rect(track_x, cy - 3.0, 80.0, 6.0, 3.0) {
            fill(&mut self.pixmap, &p, [255, 255, 255, al(40.0)], base);
        }
        let label = match value {
            Some(v) => {
                let w = (80.0 * (v as f32) / 100.0).max(6.0);
                if let Some(p) = rounded_rect(track_x, cy - 3.0, w, 6.0, 3.0) {
                    fill(&mut self.pixmap, &p, white, base);
                }
                format!("{}%", v)
            }
            None => String::from("N/A"),
        };
        if label == "N/A" {
            self.txt("N/A", rx + 142.0, cy + 4.5, 12.0, white);
        } else {
            draw_digits(&mut self.pixmap, base, &label, rx + 172.0, cy, white);
        }
    }

    fn paint_toast(&mut self, base: Transform) {
        let (msg, until) = match &self.toast {
            Some((m, u)) => (m.clone(), *u),
            None => return,
        };
        let now = Instant::now();
        if now >= until {
            return;
        }
        let left = until.duration_since(now).as_secs_f32();
        let fade = (left / 0.3).min(1.0);
        let al = |v: f32| (v * fade) as u8;
        let mut y = self.shape_bottom() + 10.0;
        if self.hud.is_some() {
            y += 42.0;
        }
        let tw = self.txt_w(&msg, 12.0);
        let w = (tw + 28.0).min(W - 20.0);
        let h = 28.0;
        if let Some(p) = rounded_rect(CX - w / 2.0, y, w, h, 14.0) {
            fill(&mut self.pixmap, &p, [8, 8, 10, al(245.0)], base);
            stroke(&mut self.pixmap, &p, [255, 255, 255, al(30.0)], 1.0, base);
        }
        let fitted = self.fit(&msg, 12.0, w - 20.0);
        self.txt_c(&fitted, CX, y + 19.0, 12.0, [255, 255, 255, al(235.0)]);
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
        WM_LBUTTONDOWN => {
            with_app(|a| a.on_click());
            0
        }
        0x031D => {
            with_app(|a| a.on_clipboard());
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
        AddClipboardFormatListener(hwnd);
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
