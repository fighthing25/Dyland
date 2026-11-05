//! Hitung tanggal: Masehi, pasaran Jawa, dan Hijri (perkiraan tabel).

pub const HARI: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
pub const PASARAN: [&str; 5] = ["Legi", "Pahing", "Pon", "Wage", "Kliwon"];
pub const BULAN: [&str; 12] = [
    "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober",
    "November", "Desember",
];
pub const HIJRI: [&str; 12] = [
    "Muharram", "Safar", "Rabiul Awal", "Rabiul Akhir", "Jumadil Awal", "Jumadil Akhir", "Rajab", "Syaban",
    "Ramadan", "Syawal", "Zulkaidah", "Zulhijah",
];

/// Jumlah hari sejak 1 Januari 1970.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = (if m <= 2 { y - 1 } else { y }) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 0 = Minggu ... 6 = Sabtu
pub fn weekday(days: i64) -> usize {
    (days + 4).rem_euclid(7) as usize
}

/// 0 = Legi, 1 = Pahing, 2 = Pon, 3 = Wage, 4 = Kliwon
pub fn pasaran(days: i64) -> usize {
    (days + 3).rem_euclid(5) as usize
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// (tahun, bulan 1-12, hari) Hijri, perkiraan dengan perhitungan tabel.
pub fn hijri(days: i64) -> (i64, i64, i64) {
    let jd = days + 2440588;
    let mut l = jd - 1948440 + 10632;
    let n = (l - 1) / 10631;
    l = l - 10631 * n + 354;
    let j = ((10985 - l) / 5316) * ((50 * l) / 17719) + (l / 5670) * ((43 * l) / 15238);
    l = l - ((30 - j) / 15) * ((17719 * j) / 50) - (j / 16) * ((15238 * j) / 43) + 29;
    let m = (24 * l) / 709;
    let d = l - (709 * m) / 24;
    let y = 30 * n + j - 30;
    (y, m, d)
}

/// Tanggal Paskah (bulan, hari) menurut kalender Gregorian.
pub fn easter(y: i32) -> (u32, u32) {
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;
    (month as u32, day as u32)
}

/// Hari libur nasional Indonesia. 2026 memakai daftar resmi; tahun lain adalah perkiraan.
pub fn holiday(y: i32, m: u32, d: u32) -> Option<&'static str> {
    if y == 2026 {
        return match (m, d) {
            (1, 1) => Some("Tahun Baru Masehi"),
            (1, 16) => Some("Isra Mikraj Nabi Muhammad SAW"),
            (2, 17) => Some("Tahun Baru Imlek"),
            (3, 19) => Some("Hari Suci Nyepi"),
            (3, 21) | (3, 22) => Some("Idul Fitri 1447 H"),
            (4, 3) => Some("Wafat Isa Almasih"),
            (4, 5) => Some("Hari Paskah"),
            (5, 1) => Some("Hari Buruh Internasional"),
            (5, 14) => Some("Kenaikan Isa Almasih"),
            (5, 27) => Some("Idul Adha 1447 H"),
            (5, 31) => Some("Hari Raya Waisak"),
            (6, 1) => Some("Hari Lahir Pancasila"),
            (6, 16) => Some("Tahun Baru Islam 1448 H"),
            (8, 17) => Some("Hari Kemerdekaan RI"),
            (8, 25) => Some("Maulid Nabi Muhammad SAW"),
            (12, 25) => Some("Hari Raya Natal"),
            _ => None,
        };
    }
    match (m, d) {
        (1, 1) => return Some("Tahun Baru Masehi"),
        (5, 1) => return Some("Hari Buruh Internasional"),
        (6, 1) => return Some("Hari Lahir Pancasila"),
        (8, 17) => return Some("Hari Kemerdekaan RI"),
        (12, 25) => return Some("Hari Raya Natal"),
        _ => {}
    }
    if y == 2027 {
        match (m, d) {
            (2, 6) => return Some("Tahun Baru Imlek (perkiraan)"),
            (3, 9) => return Some("Hari Suci Nyepi (perkiraan)"),
            (5, 20) => return Some("Hari Raya Waisak (perkiraan)"),
            _ => {}
        }
    }
    let t = days_from_civil(y, m, d);
    let (em, ed) = easter(y);
    let e = days_from_civil(y, em, ed);
    if t == e - 2 {
        return Some("Wafat Isa Almasih");
    }
    if t == e {
        return Some("Hari Paskah");
    }
    if t == e + 39 {
        return Some("Kenaikan Isa Almasih");
    }
    let (_, hm, hd) = hijri(t);
    match (hm, hd) {
        (1, 1) => Some("Tahun Baru Islam (perkiraan)"),
        (10, 1) | (10, 2) => Some("Idul Fitri (perkiraan)"),
        (12, 10) => Some("Idul Adha (perkiraan)"),
        (3, 12) => Some("Maulid Nabi (perkiraan)"),
        (7, 27) => Some("Isra Mikraj (perkiraan)"),
        _ => None,
    }
}
