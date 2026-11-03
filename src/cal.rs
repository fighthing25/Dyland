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
