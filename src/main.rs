//! DYLAND - Dynamic Island untuk Windows (Rust)
//!
//! v0.3: radial menampilkan semua ikon fitur (dengan tooltip), klik ikon membuka
//! panel dengan deretan tab fitur, pengaturan utama, addons, kalender + tugas.
//! Semua digambar lewat kode. Tanpa WebView2, tanpa file gambar.

#![windows_subsystem = "windows"]

mod cal;
mod cfg;
mod geo;
mod sys;
mod ui;

use cfg::{Cfg, Todo, N_FEAT};
use geo::*;
use std::cell::RefCell;
use std::mem;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use tiny_skia::{Color, Path, PathBuilder, Pixmap, Transform};
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
const LEAVE_DELAY: f32 = 0.18;
const PANEL_LEAVE: f32 = 0.5;
const VOLUME_STEP: f32 = 4.0;
const BRIGHT_STEP: f32 = 5.0;
const HUD_MS: u64 = 1300;
const PANEL_W: f32 = 340.0;
const ACCENT: [u8; 3] = [110, 185, 255];
const X0: f32 = CX - PANEL_W / 2.0;
const Y0: f32 = MARGIN;

const CITIES: &[&str] = &[
    "Jakarta", "Bandung", "Surabaya", "Medan", "Semarang", "Makassar", "Palembang", "Tangerang", "Depok",
    "Bekasi", "Bogor", "Yogyakarta", "Malang", "Denpasar", "Batam", "Pekanbaru", "Padang", "Bandar Lampung",
    "Banjarmasin", "Pontianak", "Samarinda", "Balikpapan", "Manado", "Jayapura", "Ambon", "Kupang", "Mataram",
    "Palu", "Kendari", "Jambi", "Bengkulu", "Banda Aceh", "Serang", "Cirebon", "Surakarta", "Pangkal Pinang",
    "Tanjung Pinang", "Gorontalo", "Ternate", "Sorong", "Palangka Raya", "Tarakan", "Sukabumi", "Tasikmalaya",
    "Magelang", "Kediri", "Jember", "Purwokerto",
];

// ---------------------------------------------------------------------------
// FITUR
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
enum Feat {
    Music,
    Sound,
    Mic,
    Shot,
    Awake,
    Lock,
    Power,
    Calendar,
    Clip,
    Prayer,
    Traffic,
    Addons,
    Settings,
}

const FEATS: [Feat; N_FEAT] = [
    Feat::Music,
    Feat::Sound,
    Feat::Mic,
    Feat::Shot,
    Feat::Awake,
    Feat::Lock,
    Feat::Power,
    Feat::Calendar,
    Feat::Clip,
    Feat::Prayer,
    Feat::Traffic,
    Feat::Addons,
    Feat::Settings,
];

impl Feat {
    fn idx(self) -> usize {
        self as usize
    }

    fn base_icon(self) -> Icon {
        match self {
            Feat::Music => Icon::Note,
            Feat::Sound => Icon::Speaker,
            Feat::Mic => Icon::Mic,
            Feat::Shot => Icon::Camera,
            Feat::Awake => Icon::Coffee,
            Feat::Lock => Icon::Lock,
            Feat::Power => Icon::Power,
            Feat::Calendar => Icon::Calendar,
            Feat::Clip => Icon::Clipboard,
            Feat::Prayer => Icon::Mosque,
            Feat::Traffic => Icon::Chart,
            Feat::Addons => Icon::Puzzle,
            Feat::Settings => Icon::Gear,
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Feat::Music => "Pemutar musik: lagu sebelumnya, jeda, berikutnya",
            Feat::Sound => "Suara: volume, kecerahan, bisukan, output",
            Feat::Mic => "Bisukan / aktifkan mikrofon",
            Feat::Shot => "Screenshot layar penuh",
            Feat::Awake => "Awake: cegah layar dan komputer tidur",
            Feat::Lock => "Kunci komputer",
            Feat::Power => "Tidur, restart, matikan",
            Feat::Calendar => "Kalender, hari libur, dan tugas",
            Feat::Clip => "Riwayat clipboard",
            Feat::Prayer => "Jadwal shalat (addon)",
            Feat::Traffic => "Monitor lalu lintas internet (addon)",
            Feat::Addons => "Addons: tambah atau sembunyikan fitur",
            Feat::Settings => "Pengaturan utama",
        }
    }

    fn page_h(self) -> f32 {
        match self {
            Feat::Music => 160.0,
            Feat::Sound => 232.0,
            Feat::Power => 150.0,
            Feat::Calendar => 350.0,
            Feat::Clip => 240.0,
            Feat::Prayer => 250.0,
            Feat::Traffic => 190.0,
            Feat::Addons => 210.0,
            Feat::Settings => 300.0,
            _ => 160.0,
        }
    }

    fn has_panel(self) -> bool {
        !matches!(self, Feat::Mic | Feat::Shot | Feat::Awake | Feat::Lock)
    }
}

// ---------------------------------------------------------------------------
// TOMBOL DI PANEL
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
enum Act {
    None,
    Close,
    Feature(Feat),
    Prev,
    PlayPause,
    Next,
    SpkMute,
    OpenSound,
    SliderVol,
    SliderBri,
    Sleep,
    Restart,
    Shutdown,
    CalPrev,
    CalNext,
    Day(u32),
    TodoToggle(usize),
    TodoDel(usize),
    TodoAdd,
    ClipCopy(usize),
    ClipClear,
    CityBtn,
    City(usize),
    CityPrev,
    CityNext,
    CityBack,
    AddonToggle(Feat),
    SetFeat(Feat),
    SetJawa,
    SetHijri,
    SetNotify,
    SetStartup,
    Quit,
}

#[derive(Clone, Copy, PartialEq)]
enum WKind {
    Button,
    Slider,
    Toggle,
    Todo,
    Plain,
}

#[derive(Clone)]
struct Widget {
    r: (f32, f32, f32, f32),
    act: Act,
    label: String,
    icon: Icon,
    on: bool,
    enabled: bool,
    tab: bool,
    tip: &'static str,
    kind: WKind,
    val: f32,
}

fn wd(r: (f32, f32, f32, f32), act: Act, label: &str, icon: Icon) -> Widget {
    Widget {
        r,
        act,
        label: label.to_string(),
        icon,
        on: false,
        enabled: true,
        tab: false,
        tip: "",
        kind: WKind::Button,
        val: -1.0,
    }
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

fn fmt_rate(b: f32) -> String {
    if b < 1024.0 {
        format!("{:.0} B/s", b)
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB/s", b / 1024.0)
    } else {
        format!("{:.1} MB/s", b / 1024.0 / 1024.0)
    }
}

/// Bentuk mata Venom (mata kiri; ujung runcing di bagian dalam bawah).
fn eye_path() -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(6.5, 6.0);
    pb.cubic_to(1.0, 8.5, -6.5, 5.5, -7.0, -1.0);
    pb.cubic_to(-7.5, -6.5, -2.0, -9.0, 2.5, -6.5);
    pb.cubic_to(5.0, -4.5, 6.5, 0.5, 6.5, 6.0);
    pb.close();
    pb.finish()
}

// ---------------------------------------------------------------------------
// STATUS APLIKASI
// ---------------------------------------------------------------------------
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
    cfg: Cfg,

    t: f32,
    open: bool,
    anim: Option<Anim>,
    leave_at: Option<Instant>,
    body_pts: Vec<Pt>,
    hl: [f32; 16],
    hover_ring: Option<usize>,
    tip: Option<(String, f32, f32)>,

    view: Option<Feat>,
    panel_open: bool,
    pv: f32,
    ph_cur: f32,
    panel_anim: Option<Anim>,
    panel_leave: Option<Instant>,
    hover_w: Option<usize>,
    confirm: Option<(Act, Instant)>,
    drag: Option<Act>,

    cal_year: i32,
    cal_month: u32,
    cal_sel: (i32, u32, u32),
    todos: Vec<Todo>,
    input: Option<String>,
    caret_phase: bool,
    city_page: Option<usize>,
    startup: bool,

    hud: Option<(Kind, Option<i32>, Instant)>,
    toast: Option<(String, Instant)>,
    bright: sys::Brightness,
    traffic: sys::Traffic,
    shot_result: sys::ShotResult,
    vol_pct: Option<i32>,
    bri_pct: Option<i32>,
    awake: bool,
    mic_muted: Option<bool>,
    spk_muted: Option<bool>,
    caps: bool,
    clips: Vec<ClipItem>,

    prayer: sys::PrayerShared,
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
            cfg: Cfg::load(),
            t: 0.0,
            open: false,
            anim: None,
            leave_at: None,
            body_pts: body_points(0.0),
            hl: [0.0; 16],
            hover_ring: None,
            tip: None,
            view: None,
            panel_open: false,
            pv: 0.0,
            ph_cur: 160.0,
            panel_anim: None,
            panel_leave: None,
            hover_w: None,
            confirm: None,
            drag: None,
            cal_year: lt.year,
            cal_month: lt.month,
            cal_sel: (lt.year, lt.month, lt.day),
            todos: cfg::load_todos(),
            input: None,
            caret_phase: false,
            city_page: None,
            startup: sys::startup_enabled(),
            hud: None,
            toast: None,
            bright: sys::Brightness::new(),
            traffic: sys::Traffic::new(),
            shot_result: sys::new_shot_result(),
            vol_pct: None,
            bri_pct: None,
            awake: false,
            mic_muted: sys::mic_muted(),
            spk_muted: sys::spk_muted(),
            caps: sys::caps_on(),
            clips: Vec::new(),
            prayer: sys::new_prayer(),
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

    // --- fitur di radial ------------------------------------------------------
    fn ring_items(&self) -> Vec<Feat> {
        FEATS.iter().copied().filter(|f| self.cfg.on[f.idx()]).collect()
    }

    fn feat_icon(&self, f: Feat) -> Icon {
        match f {
            Feat::Mic => {
                if self.mic_muted == Some(true) {
                    Icon::MicOff
                } else {
                    Icon::Mic
                }
            }
            Feat::Sound => {
                if self.spk_muted == Some(true) {
                    Icon::SpeakerOff
                } else {
                    Icon::Speaker
                }
            }
            _ => f.base_icon(),
        }
    }

    fn feat_active(&self, f: Feat) -> bool {
        match f {
            Feat::Mic => self.mic_muted == Some(true),
            Feat::Sound => self.spk_muted == Some(true),
            Feat::Awake => self.awake,
            _ => false,
        }
    }

    fn ring_pos(&self, i: usize, n: usize) -> (f32, f32) {
        let (x, y, _, _) = centerline(ring_s(i, n), 1.0);
        (x, y)
    }

    fn ring_hit(&self, mx: f32, my: f32) -> Option<usize> {
        let n = self.ring_items().len();
        for i in 0..n {
            let (x, y) = self.ring_pos(i, n);
            if (mx - x) * (mx - x) + (my - y) * (my - y) <= 12.5 * 12.5 {
                return Some(i);
            }
        }
        None
    }

    // --- bentuk panel ---------------------------------------------------------
    fn panel_rect(&self) -> (f32, f32, f32, f32) {
        let w = CAP_W + (PANEL_W - CAP_W) * self.pv;
        let h = CAP_H + (self.ph_cur - CAP_H) * self.pv;
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

    fn open_page(&mut self, f: Feat) {
        self.cancel_input();
        self.city_page = None;
        self.hover_w = None;
        self.confirm = None;
        let was_open = self.panel_open;
        self.view = Some(f);
        self.panel_open = true;
        self.panel_leave = None;
        if !was_open {
            self.ph_cur = f.page_h();
            self.set_open(false);
            self.panel_anim = Some(Anim {
                from: self.pv,
                to: 1.0,
                start: Instant::now(),
                dur: 0.75,
                elastic: true,
            });
        }
        match f {
            Feat::Sound => {
                self.vol_pct = sys::get_volume();
                self.spk_muted = sys::spk_muted();
                self.bri_pct = self.bright.current();
            }
            Feat::Calendar => {
                let lt = sys::now_local();
                self.cal_year = lt.year;
                self.cal_month = lt.month;
                self.cal_sel = (lt.year, lt.month, lt.day);
            }
            Feat::Settings => {
                self.startup = sys::startup_enabled();
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn close_panel(&mut self) {
        if !self.panel_open {
            return;
        }
        self.cancel_input();
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
                    self.drag = None;
                }
            }
            self.dirty = true;
        }
    }

    fn smooth_height(&mut self) -> bool {
        if let Some(f) = self.view {
            let target = f.page_h();
            let d = target - self.ph_cur;
            if d.abs() > 0.5 {
                self.ph_cur += d * 0.35;
                self.dirty = true;
                return true;
            } else if d != 0.0 {
                self.ph_cur = target;
                self.dirty = true;
            }
        }
        false
    }

    // --- daftar tombol di panel ---------------------------------------------------
    fn todo_rows(&self) -> Vec<usize> {
        let (y, m, d) = self.cal_sel;
        let key = (y as u32) * 10000 + m * 100 + d;
        let mut v = Vec::new();
        for (i, t) in self.todos.iter().enumerate() {
            if t.date == key {
                v.push(i);
                if v.len() >= 3 {
                    break;
                }
            }
        }
        v
    }

    fn widgets(&self) -> Vec<Widget> {
        let mut v: Vec<Widget> = Vec::new();
        let page = match self.view {
            Some(p) => p,
            None => return v,
        };

        // deretan tab fitur di bagian atas panel
        let items = self.ring_items();
        let n = items.len();
        let area_w = 262.0f32;
        let sp = (area_w / (n as f32)).min(24.0);
        let start = CX - 152.0 + (area_w - sp * (n as f32)) / 2.0 + sp / 2.0;
        for (i, f) in items.iter().enumerate() {
            let cx = start + sp * (i as f32);
            let mut w = wd((cx - sp / 2.0, Y0 + 8.0, sp, 26.0), Act::Feature(*f), "", self.feat_icon(*f));
            w.tab = true;
            w.tip = f.tip();
            w.on = self.feat_active(*f) || page == *f;
            v.push(w);
        }
        let mut close = wd((X0 + PANEL_W - 36.0, Y0 + 8.0, 28.0, 26.0), Act::Close, "", Icon::Close);
        close.tab = true;
        close.tip = "Tutup panel";
        v.push(close);

        match page {
            Feat::Music => {
                let bw = 64.0;
                let gap = 12.0;
                let sx = CX - (3.0 * bw + 2.0 * gap) / 2.0;
                let y = Y0 + 52.0;
                v.push(wd((sx, y, bw, 54.0), Act::Prev, "", Icon::Prev));
                v.push(wd((sx + bw + gap, y, bw, 54.0), Act::PlayPause, "", Icon::PlayPause));
                v.push(wd((sx + 2.0 * (bw + gap), y, bw, 54.0), Act::Next, "", Icon::Next));
            }
            Feat::Sound => {
                let muted = self.spk_muted == Some(true);
                let mut b = wd(
                    (X0 + 14.0, Y0 + 50.0, 312.0, 38.0),
                    Act::SpkMute,
                    if muted { "Suara dibisukan (klik untuk aktifkan)" } else { "Bisukan suara" },
                    if muted { Icon::SpeakerOff } else { Icon::Speaker },
                );
                b.on = muted;
                v.push(b);
                let mut s1 = wd((X0 + 14.0, Y0 + 98.0, 312.0, 36.0), Act::SliderVol, "Volume", Icon::None);
                s1.kind = WKind::Slider;
                s1.val = match self.vol_pct {
                    Some(p) => (p as f32) / 100.0,
                    None => -1.0,
                };
                v.push(s1);
                let mut s2 = wd((X0 + 14.0, Y0 + 142.0, 312.0, 36.0), Act::SliderBri, "Kecerahan", Icon::None);
                s2.kind = WKind::Slider;
                s2.val = match self.bri_pct {
                    Some(p) => (p as f32) / 100.0,
                    None => -1.0,
                };
                v.push(s2);
                v.push(wd((X0 + 14.0, Y0 + 188.0, 312.0, 30.0), Act::OpenSound, "Pengaturan output suara", Icon::None));
            }
            Feat::Power => {
                let bw = 100.0;
                let gap = 8.0;
                let sx = CX - (3.0 * bw + 2.0 * gap) / 2.0;
                let y = Y0 + 52.0;
                v.push(wd((sx, y, bw, 66.0), Act::Sleep, "Tidur", Icon::Moon));
                v.push(wd((sx + bw + gap, y, bw, 66.0), Act::Restart, "Restart", Icon::Restart));
                v.push(wd((sx + 2.0 * (bw + gap), y, bw, 66.0), Act::Shutdown, "Matikan", Icon::Power));
            }
            Feat::Calendar => {
                v.push(wd((CX - 158.0, Y0 + 44.0, 30.0, 24.0), Act::CalPrev, "", Icon::Left));
                v.push(wd((CX + 128.0, Y0 + 44.0, 30.0, 24.0), Act::CalNext, "", Icon::Right));
                let first = cal::weekday(cal::days_from_civil(self.cal_year, self.cal_month, 1));
                let first_idx = (first + 6) % 7;
                let dim = cal::days_in_month(self.cal_year, self.cal_month);
                for d in 1..=dim {
                    let idx = first_idx + (d as usize) - 1;
                    let row = idx / 7;
                    let col = idx % 7;
                    let cx = CX - 132.0 + (col as f32) * 44.0;
                    let by = Y0 + 96.0 + (row as f32) * 16.0;
                    let mut w = wd((cx - 20.0, by - 12.0, 40.0, 16.0), Act::Day(d), "", Icon::None);
                    w.kind = WKind::Plain;
                    v.push(w);
                }
                let rows = self.todo_rows();
                let mut y = Y0 + 252.0;
                for &i in rows.iter() {
                    let mut w = wd((X0 + 14.0, y, 276.0, 22.0), Act::TodoToggle(i), &self.todos[i].text, Icon::None);
                    w.kind = WKind::Todo;
                    w.on = self.todos[i].done;
                    v.push(w);
                    let mut x = wd((X0 + 294.0, y, 32.0, 22.0), Act::TodoDel(i), "", Icon::Close);
                    x.tab = true;
                    x.tip = "Hapus tugas";
                    v.push(x);
                    y += 23.0;
                }
                if self.input.is_none() {
                    v.push(wd((X0 + 14.0, y, 312.0, 22.0), Act::TodoAdd, "+ Tambah tugas", Icon::None));
                }
            }
            Feat::Clip => {
                let n = self.clips.len().min(5);
                for i in 0..n {
                    let mut w = wd((X0 + 14.0, Y0 + 46.0 + (i as f32) * 31.0, 312.0, 27.0), Act::ClipCopy(i), "", Icon::None);
                    w.tip = "";
                    v.push(w);
                }
                v.push(wd((X0 + 14.0, Y0 + 204.0, 312.0, 26.0), Act::ClipClear, "Hapus riwayat", Icon::None));
            }
            Feat::Prayer => {
                if let Some(p) = self.city_page {
                    let from = p * 10;
                    for k in 0..10 {
                        let idx = from + k;
                        if idx >= CITIES.len() {
                            break;
                        }
                        let c = (k % 2) as f32;
                        let r = (k / 2) as f32;
                        let mut w = wd(
                            (CX - 158.0 + c * 162.0, Y0 + 46.0 + r * 30.0, 154.0, 26.0),
                            Act::City(idx),
                            CITIES[idx],
                            Icon::None,
                        );
                        w.on = CITIES[idx] == self.cfg.city.as_str();
                        v.push(w);
                    }
                    v.push(wd((CX - 158.0, Y0 + 206.0, 90.0, 26.0), Act::CityBack, "Kembali", Icon::None));
                    v.push(wd((CX + 60.0, Y0 + 206.0, 30.0, 26.0), Act::CityPrev, "", Icon::Left));
                    v.push(wd((CX + 128.0, Y0 + 206.0, 30.0, 26.0), Act::CityNext, "", Icon::Right));
                } else {
                    let label = format!("Kota: {}", self.cfg.city);
                    v.push(wd((CX + 8.0, Y0 + 44.0, 150.0, 24.0), Act::CityBtn, &label, Icon::None));
                }
            }
            Feat::Traffic => {}
            Feat::Addons => {
                let rows: [(Feat, &str); 3] = [
                    (Feat::Prayer, "Jadwal shalat\nJadwal Kemenag RI, pengingat, pilih kota"),
                    (Feat::Traffic, "Monitor lalu lintas\nKecepatan unduh dan unggah internet"),
                    (Feat::Settings, "Teleprompter\nSegera hadir"),
                ];
                for (i, (f, label)) in rows.iter().enumerate() {
                    let mut w = wd((X0 + 14.0, Y0 + 48.0 + (i as f32) * 50.0, 312.0, 44.0), Act::AddonToggle(*f), label, Icon::None);
                    w.kind = WKind::Toggle;
                    if i < 2 {
                        w.on = self.cfg.on[f.idx()];
                    } else {
                        w.enabled = false;
                    }
                    v.push(w);
                }
            }
            Feat::Settings => {
                let feats: [(Feat, &str); 9] = [
                    (Feat::Music, "Musik"),
                    (Feat::Sound, "Suara"),
                    (Feat::Mic, "Mikrofon"),
                    (Feat::Shot, "Screenshot"),
                    (Feat::Awake, "Awake"),
                    (Feat::Lock, "Kunci"),
                    (Feat::Power, "Daya"),
                    (Feat::Calendar, "Kalender"),
                    (Feat::Clip, "Clipboard"),
                ];
                for (i, (f, name)) in feats.iter().enumerate() {
                    let c = (i % 3) as f32;
                    let r = (i / 3) as f32;
                    let mut w = wd(
                        (CX - 158.0 + c * 108.0, Y0 + 62.0 + r * 32.0, 100.0, 26.0),
                        Act::SetFeat(*f),
                        name,
                        Icon::None,
                    );
                    w.on = self.cfg.on[f.idx()];
                    v.push(w);
                }
                let opts: [(Act, &str, bool); 4] = [
                    (Act::SetJawa, "Tanggal Jawa", self.cfg.show_jawa),
                    (Act::SetHijri, "Tanggal Hijriah", self.cfg.show_hijri),
                    (Act::SetNotify, "Notifikasi shalat", self.cfg.notify),
                    (Act::SetStartup, "Mulai bersama Windows", self.startup),
                ];
                for (i, (act, name, on)) in opts.iter().enumerate() {
                    let c = (i % 2) as f32;
                    let r = (i / 2) as f32;
                    let mut w = wd(
                        (CX - 158.0 + c * 162.0, Y0 + 182.0 + r * 32.0, 154.0, 26.0),
                        *act,
                        name,
                        Icon::None,
                    );
                    w.on = *on;
                    v.push(w);
                }
                v.push(wd((CX - 158.0, Y0 + 254.0, 316.0, 30.0), Act::Quit, "Keluar dari DYLAND", Icon::None));
            }
            _ => {}
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
                    let w = self.widgets()[i].clone();
                    if w.kind == WKind::Slider {
                        self.drag = Some(w.act);
                        self.apply_slider(w.act, w.r, mx);
                        self.dirty = true;
                    } else {
                        self.perform(w.act);
                    }
                }
            }
            return;
        }
        if self.open && self.t > 0.85 {
            if let Some(i) = self.ring_hit(mx, my) {
                let items = self.ring_items();
                if i < items.len() {
                    self.activate(items[i]);
                }
            }
        }
    }

    fn apply_slider(&mut self, act: Act, r: (f32, f32, f32, f32), mx: f32) {
        let frac = ((mx - r.0) / r.2).clamp(0.0, 1.0);
        match act {
            Act::SliderVol => {
                self.vol_pct = sys::set_volume(frac * 100.0);
                if self.vol_pct.is_some() {
                    self.spk_muted = Some(false);
                }
            }
            Act::SliderBri => {
                self.bri_pct = self.bright.set_level(frac * 100.0);
            }
            _ => {}
        }
    }

    fn update_drag(&mut self, mx: f32) {
        let act = match self.drag {
            Some(a) => a,
            None => return,
        };
        if sys::left_button_down() {
            let ws = self.widgets();
            for w in ws.iter() {
                if w.act == act {
                    self.apply_slider(act, w.r, mx);
                    break;
                }
            }
            self.dirty = true;
        } else {
            self.drag = None;
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

    fn activate(&mut self, f: Feat) {
        match f {
            Feat::Mic => {
                self.mic_muted = sys::mic_toggle();
                match self.mic_muted {
                    Some(true) => self.say("Mikrofon dibisukan", 2.0),
                    Some(false) => self.say("Mikrofon aktif", 2.0),
                    None => self.say("Mikrofon tidak ditemukan (lihat log.txt)", 3.0),
                }
            }
            Feat::Shot => self.do_screenshot(),
            Feat::Awake => {
                self.awake = !self.awake;
                sys::set_awake(self.awake);
                if self.awake {
                    self.say("Awake aktif: layar tidak akan tidur", 3.0);
                } else {
                    self.say("Awake dimatikan", 2.0);
                }
            }
            Feat::Lock => sys::lock_pc(),
            _ => {
                if f.has_panel() {
                    self.open_page(f);
                }
            }
        }
        self.dirty = true;
    }

    fn do_screenshot(&mut self) {
        match sys::capture_screen(self.hwnd) {
            Some(shot) => {
                sys::save_shot_async(shot, self.shot_result.clone());
                self.say("Menyimpan screenshot…", 1.5);
            }
            None => self.say("Screenshot gagal (lihat log.txt)", 3.5),
        }
        self.dirty = true;
        self.refresh();
    }

    fn poll_shot(&mut self) {
        let res = match self.shot_result.lock() {
            Ok(mut g) => g.take(),
            Err(_) => None,
        };
        if let Some(r) = res {
            match r {
                Ok(_) => self.say("Screenshot tersimpan di Gambar\\Dyland", 3.5),
                Err(e) => {
                    let msg = format!("Screenshot gagal: {}", e);
                    self.say(&msg, 4.0);
                }
            }
        }
    }

    fn perform(&mut self, act: Act) {
        match act {
            Act::None => {}
            Act::Close => self.close_panel(),
            Act::Feature(f) => self.activate(f),
            Act::Prev => sys::media_key(0xB1),
            Act::PlayPause => sys::media_key(0xB3),
            Act::Next => sys::media_key(0xB0),
            Act::SpkMute => {
                self.spk_muted = sys::spk_toggle_mute();
                if self.spk_muted.is_none() {
                    self.say("Perangkat suara tidak ditemukan (lihat log.txt)", 3.0);
                }
            }
            Act::OpenSound => sys::open_sound_settings(),
            Act::SliderVol | Act::SliderBri => {}
            Act::Sleep => self.confirm_or(Act::Sleep, sys::sleep_pc),
            Act::Restart => self.confirm_or(Act::Restart, sys::restart_pc),
            Act::Shutdown => self.confirm_or(Act::Shutdown, sys::shutdown_pc),
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
            Act::Day(d) => {
                self.cal_sel = (self.cal_year, self.cal_month, d);
                self.cancel_input();
            }
            Act::TodoToggle(i) => {
                if let Some(t) = self.todos.get_mut(i) {
                    t.done = !t.done;
                }
                cfg::save_todos(&self.todos);
            }
            Act::TodoDel(i) => {
                if i < self.todos.len() {
                    self.todos.remove(i);
                    cfg::save_todos(&self.todos);
                }
            }
            Act::TodoAdd => self.begin_input(),
            Act::ClipCopy(i) => {
                let t = self.clips.get(i).map(|c| c.text.clone());
                if let Some(t) = t {
                    sys::clipboard_set(&t);
                    self.say("Disalin", 1.5);
                }
            }
            Act::ClipClear => {
                self.clips.clear();
            }
            Act::CityBtn => {
                let p = CITIES.iter().position(|c| *c == self.cfg.city.as_str()).unwrap_or(0) / 10;
                self.city_page = Some(p);
            }
            Act::City(idx) => {
                if let Some(c) = CITIES.get(idx) {
                    self.cfg.city = c.to_string();
                    self.cfg.country = String::from("Indonesia");
                    self.cfg.save();
                    sys::reset_prayer(&self.prayer);
                    self.prayer_try = None;
                    self.city_page = None;
                    let msg = format!("Kota: {}", c);
                    self.say(&msg, 2.0);
                }
            }
            Act::CityPrev => {
                if let Some(p) = self.city_page {
                    let max = (CITIES.len() - 1) / 10;
                    self.city_page = Some(if p == 0 { max } else { p - 1 });
                }
            }
            Act::CityNext => {
                if let Some(p) = self.city_page {
                    let max = (CITIES.len() - 1) / 10;
                    self.city_page = Some(if p >= max { 0 } else { p + 1 });
                }
            }
            Act::CityBack => {
                self.city_page = None;
            }
            Act::AddonToggle(f) => {
                self.cfg.on[f.idx()] = !self.cfg.on[f.idx()];
                self.cfg.save();
                if f == Feat::Prayer {
                    self.prayer_try = None;
                }
            }
            Act::SetFeat(f) => {
                self.cfg.on[f.idx()] = !self.cfg.on[f.idx()];
                self.cfg.save();
            }
            Act::SetJawa => {
                self.cfg.show_jawa = !self.cfg.show_jawa;
                self.cfg.save();
            }
            Act::SetHijri => {
                self.cfg.show_hijri = !self.cfg.show_hijri;
                self.cfg.save();
            }
            Act::SetNotify => {
                self.cfg.notify = !self.cfg.notify;
                self.cfg.save();
            }
            Act::SetStartup => {
                let want = !self.startup;
                if sys::set_startup(want) {
                    self.startup = want;
                } else {
                    self.say("Gagal mengubah pengaturan startup", 3.0);
                }
            }
            Act::Quit => unsafe {
                PostQuitMessage(0);
            },
        }
        self.dirty = true;
    }

    // --- mengetik (tugas) ---------------------------------------------------------------
    fn begin_input(&mut self) {
        self.input = Some(String::new());
        sys::set_noactivate(self.hwnd, false);
        sys::take_focus(self.hwnd);
        self.dirty = true;
    }

    fn cancel_input(&mut self) {
        if self.input.is_some() {
            self.input = None;
            sys::set_noactivate(self.hwnd, true);
            self.dirty = true;
        }
    }

    fn commit_input(&mut self) {
        let text = self.input.take().unwrap_or_default();
        let t = text.trim().to_string();
        if !t.is_empty() {
            let (y, m, d) = self.cal_sel;
            let key = (y as u32) * 10000 + m * 100 + d;
            self.todos.push(Todo { date: key, done: false, text: t });
            cfg::save_todos(&self.todos);
        }
        sys::set_noactivate(self.hwnd, true);
        self.dirty = true;
    }

    fn on_char(&mut self, c: u32) {
        let mut buf = match self.input.take() {
            Some(b) => b,
            None => return,
        };
        match c {
            8 => {
                buf.pop();
                self.input = Some(buf);
            }
            13 => {
                self.input = Some(buf);
                self.commit_input();
            }
            27 => {
                self.input = Some(buf);
                self.cancel_input();
            }
            22 => {
                if let Some(t) = sys::clipboard_get(self.hwnd) {
                    let one = t.lines().next().unwrap_or("").trim().to_string();
                    for ch in one.chars() {
                        if buf.chars().count() < 80 {
                            buf.push(ch);
                        }
                    }
                }
                self.input = Some(buf);
            }
            _ => {
                if c >= 32 {
                    if let Some(ch) = char::from_u32(c) {
                        if buf.chars().count() < 80 {
                            buf.push(ch);
                        }
                    }
                }
                self.input = Some(buf);
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

    /// Scroll di pulau: sisi kanan = volume, sisi kiri = kecerahan.
    fn on_wheel(&mut self, delta: i16) {
        if self.view.is_some() {
            return;
        }
        let (mx, _) = self.cursor_local();
        let steps = delta as f32 / 120.0;
        let (kind, val) = if mx >= CX {
            let v = sys::change_volume(steps * VOLUME_STEP);
            if v.is_some() {
                self.vol_pct = v;
            }
            (Kind::Volume, v)
        } else {
            let b = self.bright.change(steps * BRIGHT_STEP);
            if b.is_some() {
                self.bri_pct = b;
            }
            (Kind::Brightness, b)
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
        if self.input.is_some() {
            let ph = (self.epoch.elapsed().as_millis() / 500) % 2 == 0;
            if ph != self.caret_phase {
                self.caret_phase = ph;
                self.dirty = true;
            }
        }
    }

    fn per_second(&mut self, now: Instant) {
        let lt = sys::now_local();
        if lt.sec == self.last_sec {
            return;
        }
        self.last_sec = lt.sec;

        if self.cfg.on[Feat::Traffic.idx()] {
            self.traffic.sample();
            if self.view == Some(Feat::Traffic) {
                self.dirty = true;
            }
        }

        if !self.cfg.on[Feat::Prayer.idx()] {
            return;
        }
        if self.view == Some(Feat::Prayer) {
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
            sys::fetch_prayer(
                self.prayer.clone(),
                self.cfg.city.clone(),
                self.cfg.country.clone(),
                daykey,
            );
        }

        if !self.cfg.notify {
            return;
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
        self.poll_shot();
        self.update_drag(mx);
        let height_moving = self.smooth_height();

        let mut hover_ring: Option<usize> = None;
        if self.view.is_some() {
            let (px, py, pw, ph) = self.panel_rect();
            let inside = mx >= px - 14.0 && mx <= px + pw + 14.0 && my >= py - 14.0 && my <= py + ph + 14.0;
            if self.panel_open && self.input.is_none() && self.drag.is_none() {
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
            if self.open && self.t > 0.85 {
                hover_ring = self.ring_hit(mx, my);
            }
        }
        self.hover_ring = hover_ring;

        let mut moving = false;
        for i in 0..16 {
            let target = if hover_ring == Some(i) { 1.0 } else { 0.0 };
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

        // tooltip
        let new_tip: Option<(String, f32, f32)> = if self.view.is_some() {
            match self.hover_w {
                Some(i) => {
                    let ws = self.widgets();
                    match ws.get(i) {
                        Some(w) if !w.tip.is_empty() => Some((w.tip.to_string(), w.r.0 + w.r.2 / 2.0, Y0 + 46.0)),
                        _ => None,
                    }
                }
                None => None,
            }
        } else if let Some(i) = hover_ring {
            let items = self.ring_items();
            let n = items.len();
            if i < n {
                let (x, _) = self.ring_pos(i, n);
                Some((items[i].tip().to_string(), x, ARCH_CY + ARCH_R + ARCH_H / 2.0 + 10.0))
            } else {
                None
            }
        } else {
            None
        };
        if new_tip != self.tip {
            self.tip = new_tip;
            self.dirty = true;
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
            || moving
            || height_moving
            || self.drag.is_some();
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
                    self.paint_ring(base, t);
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
        self.paint_tip(base);
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
        let eye = match eye_path() {
            Some(p) => p,
            None => return,
        };

        for sx in [-1.0f32, 1.0f32] {
            let ex = CX + sx * 14.0;
            let ey = CAP_CY;
            if happy {
                let pts = arc_pts(ex, ey + 3.0, 5.5, 205.0, 335.0, 10);
                if let Some(p) = poly_path(&pts, false) {
                    stroke(&mut self.pixmap, &p, [col[0], col[1], col[2], a], 2.6, base);
                }
            } else {
                let mx = -sx;
                let ox = dxp * 0.8;
                let oy = dyp * 0.8;
                let glows: [(f32, f32); 2] = [(1.55, 22.0), (1.25, 42.0)];
                for (k, ga) in glows.iter() {
                    let tf = base.pre_translate(ex + ox, ey + oy).pre_scale(mx * *k, *k * blink);
                    fill(&mut self.pixmap, &eye, [col[0], col[1], col[2], (*ga * fa) as u8], tf);
                }
                let tf = base.pre_translate(ex + ox, ey + oy).pre_scale(mx, blink);
                fill(&mut self.pixmap, &eye, [col[0], col[1], col[2], a], tf);
            }
        }
        if mode == Face::Full {
            let pts = arc_pts(CX, CAP_CY + 4.0, 6.0, 25.0, 155.0, 10);
            if let Some(p) = poly_path(&pts, false) {
                stroke(&mut self.pixmap, &p, [col[0], col[1], col[2], a], 2.2, base);
            }
        }
        if mode == Face::Hot {
            let dx = CX + 33.0;
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

    fn paint_ring(&mut self, base: Transform, t: f32) {
        let fade = ((t - 0.5) / 0.4).clamp(0.0, 1.0);
        if fade <= 0.0 {
            return;
        }
        let items = self.ring_items();
        let n = items.len();
        for (i, f) in items.iter().enumerate() {
            let (x, y, _, _) = centerline(ring_s(i, n), t);
            let hl = self.hl[i.min(15)];
            let active = self.feat_active(*f);
            if active {
                if let Some(p) = circle_path(x, y, 13.5) {
                    fill(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], (70.0 * fade) as u8], base);
                }
            }
            if hl > 0.01 {
                if let Some(p) = circle_path(x, y, 13.5) {
                    fill(&mut self.pixmap, &p, [255, 255, 255, (46.0 * hl * fade) as u8], base);
                }
            }
            let col = [255u8, 255, 255, ((190.0 + 65.0 * hl) * fade) as u8];
            let k = t.min(1.15) * 0.82 * (1.0 + 0.16 * hl);
            let tf = base.pre_translate(x, y).pre_scale(k, k);
            let ic = self.feat_icon(*f);
            draw_icon(&mut self.pixmap, ic, tf, col);
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
        self.paint_widgets(base, ca);
        match page {
            Feat::Music => self.paint_music(ca),
            Feat::Power => {
                let dim = [255u8, 255, 255, (120.0 * ca) as u8];
                self.txt_c("Klik dua kali untuk mengonfirmasi", CX, Y0 + 138.0, 11.0, dim);
            }
            Feat::Calendar => self.paint_calendar(base, ca),
            Feat::Clip => self.paint_clips(ca),
            Feat::Prayer => self.paint_prayer(ca),
            Feat::Traffic => self.paint_traffic(base, ca),
            Feat::Settings => {
                let dim = [255u8, 255, 255, (130.0 * ca) as u8];
                self.txt("Fitur di radial", X0 + 16.0, Y0 + 54.0, 11.0, dim);
                self.txt("Tampilan dan sistem", X0 + 16.0, Y0 + 176.0, 11.0, dim);
            }
            _ => {}
        }
    }

    fn paint_widgets(&mut self, base: Transform, ca: f32) {
        let ws = self.widgets();
        let al = |v: f32| (v * ca) as u8;
        for (i, w) in ws.iter().enumerate() {
            let hov = self.hover_w == Some(i);
            let (x, y, wdt, ht) = w.r;
            match w.kind {
                WKind::Plain => {}
                WKind::Slider => self.paint_slider(base, w, ca),
                WKind::Toggle => self.paint_toggle(base, w, ca, hov),
                WKind::Todo => self.paint_todo(base, w, ca, hov),
                WKind::Button => {
                    let pending = match self.confirm {
                        Some((a, t0)) => a == w.act && t0.elapsed().as_secs_f32() < 3.0,
                        None => false,
                    };
                    let danger = w.act == Act::Quit;
                    let bg = if pending {
                        [255, 80, 80, al(130.0)]
                    } else if w.on {
                        [ACCENT[0], ACCENT[1], ACCENT[2], al(if w.tab { 60.0 } else { 85.0 })]
                    } else if hov && w.enabled {
                        [255, 255, 255, al(if w.tab { 30.0 } else { 44.0 })]
                    } else if w.tab {
                        [255, 255, 255, 0]
                    } else if danger {
                        [255, 80, 80, al(40.0)]
                    } else {
                        [255, 255, 255, al(if w.enabled { 18.0 } else { 9.0 })]
                    };
                    let rad = if w.tab { 8.0 } else { 10.0 };
                    if bg[3] > 0 {
                        if let Some(p) = rounded_rect(x, y, wdt, ht, rad) {
                            fill(&mut self.pixmap, &p, bg, base);
                        }
                    }
                    let white = [255u8, 255, 255, al(235.0)];
                    let has_label = !w.label.is_empty();
                    if w.icon != Icon::None {
                        let icon_cy = if has_label { y + ht * 0.38 } else { y + ht / 2.0 };
                        let k = if w.tab { 0.8 } else { 1.0 };
                        let tf = base.pre_translate(x + wdt / 2.0, icon_cy).pre_scale(k, k);
                        draw_icon(&mut self.pixmap, w.icon, tf, white);
                    }
                    if has_label {
                        let label = if pending { "Yakin?".to_string() } else { w.label.clone() };
                        let fitted = self.fit(&label, 11.0, wdt - 8.0);
                        let by = if w.icon == Icon::None { y + ht / 2.0 + 4.0 } else { y + ht - 9.0 };
                        self.txt_c(&fitted, x + wdt / 2.0, by, 11.0, white);
                    }
                }
            }
        }
    }

    fn paint_slider(&mut self, base: Transform, w: &Widget, ca: f32) {
        let (x, y, wdt, _) = w.r;
        let al = |v: f32| (v * ca) as u8;
        let white = [255u8, 255, 255, al(235.0)];
        let dim = [255u8, 255, 255, al(140.0)];
        self.txt(&w.label, x, y + 12.0, 11.0, dim);
        let vtxt = if w.val < 0.0 {
            "N/A".to_string()
        } else {
            format!("{}%", (w.val * 100.0).round() as i32)
        };
        let tw = self.txt_w(&vtxt, 11.0);
        self.txt(&vtxt, x + wdt - tw, y + 12.0, 11.0, white);
        let ty = y + 22.0;
        if let Some(p) = rounded_rect(x, ty, wdt, 6.0, 3.0) {
            fill(&mut self.pixmap, &p, [255, 255, 255, al(36.0)], base);
        }
        if w.val >= 0.0 {
            let fw = (wdt * w.val).max(6.0);
            if let Some(p) = rounded_rect(x, ty, fw, 6.0, 3.0) {
                fill(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], al(230.0)], base);
            }
            if let Some(p) = circle_path(x + fw, ty + 3.0, 7.0) {
                fill(&mut self.pixmap, &p, white, base);
            }
        }
    }

    fn paint_toggle(&mut self, base: Transform, w: &Widget, ca: f32, hov: bool) {
        let (x, y, wdt, ht) = w.r;
        let al = |v: f32| (v * ca) as u8;
        let (title, desc) = match w.label.split_once('\n') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (w.label.clone(), String::new()),
        };
        let bg = if hov && w.enabled { 36.0 } else { 16.0 };
        if let Some(p) = rounded_rect(x, y, wdt, ht, 10.0) {
            fill(&mut self.pixmap, &p, [255, 255, 255, al(bg)], base);
        }
        let tc = if w.enabled { [255u8, 255, 255, al(235.0)] } else { [255u8, 255, 255, al(110.0)] };
        let dc = [255u8, 255, 255, al(125.0)];
        self.txt(&title, x + 12.0, y + 19.0, 13.0, tc);
        self.txt(&desc, x + 12.0, y + 35.0, 10.0, dc);
        let sx = x + wdt - 52.0;
        let sy = y + ht / 2.0 - 10.0;
        let track = if w.on {
            [ACCENT[0], ACCENT[1], ACCENT[2], al(220.0)]
        } else {
            [255, 255, 255, al(50.0)]
        };
        if let Some(p) = rounded_rect(sx, sy, 38.0, 20.0, 10.0) {
            fill(&mut self.pixmap, &p, track, base);
        }
        let kx = if w.on { sx + 28.0 } else { sx + 10.0 };
        if let Some(p) = circle_path(kx, sy + 10.0, 7.5) {
            fill(&mut self.pixmap, &p, [255, 255, 255, al(if w.enabled { 245.0 } else { 120.0 })], base);
        }
    }

    fn paint_todo(&mut self, base: Transform, w: &Widget, ca: f32, hov: bool) {
        let (x, y, wdt, ht) = w.r;
        let al = |v: f32| (v * ca) as u8;
        if let Some(p) = rounded_rect(x, y, wdt, ht, 8.0) {
            fill(&mut self.pixmap, &p, [255, 255, 255, al(if hov { 34.0 } else { 16.0 })], base);
        }
        if let Some(p) = rounded_rect(x + 6.0, y + 4.0, 14.0, 14.0, 4.0) {
            if w.on {
                fill(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], al(220.0)], base);
                let tf = base.pre_translate(x + 13.0, y + 11.0).pre_scale(0.62, 0.62);
                draw_icon(&mut self.pixmap, Icon::Check, tf, [8, 8, 10, al(255.0)]);
            } else {
                stroke(&mut self.pixmap, &p, [255, 255, 255, al(200.0)], 1.5, base);
            }
        }
        let fitted = self.fit(&w.label, 12.0, wdt - 36.0);
        let c = if w.on { [255u8, 255, 255, al(110.0)] } else { [255u8, 255, 255, al(235.0)] };
        self.txt(&fitted, x + 28.0, y + 16.0, 12.0, c);
    }

    fn paint_music(&mut self, ca: f32) {
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        self.txt_c("Mengendalikan pemutar yang sedang aktif", CX, Y0 + 130.0, 11.0, dim);
        self.txt_c("(Spotify, YouTube, dan pemutar lainnya)", CX, Y0 + 145.0, 10.0, dim);
    }

    fn paint_prayer(&mut self, ca: f32) {
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        let accent = [ACCENT[0], ACCENT[1], ACCENT[2], (255.0 * ca) as u8];

        if let Some(p) = self.city_page {
            let pages = (CITIES.len() - 1) / 10 + 1;
            let s = format!("{}/{}", p + 1, pages);
            self.txt_c(&s, CX + 109.0, Y0 + 224.0, 11.0, dim);
            return;
        }
        self.txt("Jadwal shalat", X0 + 16.0, Y0 + 61.0, 12.0, dim);

        let (status, times) = match self.prayer.lock() {
            Ok(g) => (g.status, g.times.clone()),
            Err(_) => (2u8, Vec::new()),
        };
        if times.len() < 6 {
            let msg = if status == 2 {
                "Gagal memuat jadwal. Periksa internet."
            } else {
                "Memuat jadwal shalat…"
            };
            self.txt_c(msg, CX, Y0 + 140.0, 13.0, dim);
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
        self.txt_c(&head, CX, Y0 + 96.0, 13.0, dim);
        let clock = format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60);
        self.txt_c(&clock, CX, Y0 + 138.0, 36.0, white);
        for i in 0..6 {
            let col = i % 3;
            let row = i / 3;
            let x = CX - 150.0 + (col as f32) * 104.0;
            let by = Y0 + 178.0 + (row as f32) * 24.0;
            let s = format!("{} {:02}:{:02}", times[i].0, times[i].1 / 60, times[i].1 % 60);
            let c = if i == ni {
                accent
            } else if i == 0 && !ramadan {
                dim
            } else {
                white
            };
            self.txt(&s, x, by, 12.0, c);
        }
        self.txt_c("Metode Kemenag RI (Aladhan)", CX, Y0 + 236.0, 10.0, dim);
    }

    fn paint_calendar(&mut self, base: Transform, ca: f32) {
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        let red = [255u8, 130, 130, (240.0 * ca) as u8];
        let lt = sys::now_local();
        let (yy, mm) = (self.cal_year, self.cal_month);
        let title = format!("{} {}", cal::BULAN[(mm - 1) as usize], yy);
        self.txt_c(&title, CX, Y0 + 62.0, 14.0, white);

        let heads = ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"];
        for i in 0..7 {
            let cx = CX - 132.0 + (i as f32) * 44.0;
            let c = if i == 6 { red } else { dim };
            self.txt_c(heads[i], cx, Y0 + 84.0, 10.0, c);
        }
        let first = cal::weekday(cal::days_from_civil(yy, mm, 1));
        let first_idx = (first + 6) % 7;
        let dim_m = cal::days_in_month(yy, mm);
        for d in 1..=dim_m {
            let idx = first_idx + (d as usize) - 1;
            let row = idx / 7;
            let col = idx % 7;
            let cx = CX - 132.0 + (col as f32) * 44.0;
            let by = Y0 + 96.0 + (row as f32) * 16.0;
            let is_sel = self.cal_sel == (yy, mm, d);
            let is_today = yy == lt.year && mm == lt.month && d == lt.day;
            if is_sel {
                if let Some(p) = rounded_rect(cx - 12.0, by - 12.0, 24.0, 16.0, 6.0) {
                    fill(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], (210.0 * ca) as u8], base);
                }
            } else if is_today {
                if let Some(p) = rounded_rect(cx - 12.0, by - 12.0, 24.0, 16.0, 6.0) {
                    stroke(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], (200.0 * ca) as u8], 1.4, base);
                }
            }
            let holiday = cal::holiday(yy, mm, d).is_some();
            let c = if is_sel {
                [8u8, 8, 10, (255.0 * ca) as u8]
            } else if col == 6 || holiday {
                red
            } else {
                white
            };
            self.txt_c(&d.to_string(), cx, by, 11.0, c);
            let key = (yy as u32) * 10000 + mm * 100 + d;
            if self.todos.iter().any(|t| t.date == key && !t.done) {
                if let Some(p) = circle_path(cx, by + 3.0, 1.3) {
                    fill(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], (255.0 * ca) as u8], base);
                }
            }
        }

        let (sy, sm, sd) = self.cal_sel;
        let days = cal::days_from_civil(sy, sm, sd);
        let line1 = format!(
            "{}, {} {} {}",
            cal::HARI[cal::weekday(days)],
            sd,
            cal::BULAN[(sm - 1) as usize],
            sy
        );
        self.txt_c(&line1, CX, Y0 + 204.0, 12.0, white);
        let mut parts: Vec<String> = Vec::new();
        if self.cfg.show_jawa {
            parts.push(cal::PASARAN[cal::pasaran(days)].to_string());
        }
        if self.cfg.show_hijri {
            let (hy, hm, hd) = cal::hijri(days);
            let name = cal::HIJRI[((hm - 1).max(0).min(11)) as usize];
            parts.push(format!("{} {} {} H", hd, name, hy));
        }
        if !parts.is_empty() {
            let line2 = parts.join(" • ");
            self.txt_c(&line2, CX, Y0 + 220.0, 11.0, dim);
        }
        if let Some(h) = cal::holiday(sy, sm, sd) {
            self.txt_c(h, CX, Y0 + 235.0, 11.0, red);
        }

        self.txt("Tugas", X0 + 16.0, Y0 + 247.0, 11.0, dim);
        if let Some(buf) = self.input.clone() {
            let rows = self.todo_rows();
            let y = Y0 + 252.0 + 23.0 * (rows.len() as f32);
            if let Some(p) = rounded_rect(X0 + 14.0, y, 312.0, 22.0, 8.0) {
                fill(&mut self.pixmap, &p, [255, 255, 255, (22.0 * ca) as u8], base);
                stroke(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], (230.0 * ca) as u8], 1.4, base);
            }
            let shown = if buf.is_empty() {
                String::from("Ketik tugas, Enter untuk simpan, Esc batal")
            } else {
                buf.clone()
            };
            let fitted = self.fit(&shown, 12.0, 290.0);
            let c = if buf.is_empty() { dim } else { white };
            self.txt(&fitted, X0 + 22.0, y + 16.0, 12.0, c);
            if self.caret_phase && !buf.is_empty() {
                let tw = self.txt_w(&fitted, 12.0);
                let cx = X0 + 22.0 + tw + 1.0;
                line(&mut self.pixmap, base, white, 1.4, (cx, y + 4.0), (cx, y + 18.0));
            }
        }
    }

    fn paint_clips(&mut self, ca: f32) {
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        if self.clips.is_empty() {
            self.txt_c("Belum ada yang disalin", CX, Y0 + 120.0, 13.0, dim);
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
            self.txt(&fitted, X0 + 24.0, Y0 + 46.0 + (i as f32) * 31.0 + 18.0, 12.0, white);
        }
    }

    fn paint_traffic(&mut self, base: Transform, ca: f32) {
        let white = [255u8, 255, 255, (235.0 * ca) as u8];
        let dim = [255u8, 255, 255, (130.0 * ca) as u8];
        let down = fmt_rate(self.traffic.down);
        let up = fmt_rate(self.traffic.up);
        self.txt("↓ Unduh", X0 + 16.0, Y0 + 66.0, 12.0, dim);
        self.txt(&down, X0 + 16.0, Y0 + 90.0, 22.0, white);
        self.txt("↑ Unggah", X0 + 186.0, Y0 + 66.0, 12.0, dim);
        self.txt(&up, X0 + 186.0, Y0 + 90.0, 22.0, white);

        let (gx, gy, gw, gh) = (X0 + 14.0, Y0 + 108.0, 312.0f32, 56.0f32);
        if let Some(p) = rounded_rect(gx, gy, gw, gh, 8.0) {
            fill(&mut self.pixmap, &p, [255, 255, 255, (14.0 * ca) as u8], base);
        }
        let hist = self.traffic.hist.clone();
        if hist.len() >= 2 {
            let mut maxv = 1024.0f32;
            for v in hist.iter() {
                if *v > maxv {
                    maxv = *v;
                }
            }
            let mut pts: Vec<Pt> = Vec::new();
            for (i, v) in hist.iter().enumerate() {
                let x = gx + 6.0 + (gw - 12.0) * (i as f32) / 59.0;
                let y = gy + gh - 6.0 - (gh - 12.0) * (*v / maxv);
                pts.push((x, y));
            }
            if let Some(p) = poly_path(&pts, false) {
                stroke(&mut self.pixmap, &p, [ACCENT[0], ACCENT[1], ACCENT[2], (240.0 * ca) as u8], 1.8, base);
            }
        }
        self.txt_c("60 detik terakhir (unduh)", CX, Y0 + 180.0, 10.0, dim);
        if !self.cfg.on[Feat::Traffic.idx()] {
            self.txt_c("Addon ini mati. Nyalakan di Addons.", CX, Y0 + 150.0, 11.0, dim);
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

    fn paint_tip(&mut self, base: Transform) {
        let (s, cx, y) = match &self.tip {
            Some(t) => t.clone(),
            None => return,
        };
        let w = self.txt_w(&s, 11.0) + 22.0;
        let x = (cx - w / 2.0).max(4.0).min(W - w - 4.0);
        if let Some(p) = rounded_rect(x, y, w, 22.0, 11.0) {
            fill(&mut self.pixmap, &p, [30, 30, 36, 250], base);
            stroke(&mut self.pixmap, &p, [255, 255, 255, 50], 1.0, base);
        }
        self.txt(&s, x + 11.0, y + 15.0, 11.0, [255, 255, 255, 240]);
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
        WM_CHAR => {
            let c = wparam as u32;
            with_app(|a| a.on_char(c));
            0
        }
        WM_KILLFOCUS => {
            with_app(|a| a.cancel_input());
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
