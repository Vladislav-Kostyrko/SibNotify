#[allow(unused_imports)]
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("SibNotify/0.2")
        .build()
        .map_err(|e| e.to_string())
}

/// База сервиса: напрямую или через прокси (<proxy>/<svc>, см. nginx на сервере).

pub(crate) fn svc_base(direct: &str, proxy: &str, svc: &str) -> String {
    let p = proxy.trim().trim_end_matches('/');
    if p.is_empty() {
        direct.to_string()
    } else {
        format!("{}/{}", p, svc)
    }
}

pub(crate) fn clean_tags(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '[' {
            let mut tag = String::new();
            for n in it.by_ref() {
                if n == ']' {
                    break;
                }
                tag.push(n);
            }
            out.push(' ');
        } else if c == '*' || c == '`' || c == '_' {
            continue;
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Mattermost: непрочитанные каналы + упоминания.

pub(crate) fn fmt_date(ts: i64) -> String {
    // YYYY-MM-DD в локальном поясе сервера Б24 неважно — API принимает дату
    let days = ts / 86400;
    let mut y = 1970;
    let mut d = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    while m < 12 && d >= months[m] {
        d -= months[m];
        m += 1;
    }
    format!("{:04}-{:02}-{:02}", y, m + 1, d + 1)
}

pub(crate) fn ts_date(ts: i64) -> String {
    // YYYY-MM-DD по Новосибирску (+7)
    let t = ts + 7 * 3600;
    let days = t / 86400;
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(crate) fn parse_b24_date(s: &str) -> String {
    // "26.11.2026 10:00:00" -> "2026-11-26T10:00:00"
    let parts: Vec<&str> = s.split(' ').collect();
    if parts.len() == 2 {
        let d: Vec<&str> = parts[0].split('.').collect();
        if d.len() == 3 {
            return format!("{}-{}-{}T{}", d[2], d[1], d[0], parts[1]);
        }
    }
    s.to_string()
}

pub(crate) fn hhmm(s: &str) -> String {
    let parts: Vec<&str> = s.split(' ').collect();
    if parts.len() == 2 {
        parts[1].chars().take(5).collect()
    } else {
        String::new()
    }
}

pub(crate) fn ts_hhmm(ts: i64) -> String {
    let secs = ts % 86400;
    format!("{:02}:{:02}", (secs / 3600 + 7) % 24, (secs % 3600) / 60)
}

pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Telegram через Bot API: последние сообщения, пришедшие боту.

pub(crate) fn gattr<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!(" {}=\"", key);
    let i = s.find(&pat)? + pat.len();
    let j = s[i..].find('"')?;
    Some(&s[i..i + j])
}

pub(crate) fn gnum(s: &str, key: &str) -> Option<f64> {
    gattr(s, key).and_then(|v| v.parse::<f64>().ok())
}

pub(crate) fn iso_ts(s: &str) -> i64 {
    if s.len() < 19 {
        return 0;
    }
    let y: i64 = s[0..4].parse().unwrap_or(1970);
    let mo: i64 = s[5..7].parse().unwrap_or(1);
    let d: i64 = s[8..10].parse().unwrap_or(1);
    let h: i64 = s[11..13].parse().unwrap_or(0);
    let mi: i64 = s[14..16].parse().unwrap_or(0);
    let se: i64 = s[17..19].parse().unwrap_or(0);
    days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se
}

pub(crate) fn q_decode(payload: &str) -> Vec<u8> {
    let b = payload.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

pub(crate) fn decode_bytes(b: &[u8], charset: &str) -> String {
    let cs = charset.to_lowercase();
    if cs.contains("1251") {
        let (res, _, _) = encoding_rs::WINDOWS_1251.decode(b);
        res.to_string()
    } else if cs.contains("koi8") {
        let (res, _, _) = encoding_rs::KOI8_R.decode(b);
        res.to_string()
    } else {
        String::from_utf8_lossy(b).to_string()
    }
}

pub(crate) fn decode_mime(s: &str) -> String {
    // =?utf-8?B?...?= / =?windows-1251?Q?...?=
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let mut out = String::new();
    let mut rest = s;
    while let Some(pos) = rest.find("=?") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let mut it = after.splitn(3, '?');
        let charset = it.next().unwrap_or("").to_string();
        let enc = it.next().unwrap_or("").to_string();
        let tail = it.next().unwrap_or("");
        match tail.find("?=") {
            Some(e) => {
                let payload = &tail[..e];
                let bytes = match enc.to_uppercase().as_str() {
                    "B" => STANDARD.decode(payload.trim()).ok(),
                    "Q" => Some(q_decode(payload)),
                    _ => None,
                };
                match bytes {
                    Some(b) => out.push_str(&decode_bytes(&b, &charset)),
                    None => out.push_str(&rest[pos..std::cmp::min(rest.len(), pos + 3)]),
                }
                rest = &tail[e + 2..];
            }
            None => {
                out.push_str(&rest[pos..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    let low = out.to_lowercase();
    if low.contains("=d0") || low.contains("=d1") || low.contains("=d2") || low.contains("=c3") {
        // тема пришла голым quoted-printable без =?...?=
        out = decode_bytes(&q_decode(&out), "utf-8");
    }
    out
}
