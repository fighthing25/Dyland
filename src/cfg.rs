//! Pengaturan dan daftar tugas yang disimpan di %APPDATA%\Dyland.

pub const N_FEAT: usize = 13;

pub const FEAT_KEYS: [&str; N_FEAT] = [
    "musik", "suara", "mic", "screenshot", "awake", "kunci", "daya", "kalender", "clipboard", "shalat", "traffic",
    "addons", "pengaturan",
];

#[derive(Clone)]
pub struct Cfg {
    pub city: String,
    pub country: String,
    pub show_jawa: bool,
    pub show_hijri: bool,
    pub notify: bool,
    pub on: [bool; N_FEAT],
}

fn dir() -> Option<String> {
    std::env::var("APPDATA").ok().map(|a| format!("{}\\Dyland", a))
}

impl Cfg {
    pub fn new() -> Cfg {
        let mut on = [true; N_FEAT];
        on[10] = false; // traffic monitor: addon, mati secara bawaan
        Cfg {
            city: String::from("Jakarta"),
            country: String::from("Indonesia"),
            show_jawa: true,
            show_hijri: true,
            notify: true,
            on,
        }
    }

    pub fn load() -> Cfg {
        let mut c = Cfg::new();
        if let Some(d) = dir() {
            if let Ok(content) = std::fs::read_to_string(format!("{}\\config.txt", d)) {
                for line in content.lines() {
                    if let Some((k, v)) = line.split_once('=') {
                        let k = k.trim();
                        let v = v.trim();
                        let b = v == "1";
                        match k {
                            "kota" => {
                                if !v.is_empty() {
                                    c.city = v.to_string();
                                }
                            }
                            "negara" => {
                                if !v.is_empty() {
                                    c.country = v.to_string();
                                }
                            }
                            "jawa" => c.show_jawa = b,
                            "hijri" => c.show_hijri = b,
                            "notifikasi" => c.notify = b,
                            _ => {
                                if let Some(name) = k.strip_prefix("fitur_") {
                                    for i in 0..N_FEAT {
                                        if FEAT_KEYS[i] == name {
                                            c.on[i] = b;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        c.on[11] = true;
        c.on[12] = true;
        c
    }

    pub fn save(&self) {
        if let Some(d) = dir() {
            let _ = std::fs::create_dir_all(&d);
            let mut s = String::new();
            s.push_str(&format!("kota={}\r\n", self.city));
            s.push_str(&format!("negara={}\r\n", self.country));
            s.push_str(&format!("jawa={}\r\n", if self.show_jawa { 1 } else { 0 }));
            s.push_str(&format!("hijri={}\r\n", if self.show_hijri { 1 } else { 0 }));
            s.push_str(&format!("notifikasi={}\r\n", if self.notify { 1 } else { 0 }));
            for i in 0..N_FEAT {
                s.push_str(&format!("fitur_{}={}\r\n", FEAT_KEYS[i], if self.on[i] { 1 } else { 0 }));
            }
            let _ = std::fs::write(format!("{}\\config.txt", d), s);
        }
    }
}

#[derive(Clone)]
pub struct Todo {
    pub date: u32, // yyyymmdd
    pub done: bool,
    pub text: String,
}

pub fn load_todos() -> Vec<Todo> {
    let mut v = Vec::new();
    if let Some(d) = dir() {
        if let Ok(content) = std::fs::read_to_string(format!("{}\\todo.txt", d)) {
            for line in content.lines() {
                let mut it = line.splitn(3, '|');
                let date = it.next().and_then(|s| s.trim().parse::<u32>().ok());
                let done = it.next().map(|s| s.trim() == "1");
                let text = it.next();
                if let (Some(date), Some(done), Some(text)) = (date, done, text) {
                    if !text.trim().is_empty() {
                        v.push(Todo { date, done, text: text.trim().to_string() });
                    }
                }
            }
        }
    }
    v
}

pub fn save_todos(items: &[Todo]) {
    if let Some(d) = dir() {
        let _ = std::fs::create_dir_all(&d);
        let mut s = String::new();
        for t in items {
            let clean: String = t.text.replace('|', " ").replace('\r', " ").replace('\n', " ");
            s.push_str(&format!("{}|{}|{}\r\n", t.date, if t.done { 1 } else { 0 }, clean));
        }
        let _ = std::fs::write(format!("{}\\todo.txt", d), s);
    }
}
