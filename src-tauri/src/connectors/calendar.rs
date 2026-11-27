#[allow(unused_imports)]
#[allow(unused_imports)]
use super::common::*;
use serde_json::{json, Value};

pub async fn cal_data(portal: &str, webhook: &str, user_id: i64, from: i64, to: i64, tz: i64, ics: &[String]) -> Value {
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e, "events": []}),
    };
    let mut events: Vec<Value> = Vec::new();
    let horizon = to;
    let now = from;

    if !webhook.trim().is_empty() {
        let base = webhook.trim().trim_end_matches('/');
        let body = json!({
            "type": "user",
            "ownerId": user_id,
            "from": fmt_date(now + tz),
            "to": fmt_date(horizon + tz)
        });
        if let Ok(r) = c.post(format!("{}/calendar.event.get.json", base)).json(&body).send().await {
            if let Ok(v) = r.json::<Value>().await {
                if let Some(arr) = v.get("result").and_then(|x| x.as_array()) {
                    for e in arr {
                        let from_ts = e
                            .get("DATE_FROM_TS_UTC")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);
                        let name = e.get("NAME").and_then(|x| x.as_str()).unwrap_or("");
                        if name.is_empty() {
                            continue;
                        }
                        // Б24 отдаёт события шире окна — режем по фактическому интервалу
                        let to_ts = e
                            .get("DATE_TO_TS_UTC")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(from_ts + 3600);
                        if from_ts > 0 && (from_ts >= horizon || to_ts <= now) {
                            continue;
                        }
                        let df = e.get("DATE_FROM").and_then(|x| x.as_str()).unwrap_or("");
                        let meet = meet_url(&format!(
                            "{} {} {}",
                            e.get("DESCRIPTION").and_then(|x| x.as_str()).unwrap_or(""),
                            e.get("LOCATION").and_then(|x| x.as_str()).unwrap_or(""),
                            e.get("URL").and_then(|x| x.as_str()).unwrap_or("")
                        ));
                        let dt = e.get("DATE_TO").and_then(|x| x.as_str()).unwrap_or("");
                        let id = e.get("ID").and_then(|x| x.as_str()).unwrap_or("0");
                        events.push(json!({
                            "title": name,
                            "start": parse_b24_date(df),
                            "end": parse_b24_date(dt),
                            "start_hhmm": hhmm(df),
                            "end_hhmm": hhmm(dt),
                            "kind": "b24",
                            "date": df.get(..10).map(|d| {
                                let p: Vec<&str> = d.split('.').collect();
                                if p.len() == 3 { format!("{}-{}-{}", p[2], p[1], p[0]) } else { d.to_string() }
                            }).unwrap_or_default(),
                            "url": format!("{}/company/calendar/?EVENT_ID={}", portal, id),
                            "meet": meet,
                            "_sort": from_ts,
                        }));
                    }
                }
            }
        }
    }

    // .ics грузим параллельно: один медленный календарь не должен держать остальные
    let mut handles = Vec::new();
    for url in ics.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let u = if url.starts_with("webcal://") {
            url.replacen("webcal://", "https://", 1)
        } else {
            url.to_string()
        };
        let cc = c.clone();
        handles.push(tokio::spawn(async move { cc.get(&u).send().await }));
    }
    let mut skipped: Vec<String> = Vec::new();
    for h in handles {
        let txt = match h.await {
            Ok(Ok(r)) => match r.text().await {
                Ok(t) => t,
                Err(_) => continue,
            },
            _ => {
                skipped.push("календарь не ответил".to_string());
                continue;
            }
        };
        for (title, start, end, meet) in parse_ics(&txt) {
            // событие попадает в окно, только если ещё не закончилось и не началось после него
            if start >= horizon || end <= now {
                continue;
            }
            if title.trim().is_empty() {
                continue;
            }
            events.push(json!({
                "title": title,
                "start": start,
                "end": end,
                "start_hhmm": ts_hhmm(start),
                "end_hhmm": ts_hhmm(end),
                "kind": "ics",
                "date": ts_date(start),
                "url": "",
                "meet": meet,
                "_sort": start,
            }));
        }
    }

    events.sort_by(|a, b| {
        a.get("_sort").and_then(|v| v.as_i64()).unwrap_or(0)
            .cmp(&b.get("_sort").and_then(|v| v.as_i64()).unwrap_or(0))
    });
    events.truncate(40);
    json!({"ok": true, "events": events, "count": events.len(), "warn": skipped.join("; "), "error": Value::Null})
}

fn parse_ics(txt: &str) -> Vec<(String, i64, i64, String)> {
    let unfolded = txt.replace("\r\n ", "").replace("\n ", "");
    let mut out = Vec::new();
    let mut title = String::new();
    let mut start = 0i64;
    let mut end = 0i64;
    let mut meet = String::new();
    let mut in_event = false;
    for line in unfolded.lines() {
        let line = line.trim();
        if line == "BEGIN:VEVENT" {
            in_event = true;
            title.clear();
            start = 0;
            end = 0;
            meet.clear();
            continue;
        }
        if line == "END:VEVENT" {
            if in_event && start > 0 {
                if end <= start {
                    end = start + 3600;
                }
                if !title.is_empty() {
                    out.push((title.clone(), start, end, meet.clone()));
                }
            }
            in_event = false;
            continue;
        }
        if !in_event {
            continue;
        }
        let up = line.to_uppercase();
        if up.starts_with("LOCATION:")
            || up.starts_with("DESCRIPTION:")
            || up.starts_with("CONFERENCE")
            || up.starts_with("X-GOOGLE-CONFERENCE:")
            || up.starts_with("URL:")
        {
            let val = line.splitn(2, ':').nth(1).unwrap_or("");
            if meet.is_empty() {
                let u = meet_url(val);
                if !u.is_empty() {
                    meet = u;
                }
            }
        }
        if let Some(v) = line.strip_prefix("SUMMARY:") {
            title = v.replace("\\,", ",").replace("\\n", " ").to_string();
        } else if let Some(v) = line.strip_prefix("DTSTART") {
            if let Some(idx) = v.find(':') {
                start = ics_ts(&v[idx + 1..]);
            }
        } else if let Some(v) = line.strip_prefix("DTEND") {
            if let Some(idx) = v.find(':') {
                end = ics_ts(&v[idx + 1..]);
            }
        }
    }
    out
}

fn ics_ts(v: &str) -> i64 {
    let s = v.trim().trim_end_matches('Z');
    if s.len() < 8 {
        return 0;
    }
    let y: i64 = s[0..4].parse().unwrap_or(0);
    let m: i64 = s[4..6].parse().unwrap_or(1);
    let d: i64 = s[6..8].parse().unwrap_or(1);
    let (hh, mm, ss) = if s.len() >= 15 {
        (
            s[9..11].parse::<i64>().unwrap_or(0),
            s[11..13].parse::<i64>().unwrap_or(0),
            s[13..15].parse::<i64>().unwrap_or(0),
        )
    } else {
        (0, 0, 0)
    };
    // приблизительный перевод в unix (UTC) — для сортировки и окна достаточно
    days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss
}

/// Вытаскиваем ссылку на видеовстречу из текста события.
fn meet_url(text: &str) -> String {
    let hosts = [
        "telemost",
        "zoom.us",
        "meet.google.com",
        "jazz.sber",
        "salutejazz",
        "vk.com/call",
        "teams.microsoft.com/l/meetup",
        "trueconf",
        "kontur.ru/talk",
    ];
    // в ics переводы строк и разделители экранированы: \n \, \;
    let clean = text
        .replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\,", ",")
        .replace("\\;", ";");
    for tok in clean.split(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>') {
        let t = tok.trim_matches(|c: char| ",;)(.".contains(c));
        if !t.starts_with("http") {
            continue;
        }
        // обрезаем хвост, если ссылка склеилась со следующим словом
        let cut = t.find(|c: char| c == '\\' || c == '#').unwrap_or(t.len());
        let url = &t[..cut];
        let low = url.to_lowercase();
        if hosts.iter().any(|h| low.contains(h)) {
            return url.to_string();
        }
    }
    String::new()
}
