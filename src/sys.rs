//! Semua yang berhubungan dengan Windows: suara, layar, daya, clipboard, dll.

use std::mem;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use tiny_skia::Pixmap;
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{BOOL, FILETIME};
use winapi::shared::windef::HWND;
use winapi::um::combaseapi::CoCreateInstance;
use winapi::um::endpointvolume::IAudioEndpointVolume;
use winapi::um::minwinbase::SYSTEMTIME;
use winapi::um::mmdeviceapi::{eCapture, eConsole, eRender, EDataFlow, IMMDevice, IMMDeviceEnumerator};
use winapi::um::processthreadsapi::GetSystemTimes;
use winapi::um::sysinfoapi::GetLocalTime;
use winapi::um::wingdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};
use winapi::um::winbase::{
    GetSystemPowerStatus, GlobalAlloc, GlobalLock, GlobalUnlock, SetThreadExecutionState, GMEM_MOVEABLE,
    SYSTEM_POWER_STATUS,
};
use winapi::um::winuser::*;
use winapi::Interface;

// CLSCTX_ALL = INPROC_SERVER | INPROC_HANDLER | LOCAL_SERVER | REMOTE_SERVER
const CLSCTX_ALL: u32 = 0x1 | 0x2 | 0x4 | 0x10;

static CLSID_MMDEVICE_ENUMERATOR: GUID = GUID {
    Data1: 0xBCDE0395,
    Data2: 0xE52F,
    Data3: 0x467C,
    Data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
};

// ---------------------------------------------------------------------------
// Volume dan mikrofon (Windows Core Audio)
// ---------------------------------------------------------------------------
fn with_endpoint<T, F: FnOnce(*mut IAudioEndpointVolume) -> Option<T>>(flow: EDataFlow, f: F) -> Option<T> {
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
        let hr = (*en).GetDefaultAudioEndpoint(flow, eConsole, &mut dev);
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
            result = f(ep);
            (*ep).Release();
        }
        (*dev).Release();
        (*en).Release();
        result
    }
}

/// Ubah volume utama sebesar `delta` persen. Hasilnya volume baru (0-100).
pub fn change_volume(delta: f32) -> Option<i32> {
    with_endpoint(eRender, |ep| unsafe {
        let mut cur = 0.0f32;
        if (*ep).GetMasterVolumeLevelScalar(&mut cur) < 0 {
            return None;
        }
        let new = (cur * 100.0 + delta).clamp(0.0, 100.0);
        if delta > 0.0 {
            let mut muted: BOOL = 0;
            (*ep).GetMute(&mut muted);
            if muted != 0 {
                (*ep).SetMute(0, null());
            }
        }
        (*ep).SetMasterVolumeLevelScalar(new / 100.0, null());
        Some(new.round() as i32)
    })
}

/// Bisukan/aktifkan mikrofon bawaan. Hasilnya: true jika sekarang bisu.
pub fn mic_toggle() -> Option<bool> {
    with_endpoint(eCapture, |ep| unsafe {
        let mut m: BOOL = 0;
        if (*ep).GetMute(&mut m) < 0 {
            return None;
        }
        let new: BOOL = if m != 0 { 0 } else { 1 };
        (*ep).SetMute(new, null());
        Some(new != 0)
    })
}

pub fn mic_muted() -> Option<bool> {
    with_endpoint(eCapture, |ep| unsafe {
        let mut m: BOOL = 0;
        if (*ep).GetMute(&mut m) < 0 {
            None
        } else {
            Some(m != 0)
        }
    })
}

// ---------------------------------------------------------------------------
// PowerShell (untuk kecerahan dan jadwal shalat), dikerjakan di latar belakang
// ---------------------------------------------------------------------------
pub fn run_powershell(script: &str) -> Option<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub struct Brightness {
    level: f32,
    available: Arc<AtomicBool>,
    initial: Arc<AtomicI32>,
    tx: Sender<i32>,
}

impl Brightness {
    pub fn new() -> Brightness {
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
                    v = newer;
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

    pub fn change(&mut self, delta: f32) -> Option<i32> {
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
// Tombol media, daya, awake, caps lock
// ---------------------------------------------------------------------------
pub fn media_key(vk: u8) {
    unsafe {
        keybd_event(vk, 0, 0, 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
}

pub fn caps_on() -> bool {
    unsafe { (GetKeyState(0x14) & 1) != 0 }
}

pub fn set_awake(on: bool) {
    unsafe {
        let flags: u32 = if on { 0x8000_0003 } else { 0x8000_0000 };
        SetThreadExecutionState(flags);
    }
}

fn run_hidden(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).creation_flags(0x0800_0000).spawn();
}

pub fn shutdown_pc() {
    run_hidden("shutdown", &["/s", "/t", "0"]);
}

pub fn restart_pc() {
    run_hidden("shutdown", &["/r", "/t", "0"]);
}

pub fn sleep_pc() {
    run_hidden("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]);
}

pub fn lock_pc() {
    unsafe {
        LockWorkStation();
    }
}

pub fn open_sound_settings() {
    let _ = Command::new("explorer").arg("ms-settings:sound").spawn();
}

pub fn beep() {
    unsafe {
        MessageBeep(0x40);
    }
}

// ---------------------------------------------------------------------------
// Waktu, baterai, CPU
// ---------------------------------------------------------------------------
pub struct Now {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
}

pub fn now_local() -> Now {
    unsafe {
        let mut st: SYSTEMTIME = mem::zeroed();
        GetLocalTime(&mut st);
        Now {
            year: st.wYear as i32,
            month: st.wMonth as u32,
            day: st.wDay as u32,
            hour: st.wHour as u32,
            min: st.wMinute as u32,
            sec: st.wSecond as u32,
        }
    }
}

#[derive(Clone, Copy)]
pub struct PowerInfo {
    pub has_battery: bool,
    pub percent: u32,
    pub plugged: bool,
}

pub fn power_info() -> PowerInfo {
    unsafe {
        let mut s: SYSTEM_POWER_STATUS = mem::zeroed();
        if GetSystemPowerStatus(&mut s) == 0 {
            return PowerInfo { has_battery: false, percent: 100, plugged: true };
        }
        let pct = s.BatteryLifePercent as u32;
        let has = s.BatteryFlag != 128 && s.BatteryFlag != 255 && pct <= 100;
        PowerInfo {
            has_battery: has,
            percent: if pct <= 100 { pct } else { 100 },
            plugged: s.ACLineStatus == 1,
        }
    }
}

pub struct CpuMeter {
    last: Option<(u64, u64)>,
}

impl CpuMeter {
    pub fn new() -> CpuMeter {
        CpuMeter { last: None }
    }

    /// Pemakaian CPU sejak pemanggilan terakhir, 0-100.
    pub fn usage(&mut self) -> f32 {
        unsafe {
            let mut i: FILETIME = mem::zeroed();
            let mut k: FILETIME = mem::zeroed();
            let mut u: FILETIME = mem::zeroed();
            if GetSystemTimes(&mut i, &mut k, &mut u) == 0 {
                return 0.0;
            }
            let ft = |f: &FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
            let idle = ft(&i);
            let total = ft(&k) + ft(&u);
            let r = match self.last {
                Some((pi, pt)) if total > pt => {
                    let dt = (total - pt) as f32;
                    let di = (idle.saturating_sub(pi)) as f32;
                    ((dt - di) / dt * 100.0).clamp(0.0, 100.0)
                }
                _ => 0.0,
            };
            self.last = Some((idle, total));
            r
        }
    }
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------
pub fn clipboard_set(text: &str) -> bool {
    unsafe {
        let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = w.len() * 2;
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if h.is_null() {
            return false;
        }
        let p = GlobalLock(h) as *mut u16;
        if p.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
        GlobalUnlock(h);
        if OpenClipboard(null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let ok = !SetClipboardData(CF_UNICODETEXT, h).is_null();
        CloseClipboard();
        ok
    }
}

pub fn clipboard_get(hwnd: HWND) -> Option<String> {
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT);
        let mut out = None;
        if !h.is_null() {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let mut len = 0usize;
                while len < 4000 && *p.add(len) != 0 {
                    len += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
}

// ---------------------------------------------------------------------------
// Screenshot layar penuh -> Gambar\Dyland\*.png
// ---------------------------------------------------------------------------
pub fn screenshot(hwnd: HWND) -> Option<String> {
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
        std::thread::sleep(std::time::Duration::from_millis(180));

        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        let sdc = GetDC(null_mut());
        let mdc = CreateCompatibleDC(sdc);
        let mut bmi: BITMAPINFO = mem::zeroed();
        bmi.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = 0;
        let mut bits: *mut winapi::ctypes::c_void = null_mut();
        let bmp = CreateDIBSection(mdc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let old = SelectObject(mdc, bmp as *mut _);
        BitBlt(mdc, 0, 0, w, h, sdc, 0, 0, SRCCOPY | CAPTUREBLT);

        let mut result = None;
        if !bits.is_null() && w > 0 && h > 0 {
            if let Some(mut pm) = Pixmap::new(w as u32, h as u32) {
                let n = (w as usize) * (h as usize);
                let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
                let dst = pm.data_mut();
                for i in 0..n {
                    dst[i * 4] = src[i * 4 + 2];
                    dst[i * 4 + 1] = src[i * 4 + 1];
                    dst[i * 4 + 2] = src[i * 4];
                    dst[i * 4 + 3] = 255;
                }
                let home = std::env::var("USERPROFILE").unwrap_or_else(|_| String::from("."));
                let dir = format!("{}\\Pictures\\Dyland", home);
                let _ = std::fs::create_dir_all(&dir);
                let t = now_local();
                let path = format!(
                    "{}\\Dyland_{:04}{:02}{:02}_{:02}{:02}{:02}.png",
                    dir, t.year, t.month, t.day, t.hour, t.min, t.sec
                );
                if pm.save_png(&path).is_ok() {
                    result = Some(path);
                }
            }
        }

        SelectObject(mdc, old);
        DeleteObject(bmp as *mut _);
        DeleteDC(mdc);
        ReleaseDC(null_mut(), sdc);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        result
    }
}

// ---------------------------------------------------------------------------
// Jadwal shalat (Aladhan, metode Kemenag RI) lewat PowerShell
// ---------------------------------------------------------------------------
pub struct Prayer {
    pub status: u8, // 0 memuat, 1 siap, 2 gagal
    pub day: u32,   // tanggal data (yyyymmdd)
    pub times: Vec<(String, u32)>, // nama, menit sejak 00:00
}

pub type PrayerShared = Arc<Mutex<Prayer>>;

pub fn new_prayer() -> PrayerShared {
    Arc::new(Mutex::new(Prayer { status: 0, day: 0, times: Vec::new() }))
}

pub fn fetch_prayer(shared: PrayerShared, city: String, country: String, day: u32) {
    std::thread::spawn(move || {
        let clean = |s: &str| -> String {
            s.chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
                .collect::<String>()
                .replace(' ', "%20")
        };
        let script = format!(
            "[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;\
             $r=Invoke-RestMethod -Uri 'https://api.aladhan.com/v1/timingsByCity?city={}&country={}&method=20' -TimeoutSec 20;\
             $t=$r.data.timings;\
             ($t.Imsak.Substring(0,5)+' '+$t.Fajr.Substring(0,5)+' '+$t.Dhuhr.Substring(0,5)+' '+$t.Asr.Substring(0,5)+' '+$t.Maghrib.Substring(0,5)+' '+$t.Isha.Substring(0,5))",
            clean(&city),
            clean(&country)
        );
        let names = ["Imsak", "Subuh", "Dzuhur", "Ashar", "Maghrib", "Isya"];
        let mut times: Vec<(String, u32)> = Vec::new();
        if let Some(text) = run_powershell(&script) {
            let line = text.lines().last().unwrap_or("");
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 6 {
                for (i, p) in parts.iter().enumerate() {
                    if let Some((h, m)) = p.split_once(':') {
                        if let (Ok(h), Ok(m)) = (h.parse::<u32>(), m.parse::<u32>()) {
                            times.push((names[i].to_string(), h * 60 + m));
                        }
                    }
                }
            }
        }
        if let Ok(mut g) = shared.lock() {
            if times.len() == 6 {
                g.status = 1;
                g.day = day;
                g.times = times;
            } else if g.times.is_empty() {
                g.status = 2;
            }
        }
    });
}
