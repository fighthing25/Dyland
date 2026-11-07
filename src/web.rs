//! Sambung HP: server web kecil di PC. HP membuka alamatnya lewat browser
//! (Wi-Fi yang sama), memasukkan PIN, lalu bisa kirim/ambil file dan teks.
//! Hanya memakai pustaka standar Rust.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct ShelfFile {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub size: u64,
}

pub struct Shared {
    pub pin: String,
    pub shelf: Vec<ShelfFile>,
    pub inbox: Vec<String>,
    pub clip_in: Option<String>,
    pub pc_clip: String,
    pub event: Option<String>,
    pub next_id: u32,
    pub recv_dir: String,
    fails: u32,
    locked_until: Option<Instant>,
}

pub type WebShared = Arc<Mutex<Shared>>;

pub struct WebServer {
    pub stop: Arc<AtomicBool>,
    pub port: u16,
}

pub fn random_pin() -> String {
    let mut h = RandomState::new().build_hasher();
    h.write_u8(7);
    format!("{:06}", h.finish() % 1_000_000)
}

pub fn new_shared() -> WebShared {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| String::from("."));
    Arc::new(Mutex::new(Shared {
        pin: random_pin(),
        shelf: Vec::new(),
        inbox: Vec::new(),
        clip_in: None,
        pc_clip: String::new(),
        event: None,
        next_id: 1,
        recv_dir: format!("{}\\Downloads\\Dyland", home),
        fails: 0,
        locked_until: None,
    }))
}

pub fn new_pin(sh: &WebShared) {
    if let Ok(mut g) = sh.lock() {
        g.pin = random_pin();
        g.fails = 0;
        g.locked_until = None;
    }
}

pub fn get_pin(sh: &WebShared) -> String {
    match sh.lock() {
        Ok(g) => g.pin.clone(),
        Err(_) => String::new(),
    }
}

pub fn recv_dir(sh: &WebShared) -> String {
    match sh.lock() {
        Ok(g) => g.recv_dir.clone(),
        Err(_) => String::from("."),
    }
}

pub fn last_inbox(sh: &WebShared) -> Option<String> {
    match sh.lock() {
        Ok(g) => g.inbox.last().cloned(),
        Err(_) => None,
    }
}

pub fn set_pc_clip(sh: &WebShared, text: &str) {
    if let Ok(mut g) = sh.lock() {
        g.pc_clip = text.chars().take(4000).collect();
    }
}

// ---------------------------------------------------------------------------
// Rak file
// ---------------------------------------------------------------------------
pub fn shelf_list(sh: &WebShared) -> Vec<ShelfFile> {
    match sh.lock() {
        Ok(g) => g.shelf.clone(),
        Err(_) => Vec::new(),
    }
}

pub fn add_shelf(sh: &WebShared, path: String) -> bool {
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !meta.is_file() {
        return false;
    }
    let name = match std::path::Path::new(&path).file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => return false,
    };
    if let Ok(mut g) = sh.lock() {
        if g.shelf.iter().any(|f| f.path == path) {
            return false;
        }
        let id = g.next_id;
        g.next_id += 1;
        g.shelf.push(ShelfFile { id, name, path, size: meta.len() });
        return true;
    }
    false
}

pub fn remove_shelf(sh: &WebShared, idx: usize) {
    if let Ok(mut g) = sh.lock() {
        if idx < g.shelf.len() {
            g.shelf.remove(idx);
        }
    }
}

pub fn clear_shelf(sh: &WebShared) {
    if let Ok(mut g) = sh.lock() {
        g.shelf.clear();
    }
}

// ---------------------------------------------------------------------------
// Alamat jaringan dan QR
// ---------------------------------------------------------------------------
pub fn local_ip() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let a = sock.local_addr().ok()?;
    Some(a.ip().to_string())
}

pub fn make_qr(data: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    let w = code.width();
    let cols = code.to_colors();
    let cells: Vec<bool> = cols.iter().map(|c| *c == qrcode::Color::Dark).collect();
    Some((w, cells))
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------
pub fn start(shared: WebShared) -> Option<WebServer> {
    let mut found: Option<(TcpListener, u16)> = None;
    for port in 8765u16..8776u16 {
        if let Ok(l) = TcpListener::bind(("0.0.0.0", port)) {
            found = Some((l, port));
            break;
        }
    }
    let (listener, port) = found?;
    if listener.set_nonblocking(true).is_err() {
        return None;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::spawn(move || loop {
        if stop2.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((s, _)) => {
                let sh = shared.clone();
                std::thread::spawn(move || handle(s, sh));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    });
    Some(WebServer { stop, port })
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn hexv(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if b[i] == b'%' && i + 2 < b.len() + 0 && hexv(b[i + 1]).is_some() && hexv(b[i + 2]).is_some() {
            let hi = hexv(b[i + 1]).unwrap_or(0);
            let lo = hexv(b[i + 2]).unwrap_or(0);
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        let ok = c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.' || c == b'~';
        if ok {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{:02X}", c));
        }
    }
    out
}

fn qparam(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(url_decode(v));
            }
        }
    }
    None
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let t = cleaned.trim().trim_matches('.').trim().to_string();
    let t: String = t.chars().take(120).collect();
    if t.is_empty() {
        String::from("berkas")
    } else {
        t
    }
}

fn unique_path(dir: &str, name: &str) -> String {
    let first = format!("{}\\{}", dir, name);
    if !std::path::Path::new(&first).exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.to_string(), String::new()),
    };
    let mut n = 1;
    loop {
        let p = format!("{}\\{} ({}){}", dir, stem, n, ext);
        if !std::path::Path::new(&p).exists() {
            return p;
        }
        n += 1;
        if n > 9999 {
            return p;
        }
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 32 => o.push(' '),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn state_json(sh: &WebShared) -> String {
    let g = match sh.lock() {
        Ok(g) => g,
        Err(_) => return String::from("{}"),
    };
    let mut s = String::from("{\"clip\":");
    s.push_str(&json_str(&g.pc_clip));
    s.push_str(",\"shelf\":[");
    for (i, f) in g.shelf.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("{{\"id\":{},\"name\":{},\"size\":{}}}", f.id, json_str(&f.name), f.size));
    }
    s.push_str("],\"inbox\":[");
    let start = if g.inbox.len() > 8 { g.inbox.len() - 8 } else { 0 };
    for (i, n) in g.inbox[start..].iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&json_str(n));
    }
    s.push_str("]}");
    s
}

/// 1 = PIN benar, 0 = salah, 2 = dikunci sementara.
fn check_pin(sh: &WebShared, given: &str) -> u8 {
    let mut g = match sh.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    if let Some(t) = g.locked_until {
        if Instant::now() < t {
            return 2;
        }
        g.locked_until = None;
        g.fails = 0;
    }
    if !given.is_empty() && given == g.pin {
        g.fails = 0;
        1
    } else {
        g.fails += 1;
        if g.fails >= 5 {
            g.locked_until = Some(Instant::now() + Duration::from_secs(60));
        }
        0
    }
}

fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        status,
        ctype,
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
    let _ = s.flush();
}

fn read_body(s: &mut TcpStream, first: Vec<u8>, clen: u64, limit: u64) -> Option<Vec<u8>> {
    if clen > limit {
        return None;
    }
    let mut body = first;
    let mut tmp = [0u8; 8192];
    while (body.len() as u64) < clen {
        let n = s.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(clen as usize);
    Some(body)
}

fn handle(mut s: TcpStream, sh: WebShared) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(60)));

    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        let n = match s.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => n,
            Err(_) => return,
        };
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = find_sub(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > 16384 {
            return;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut clen: u64 = 0;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                clen = v.trim().parse::<u64>().unwrap_or(0);
            }
        }
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.clone(), String::new()),
    };
    let body_start: Vec<u8> = buf[header_end..].to_vec();

    if method == "GET" && path == "/" {
        respond(&mut s, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes());
        return;
    }
    if path == "/favicon.ico" {
        respond(&mut s, "204 No Content", "text/plain", b"");
        return;
    }

    let pin = qparam(&query, "pin").unwrap_or_default();
    match check_pin(&sh, &pin) {
        1 => {}
        2 => {
            respond(&mut s, "429 Too Many Requests", "text/plain; charset=utf-8", b"Terlalu banyak percobaan");
            return;
        }
        _ => {
            respond(&mut s, "401 Unauthorized", "text/plain; charset=utf-8", b"PIN salah");
            return;
        }
    }

    if method == "GET" && path == "/api/state" {
        let body = state_json(&sh);
        respond(&mut s, "200 OK", "application/json; charset=utf-8", body.as_bytes());
        return;
    }

    if method == "POST" && path == "/api/clip" {
        match read_body(&mut s, body_start, clen, 200_000) {
            Some(b) => {
                let text = String::from_utf8_lossy(&b).to_string();
                if let Ok(mut g) = sh.lock() {
                    g.clip_in = Some(text);
                }
                respond(&mut s, "200 OK", "application/json", b"{\"ok\":true}");
            }
            None => respond(&mut s, "400 Bad Request", "text/plain", b"gagal"),
        }
        return;
    }

    if method == "POST" && path == "/api/up" {
        handle_upload(&mut s, &sh, &query, body_start, clen);
        return;
    }

    if method == "GET" && path.starts_with("/dl/") {
        handle_download(&mut s, &sh, &path);
        return;
    }

    respond(&mut s, "404 Not Found", "text/plain; charset=utf-8", b"tidak ditemukan");
}

fn handle_upload(s: &mut TcpStream, sh: &WebShared, query: &str, first: Vec<u8>, clen: u64) {
    if clen == 0 || clen > 4_000_000_000 {
        respond(s, "400 Bad Request", "text/plain", b"ukuran tidak valid");
        return;
    }
    let name = sanitize(&qparam(query, "name").unwrap_or_else(|| String::from("berkas")));
    let dir = recv_dir(sh);
    if std::fs::create_dir_all(&dir).is_err() {
        respond(s, "500 Internal Server Error", "text/plain", b"folder gagal dibuat");
        return;
    }
    let path = unique_path(&dir, &name);
    let mut f = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(_) => {
            respond(s, "500 Internal Server Error", "text/plain", b"file gagal dibuat");
            return;
        }
    };
    let mut written: u64 = 0;
    if !first.is_empty() {
        let take = first.len().min(clen as usize);
        if f.write_all(&first[..take]).is_err() {
            drop(f);
            let _ = std::fs::remove_file(&path);
            respond(s, "500 Internal Server Error", "text/plain", b"gagal menulis");
            return;
        }
        written += take as u64;
    }
    let mut tmp = vec![0u8; 65536];
    while written < clen {
        let want = ((clen - written) as usize).min(tmp.len());
        let n = match s.read(&mut tmp[..want]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if f.write_all(&tmp[..n]).is_err() {
            break;
        }
        written += n as u64;
    }
    drop(f);
    if written == clen {
        let shown = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(name);
        if let Ok(mut g) = sh.lock() {
            g.inbox.push(shown.clone());
            g.event = Some(format!("Diterima dari HP: {}", shown));
        }
        respond(s, "200 OK", "application/json", b"{\"ok\":true}");
    } else {
        let _ = std::fs::remove_file(&path);
        respond(s, "400 Bad Request", "text/plain", b"unggahan tidak lengkap");
    }
}

fn handle_download(s: &mut TcpStream, sh: &WebShared, path: &str) {
    let id: u32 = path.trim_start_matches("/dl/").parse::<u32>().unwrap_or(0);
    let found = shelf_list(sh).into_iter().find(|f| f.id == id);
    let file = match found {
        Some(f) => f,
        None => {
            respond(s, "404 Not Found", "text/plain; charset=utf-8", b"file tidak ada di rak");
            return;
        }
    };
    let mut fh = match std::fs::File::open(&file.path) {
        Ok(f) => f,
        Err(_) => {
            respond(s, "404 Not Found", "text/plain; charset=utf-8", b"file tidak bisa dibuka");
            return;
        }
    };
    let len = fh.metadata().map(|m| m.len()).unwrap_or(file.size);
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename*=UTF-8''{}\r\nConnection: close\r\n\r\n",
        len,
        pct_encode(&file.name)
    );
    if s.write_all(head.as_bytes()).is_err() {
        return;
    }
    let mut tmp = vec![0u8; 65536];
    loop {
        let n = match fh.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if s.write_all(&tmp[..n]).is_err() {
            break;
        }
    }
    let _ = s.flush();
}

// ---------------------------------------------------------------------------
// Halaman web untuk HP
// ---------------------------------------------------------------------------
const PAGE: &str = r##"<!doctype html>
<html lang="id"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>DYLAND - Sambung HP</title>
<style>
body{margin:0 auto;max-width:560px;padding:16px;background:#0b0b0e;color:#eee;font:16px system-ui,sans-serif}
h1{font-size:20px;margin:0 0 12px}
h2{font-size:15px;margin:20px 0 8px;color:#9ac8ff}
.card{background:#16161c;border-radius:14px;padding:14px;margin-bottom:10px}
button{font:inherit;background:#2b6fff;color:#fff;border:0;border-radius:10px;padding:10px 14px}
input,textarea{font:inherit;box-sizing:border-box;width:100%;border-radius:10px;border:1px solid #333;background:#0f0f13;color:#eee;padding:10px}
textarea{min-height:90px}
.row{display:flex;justify-content:space-between;gap:10px;padding:9px 0;border-bottom:1px solid #26262e;align-items:center}
.row span{word-break:break-all}
a{color:#7fb8ff}
.dim{color:#8a8a96;font-size:13px}
</style></head><body>
<h1>DYLAND &middot; Sambung HP</h1>
<div id="login" class="card">
<div>Masukkan PIN yang tampil di PC</div>
<input id="pin" inputmode="numeric" maxlength="6" style="font-size:24px;margin:10px 0">
<button onclick="login()">Sambungkan</button>
<div id="msg" class="dim" style="margin-top:8px"></div>
</div>
<div id="app" style="display:none">
<h2>Teks dan clipboard</h2>
<div class="card">
<textarea id="t" placeholder="Ketik atau tempel teks di sini"></textarea>
<p><button onclick="sendText()">Kirim ke clipboard PC</button> <span id="tm" class="dim"></span></p>
<div class="dim">Clipboard PC saat ini (tekan lama untuk menyalin):</div>
<textarea id="pc" readonly></textarea>
</div>
<h2>Kirim file ke PC</h2>
<div class="card"><input type="file" id="f" multiple onchange="sendFiles()"><div id="up" class="dim" style="margin-top:8px"></div></div>
<h2>File dari PC</h2>
<div class="card" id="shelf"></div>
</div>
<script>
var PIN='',busy=false;
function api(p){return p+(p.indexOf('?')<0?'?':'&')+'pin='+encodeURIComponent(PIN);}
function login(){
  PIN=document.getElementById('pin').value.trim();
  fetch(api('/api/state')).then(function(r){
    if(r.ok)return r.json();
    throw r.status;
  }).then(function(s){
    document.getElementById('login').style.display='none';
    document.getElementById('app').style.display='block';
    render(s);
    setInterval(poll,3000);
  }).catch(function(e){
    document.getElementById('msg').textContent=(e===429?'Terlalu banyak percobaan. Tunggu 1 menit.':'PIN salah atau PC tidak terjangkau');
  });
}
function poll(){
  if(busy)return;
  fetch(api('/api/state')).then(function(r){return r.json();}).then(render).catch(function(){});
}
function render(s){
  var pc=document.getElementById('pc');
  if(document.activeElement!==pc)pc.value=s.clip||'';
  var box=document.getElementById('shelf');
  box.innerHTML='';
  if(!s.shelf.length){
    var d=document.createElement('div');d.className='dim';
    d.textContent='Belum ada file di rak PC. Tarik file ke pulau DYLAND di PC.';
    box.appendChild(d);
  }
  s.shelf.forEach(function(f){
    var r=document.createElement('div');r.className='row';
    var n=document.createElement('span');n.textContent=f.name+' ('+size(f.size)+')';
    var a=document.createElement('a');a.href=api('/dl/'+f.id);a.textContent='Unduh';
    r.appendChild(n);r.appendChild(a);box.appendChild(r);
  });
}
function size(b){
  if(b<1024)return b+' B';
  if(b<1048576)return Math.round(b/1024)+' KB';
  return (b/1048576).toFixed(1)+' MB';
}
function sendText(){
  var v=document.getElementById('t').value;
  fetch(api('/api/clip'),{method:'POST',body:v}).then(function(r){
    document.getElementById('tm').textContent=r.ok?'Terkirim':'Gagal';
  }).catch(function(){document.getElementById('tm').textContent='Gagal';});
}
function sendFiles(){
  var files=document.getElementById('f').files;
  var info=document.getElementById('up');
  var i=0;
  busy=true;
  function next(){
    if(i>=files.length){busy=false;info.textContent='Selesai';document.getElementById('f').value='';return;}
    var f=files[i];
    var x=new XMLHttpRequest();
    x.open('POST',api('/api/up?name='+encodeURIComponent(f.name)));
    x.upload.onprogress=function(e){
      if(e.lengthComputable)info.textContent='Mengirim '+f.name+' '+Math.round(e.loaded*100/e.total)+'%';
    };
    x.onload=function(){
      if(x.status!==200)info.textContent='Gagal mengirim '+f.name;
      i++;next();
    };
    x.onerror=function(){busy=false;info.textContent='Gagal mengirim '+f.name;};
    x.send(f);
  }
  next();
}
if(location.hash.length>1){document.getElementById('pin').value=location.hash.substring(1);login();}
</script></body></html>
"##;
