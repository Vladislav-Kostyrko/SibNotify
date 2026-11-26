use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("SibNotify/0.2")
        .build()
        .map_err(|e| e.to_string())
}

/// База сервиса: напрямую или через прокси (<proxy>/<svc>, см. nginx на сервере).
fn svc_base(direct: &str, proxy: &str, svc: &str) -> String {
    let p = proxy.trim().trim_end_matches('/');
    if p.is_empty() {
        direct.to_string()
    } else {
        format!("{}/{}", p, svc)
    }
}

fn clean_tags(s: &str) -> String {
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
pub async fn mm_data(url: &str, token: &str, limit: usize) -> Value {
    let base = url.trim().trim_end_matches('/').to_string();
    if base.is_empty() || token.trim().is_empty() {
        return json!({"ok": false, "error": "ММ не настроен", "channels": [], "total": 0});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e, "channels": [], "total": 0}),
    };
    let auth = format!("Bearer {}", token.trim());

    let get_json = |path: String| {
        let c = c.clone();
        let auth = auth.clone();
        let base = base.clone();
        async move {
            match c
                .get(format!("{}{}", base, path))
                .header("Authorization", &auth)
                .send()
                .await
            {
                Ok(r) => r.json::<Value>().await.ok(),
                Err(_) => None,
            }
        }
    };

    let my_id = match get_json("/api/v4/users/me".to_string()).await {
        Some(v) => v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        None => String::new(),
    };

    let teams: Value = match get_json("/api/v4/users/me/teams".to_string()).await {
        Some(v) => v,
        None => return json!({"ok": false, "error": "ММ не ответил", "channels": [], "total": 0}),
    };
    if teams.get("message").is_some() {
        let msg = teams.get("message").and_then(|v| v.as_str()).unwrap_or("ошибка ММ");
        return json!({"ok": false, "error": msg, "channels": [], "total": 0});
    }
    let teams = teams.as_array().cloned().unwrap_or_default();
    if teams.is_empty() {
        return json!({"ok": false, "error": "нет команд в ММ", "channels": [], "total": 0});
    }

    // кандидаты: канал, непрочитанное, последний просмотр
    struct Cand {
        name: String,
        cname: String,
        cid: String,
        unread: i64,
        mentions: i64,
        last_viewed: i64,
    }
    let mut cands: Vec<Cand> = Vec::new();
    let mut mentions_total: i64 = 0;

    for t in teams.iter().take(2) {
        let tid = t.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let _tname = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if tid.is_empty() {
            continue;
        }
        let chans = match get_json(format!("/api/v4/users/me/teams/{}/channels", tid)).await {
            Some(v) => v,
            None => continue,
        };
        let members = match get_json(format!("/api/v4/users/me/teams/{}/channels/members", tid)).await {
            Some(v) => v,
            None => continue,
        };

        // cid -> (прочитано, упоминания, последний просмотр, замучен)
        let mut seen: std::collections::HashMap<String, (i64, i64, i64, bool)> =
            std::collections::HashMap::new();
        if let Some(arr) = members.as_array() {
            for m in arr {
                let cid = m.get("channel_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if cid.is_empty() {
                    continue;
                }
                let read = m.get("msg_count").and_then(|v| v.as_i64()).unwrap_or(0);
                let mt = m.get("mention_count").and_then(|v| v.as_i64()).unwrap_or(0);
                let lv = m.get("last_viewed_at").and_then(|v| v.as_i64()).unwrap_or(0);
                // MM считает канал замьюченным, если mark_unread == "mention"
                let muted = m
                    .pointer("/notify_props/mark_unread")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "mention")
                    .unwrap_or(false);
                seen.insert(cid, (read, mt, lv, muted));
            }
        }

        if let Some(arr) = chans.as_array() {
            for ch in arr {
                let cid = ch.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if cid.is_empty() {
                    continue;
                }
                let (read, mt, lv, muted) = match seen.get(&cid) {
                    Some(x) => *x,
                    None => continue,
                };
                if muted {
                    continue; // замученный канал — ничего не присылаем
                }
                let total = ch.get("total_msg_count").and_then(|v| v.as_i64()).unwrap_or(0);
                let unread = (total - read).max(0);
                if unread == 0 && mt == 0 {
                    continue;
                }
                let ctype = ch.get("type").and_then(|v| v.as_str()).unwrap_or("O");
                let cname = ch.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let api_name = ch.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let disp = ch.get("display_name").and_then(|v| v.as_str()).unwrap_or("");
                let mut name = if !disp.trim().is_empty() {
                    disp.to_string()
                } else if !api_name.trim().is_empty() {
                    api_name.to_string()
                } else {
                    "канал".to_string()
                };
                if ctype == "D" {
                    // личка: имя второго участника из "uid1__uid2"
                    if let Some(other) = api_name.split("__").find(|x| !x.is_empty() && *x != my_id) {
                        if let Some(u) = get_json(format!("/api/v4/users/{}", other)).await {
                            if let Some(un) = u.get("username").and_then(|x| x.as_str()) {
                                name = un.to_string();
                            }
                        }
                    }
                }
                cands.push(Cand {
                    name,
                    cname,
                    cid,
                    unread,
                    mentions: mt,
                    last_viewed: lv,
                });
            }
        }
    }

    cands.sort_by(|a, b| b.mentions.cmp(&a.mentions).then(b.unread.cmp(&a.unread)));
    cands.truncate(limit.max(1) * 3);

    // только НОВЫЕ сообщения: посты после last_viewed_at
    let mut out: Vec<(i64, Value)> = Vec::new();
    for cand in cands.iter() {
        mentions_total += cand.mentions;
        let path = if cand.last_viewed > 0 {
            format!(
                "/api/v4/channels/{}/posts?since={}&per_page=8",
                cand.cid, cand.last_viewed
            )
        } else {
            format!("/api/v4/channels/{}/posts?per_page=3", cand.cid)
        };
        let v = match get_json(path).await {
            Some(v) => v,
            None => continue,
        };
        let order = v.get("order").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        let posts = v.get("posts").cloned().unwrap_or(json!({}));
        let mut msgs: Vec<Value> = Vec::new();
        let mut newest: i64 = 0;
        for pid in order.iter().take(8) {
            let pid = match pid.as_str() {
                Some(s) => s,
                None => continue,
            };
            let p = match posts.get(pid) {
                Some(p) => p,
                None => continue,
            };
            let text = clean_tags(p.get("message").and_then(|x| x.as_str()).unwrap_or(""));
            if text.is_empty() {
                continue;
            }
            let ts = p.get("create_at").and_then(|x| x.as_i64()).unwrap_or(0);
            if ts > newest {
                newest = ts;
            }
            let uid = p.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
            let mut who = String::new();
            if !uid.is_empty() {
                if let Some(u) = get_json(format!("/api/v4/users/{}", uid)).await {
                    who = u
                        .get("username")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
            msgs.push(json!({
                "who": who,
                "text": text.chars().take(180).collect::<String>(),
                "at": ts_hhmm(ts / 1000),
            }));
            if msgs.len() >= 2 {
                break;
            }
        }
        if msgs.is_empty() {
            continue; // в канале новые только системные/пустые — не показываем
        }
        out.push((
            newest,
            json!({
                "name": cand.name,
                "kind": "mm",
                "unread": if cand.unread > 999 { 999 } else { cand.unread },
                "mentions": cand.mentions,
                "msgs": msgs,
                "text": "",
                "url": format!("{}/ptbr/channels/{}", base, cand.cname),
            }),
        ));
    }

    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.truncate(limit.max(1));
    let channels: Vec<Value> = out.into_iter().map(|(_, v)| v).collect();
    let total: i64 = channels
        .iter()
        .map(|v| v.get("unread").and_then(|x| x.as_i64()).unwrap_or(0))
        .sum();
    json!({"ok": true, "channels": channels, "total": total, "mentions": mentions_total, "error": Value::Null})
}

/// Календарь: события Б24 (calendar.event.get) + внешние .ics
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
        for (title, start, end) in parse_ics(&txt) {
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

fn fmt_date(ts: i64) -> String {
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

fn ts_date(ts: i64) -> String {
    // YYYY-MM-DD по Новосибирску (+7)
    let t = ts + 7 * 3600;
    let days = t / 86400;
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
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

fn parse_b24_date(s: &str) -> String {
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

fn hhmm(s: &str) -> String {
    let parts: Vec<&str> = s.split(' ').collect();
    if parts.len() == 2 {
        parts[1].chars().take(5).collect()
    } else {
        String::new()
    }
}

fn ts_hhmm(ts: i64) -> String {
    let secs = ts % 86400;
    format!("{:02}:{:02}", (secs / 3600 + 7) % 24, (secs % 3600) / 60)
}

fn parse_ics(txt: &str) -> Vec<(String, i64, i64)> {
    let unfolded = txt.replace("\r\n ", "").replace("\n ", "");
    let mut out = Vec::new();
    let mut title = String::new();
    let mut start = 0i64;
    let mut end = 0i64;
    let mut in_event = false;
    for line in unfolded.lines() {
        let line = line.trim();
        if line == "BEGIN:VEVENT" {
            in_event = true;
            title.clear();
            start = 0;
            end = 0;
            continue;
        }
        if line == "END:VEVENT" {
            if in_event && start > 0 {
                if end <= start {
                    end = start + 3600;
                }
                if !title.is_empty() {
                    out.push((title.clone(), start, end));
                }
            }
            in_event = false;
            continue;
        }
        if !in_event {
            continue;
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

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Telegram через Bot API: последние сообщения, пришедшие боту.
pub async fn tg_data(token: &str, proxy: &str) -> Value {
    if token.trim().is_empty() {
        return json!({"ok": false, "error": "токен бота не задан", "messages": []});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e, "messages": []}),
    };
    let base = svc_base("https://api.telegram.org", proxy, "tg");
    let url = format!(
        "{}/bot{}/getUpdates?limit=8&timeout=0&allowed_updates=%5B%22message%22%5D",
        base,
        token.trim()
    );
    match c.get(&url).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
                    let msg = v
                        .get("description")
                        .and_then(|x| x.as_str())
                        .unwrap_or("ошибка Telegram");
                    return json!({"ok": false, "error": msg, "messages": []});
                }
                let mut out = Vec::new();
                for u in v.get("result").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                    let m = match u.get("message") {
                        Some(m) => m,
                        None => continue,
                    };
                    let from = m
                        .pointer("/from/first_name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("?");
                    let text = m.get("text").and_then(|x| x.as_str()).unwrap_or("");
                    if text.is_empty() {
                        continue;
                    }
                    out.push(json!({
                        "from": from,
                        "text": text.chars().take(160).collect::<String>(),
                        "date": m.get("date").and_then(|x| x.as_i64()).unwrap_or(0),
                    }));
                }
                out.reverse();
                out.truncate(6);
                json!({"ok": true, "messages": out, "error": Value::Null})
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}"), "messages": []}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}"), "messages": []}),
    }
}

/// Погода: источник по настройке, с фолбэком на open-meteo.
pub async fn weather(lat: f64, lon: f64, city: &str, proxy: &str, source: &str) -> Value {
    let src = source.trim();
    if src != "open-meteo" {
        let g = weather_gis(lat, lon, city).await;
        if g.get("ok").and_then(|x| x.as_bool()) == Some(true) {
            return g;
        }
        let om = weather_om(lat, lon, city, proxy).await;
        if om.get("ok").and_then(|x| x.as_bool()) == Some(true) {
            return om;
        }
        return g;
    }
    weather_om(lat, lon, city, proxy).await
}

fn gattr<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!(" {}=\"", key);
    let i = s.find(&pat)? + pat.len();
    let j = s[i..].find('"')?;
    Some(&s[i..i + j])
}

fn gnum(s: &str, key: &str) -> Option<f64> {
    gattr(s, key).and_then(|v| v.parse::<f64>().ok())
}

fn iso_ts(s: &str) -> i64 {
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

fn gis_icon(icon: &str, descr: &str) -> &'static str {
    let d = descr.to_lowercase();
    let night = icon.starts_with("n.");
    if d.contains("гроза") {
        return "⛈";
    }
    if d.contains("снег") {
        return "❄";
    }
    if d.contains("дожд") {
        return "🌧";
    }
    if d.contains("морос") {
        return "🌦";
    }
    if d.contains("туман") || d.contains("дымк") {
        return "🌫";
    }
    if d.contains("пасмурно") {
        return "☁";
    }
    if d.contains("облачно") {
        return "⛅";
    }
    if d.contains("ясно") || d.contains("солнеч") {
        return if night { "🌙" } else { "☀" };
    }
    if night { "🌙" } else { "☁" }
}

/// Погода с gismeteo (RU, без ключа): города по координатам + прогноз.
pub async fn weather_gis(lat: f64, lon: f64, city: &str) -> Value {
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let base = "https://services.gismeteo.ru/inform-service/inf_chrome";
    let list = match c
        .get(format!("{}/cities/", base))
        .query(&[
            ("lat", format!("{:.3}", lat)),
            ("lng", format!("{:.3}", lon)),
            ("cnt", "1".to_string()),
            ("lang", "ru".to_string()),
        ])
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
    {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(e) => return json!({"ok": false, "error": format!("сеть: {e}")}),
    };
    let id = gattr(&list, "id").unwrap_or("").to_string();
    if id.is_empty() {
        return json!({"ok": false, "error": "город не найден в Гисметео"});
    }
    let xml = match c
        .get(format!("{}/forecast/", base))
        .query(&[("city", id.clone()), ("lang", "ru".to_string())])
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
    {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(e) => return json!({"ok": false, "error": format!("сеть: {e}")}),
    };
    if !xml.contains("<location") {
        return json!({"ok": false, "error": "Гисметео не отдал прогноз"});
    }
    let loc_start = xml.find("<location").unwrap_or(0);
    let loc_end = xml[loc_start..].find('>').map(|e| loc_start + e).unwrap_or(xml.len());
    let loc = &xml[loc_start..loc_end];
    let tz: i64 = gattr(loc, "tzone").and_then(|v| v.parse().ok()).unwrap_or(0);
    let city_name = gattr(loc, "name").unwrap_or(city).to_string();
    let cur_ts = iso_ts(gattr(loc, "cur_time").unwrap_or(""));
    let local_ts = if cur_ts > 0 { cur_ts + tz * 60 } else { now_ts() + tz * 60 };
    let today = ts_date(local_ts);
    let local_hour = (local_ts.rem_euclid(86400)) / 3600;

    let fact_start = xml.find("<fact").unwrap_or(0);
    let fact_end = xml[fact_start..].find("</fact>").map(|e| fact_start + e).unwrap_or(xml.len());
    let fact = &xml[fact_start..fact_end];
    let t = gnum(fact, "t");
    let feels = gnum(fact, "tflt");
    let hum = gnum(fact, "hum");
    let wind = gnum(fact, "ws");
    let icon_code = gattr(fact, "icon").unwrap_or("");
    let descr = gattr(fact, "descr").unwrap_or("");

    let mut days: Vec<Value> = Vec::new();
    let mut hourly: Vec<Value> = Vec::new();
    let mut idx = 0;
    while let Some(off) = xml[idx..].find("<day ") {
        let start = idx + off;
        let end = xml[start + 5..].find("<day ").map(|e| start + 5 + e).unwrap_or(xml.len());
        let seg = &xml[start..end];
        let date = gattr(seg, "date").unwrap_or("");
        if let (Some(tmin), Some(tmax)) = (gnum(seg, "tmin"), gnum(seg, "tmax")) {
            days.push(json!({
                "date": date,
                "min": tmin.round() as i64,
                "max": tmax.round() as i64,
                "icon": gis_icon(gattr(seg, "icon").unwrap_or(""), gattr(seg, "descr").unwrap_or("")),
            }));
        }
        if date == today && hourly.is_empty() {
            let mut j = 0;
            while let Some(o2) = seg[j..].find("<forecast valid=\"") {
                let s2 = j + o2;
                let e2 = seg[s2 + 5..].find("</forecast>").map(|x| s2 + 5 + x).unwrap_or(seg.len());
                let f = &seg[s2..e2];
                let lh = ((iso_ts(gattr(f, "valid").unwrap_or("")) + tz * 60).rem_euclid(86400)) / 3600;
                if lh >= local_hour {
                    hourly.push(json!({
                        "hour": format!("{:02}", lh),
                        "temp": gnum(f, "t").unwrap_or(0.0).round() as i64,
                        "icon": gis_icon(gattr(f, "icon").unwrap_or(""), gattr(f, "descr").unwrap_or("")),
                    }));
                }
                j = e2 + 1;
            }
            hourly.truncate(6);
        }
        idx = end;
        if idx >= xml.len() {
            break;
        }
    }
    days.truncate(4);

    let sunrise = gattr(loc, "sunrise").and_then(|v| v.parse::<i64>().ok());
    let sunset = gattr(loc, "sunset").and_then(|v| v.parse::<i64>().ok());
    let hhmm_local = |ts: i64| {
        let x = (ts + tz * 60).rem_euclid(86400);
        format!("{:02}:{:02}", x / 3600, (x % 3600) / 60)
    };

    json!({
        "ok": t.is_some(),
        "src": "gismeteo",
        "city": city_name,
        "temp": t.map(|x| x.round() as i64),
        "feels": feels.map(|x| x.round() as i64),
        "hum": hum.map(|x| x.round() as i64),
        "wind": wind.map(|x| x.round() as i64),
        "code": Value::Null,
        "icon": gis_icon(icon_code, descr),
        "descr": descr,
        "hourly": hourly,
        "days": days,
        "sunrise": sunrise.map(hhmm_local).unwrap_or_default(),
        "sunset": sunset.map(hhmm_local).unwrap_or_default(),
        "error": if t.is_some() { Value::Null } else { json!("нет текущей температуры") }
    })
}

/// Погода: open-meteo (работает вне РФ, у нас часто режется).
async fn weather_om(lat: f64, lon: f64, city: &str, proxy: &str) -> Value {
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let geo = svc_base("https://geocoding-api.open-meteo.com", proxy, "wxgeo");
    let api = svc_base("https://api.open-meteo.com", proxy, "wx");
    let (mut lat, mut lon) = (lat, lon);
    if !city.trim().is_empty() {
        if let Ok(r) = c
            .get(format!("{}/v1/search", geo))
            .query(&[("name", city.trim()), ("count", "1"), ("language", "ru")])
            .send()
            .await
        {
            if let Ok(v) = r.json::<Value>().await {
                if let Some(g) = v.get("results").and_then(|x| x.as_array()).and_then(|a| a.first()) {
                    lat = g.get("latitude").and_then(|x| x.as_f64()).unwrap_or(lat);
                    lon = g.get("longitude").and_then(|x| x.as_f64()).unwrap_or(lon);
                }
            }
        }
    }
    let url = format!(
        "{}/v1/forecast?latitude={:.3}&longitude={:.3}&current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code&hourly=temperature_2m,weather_code&daily=temperature_2m_max,temperature_2m_min,weather_code,sunrise,sunset&timezone=Asia%2FNovosibirsk&forecast_days=4",
        api, lat, lon
    );
    match c.get(&url).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let t = v.pointer("/current/temperature_2m").and_then(|x| x.as_f64());
                let w = v.pointer("/current/wind_speed_10m").and_then(|x| x.as_f64());
                let feels = v.pointer("/current/apparent_temperature").and_then(|x| x.as_f64());
                let hum = v.pointer("/current/relative_humidity_2m").and_then(|x| x.as_f64());
                let code = v.pointer("/current/weather_code").and_then(|x| x.as_i64()).unwrap_or(-1);

                let mut hourly: Vec<Value> = Vec::new();
                if let (Some(times), Some(temps), Some(codes)) = (
                    v.pointer("/hourly/time").and_then(|x| x.as_array()),
                    v.pointer("/hourly/temperature_2m").and_then(|x| x.as_array()),
                    v.pointer("/hourly/weather_code").and_then(|x| x.as_array()),
                ) {
                    // ближайшие 6 часов начиная с текущего часа
                    let now_h = {
                        let secs = now_ts() % 86400;
                        (secs / 3600 + 7) % 24
                    };
                    let today = ts_date(now_ts());
                    let mut started = false;
                    for (i, tm) in times.iter().enumerate() {
                        let ts = tm.as_str().unwrap_or("");
                        if ts.len() < 13 {
                            continue;
                        }
                        if &ts[..10] != today.as_str() {
                            continue;
                        }
                        let h: i64 = ts[11..13].parse().unwrap_or(-1);
                        if h < now_h {
                            continue;
                        }
                        started = true;
                        let temp = temps.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0).round() as i64;
                        let c2 = codes.get(i).and_then(|x| x.as_i64()).unwrap_or(-1);
                        hourly.push(json!({"hour": format!("{:02}", h), "temp": temp, "icon": weather_icon(c2)}));
                        if hourly.len() >= 6 {
                            break;
                        }
                    }
                    let _ = started;
                }

                let mut days: Vec<Value> = Vec::new();
                if let (Some(dates), Some(mx), Some(mn), Some(codes)) = (
                    v.pointer("/daily/time").and_then(|x| x.as_array()),
                    v.pointer("/daily/temperature_2m_max").and_then(|x| x.as_array()),
                    v.pointer("/daily/temperature_2m_min").and_then(|x| x.as_array()),
                    v.pointer("/daily/weather_code").and_then(|x| x.as_array()),
                ) {
                    for i in 0..dates.len().min(4) {
                        days.push(json!({
                            "date": dates.get(i).and_then(|x| x.as_str()).unwrap_or(""),
                            "max": mx.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0).round() as i64,
                            "min": mn.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0).round() as i64,
                            "icon": weather_icon(codes.get(i).and_then(|x| x.as_i64()).unwrap_or(-1)),
                        }));
                    }
                }
                let sunset = v.pointer("/daily/sunset/0").and_then(|x| x.as_str()).unwrap_or("");
                let sunrise = v.pointer("/daily/sunrise/0").and_then(|x| x.as_str()).unwrap_or("");
                json!({
                    "ok": t.is_some(),
                    "city": city,
                    "temp": t.map(|x| x.round() as i64),
                    "feels": feels.map(|x| x.round() as i64),
                    "hum": hum.map(|x| x.round() as i64),
                    "wind": w.map(|x| x.round() as i64),
                    "code": code,
                    "icon": weather_icon(code),
                    "hourly": hourly,
                    "days": days,
                    "sunrise": sunrise.chars().skip(11).take(5).collect::<String>(),
                    "sunset": sunset.chars().skip(11).take(5).collect::<String>(),
                    "error": Value::Null
                })
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}")}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}")}),
    }
}

fn weather_icon(code: i64) -> &'static str {
    match code {
        0 => "☀",
        1 | 2 => "🌤",
        3 => "☁",
        45 | 48 => "🌫",
        51 | 53 | 55 | 56 | 57 => "🌦",
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => "🌧",
        71 | 73 | 75 | 77 | 85 | 86 => "❄",
        95 | 96 | 99 => "⛈",
        _ => "·",
    }
}

/// Непрочитанные чаты Б24 (im.counters + im.recent)
pub async fn b24_unread(webhook: &str) -> Value {
    let base = webhook.trim().trim_end_matches('/');
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан", "total": 0, "dialogs": []});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e, "total": 0, "dialogs": []}),
    };
    let counters: Value = match c.post(format!("{}/im.counters.get.json", base)).json(&json!({})).send().await {
        Ok(r) => r.json().await.unwrap_or(json!({})),
        Err(e) => return json!({"ok": false, "error": format!("сеть: {e}"), "total": 0, "dialogs": []}),
    };
    let total = counters
        .pointer("/result/TYPE/ALL")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let per_chat = counters
        .pointer("/result/CHAT")
        .cloned()
        .unwrap_or(json!({}));

    let recent: Value = match c
        .post(format!("{}/im.recent.get.json", base))
        .json(&json!({"SKIP_OPENLINES": "Y", "LIMIT": 15}))
        .send()
        .await
    {
        Ok(r) => r.json().await.unwrap_or(json!([])),
        Err(_) => json!([]),
    };

    let mut dialogs: Vec<Value> = Vec::new();
    if let Some(arr) = recent.get("result").and_then(|x| x.as_array()) {
        for it in arr {
            let chat_id = it.get("chat_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let cnt = per_chat
                .get(chat_id.to_string())
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if cnt <= 0 {
                continue;
            }
            let title = it.get("title").and_then(|v| v.as_str()).unwrap_or("чат");
            let msg = it.pointer("/message/text").and_then(|v| v.as_str()).unwrap_or("");
            let date = it.pointer("/message/date").and_then(|v| v.as_str()).unwrap_or("");
            dialogs.push(json!({
                "title": title,
                "text": clean_tags(msg).chars().take(120).collect::<String>(),
                "date": date,
                "count": cnt,
                "url": format!("{}/online/?IM_DIALOG={}", base.split("/rest/").next().unwrap_or(""), it.get("id").and_then(|v| v.as_str()).unwrap_or("")),
            }));
        }
    }
    dialogs.sort_by(|a, b| {
        b.get("count").and_then(|v| v.as_i64()).unwrap_or(0)
            .cmp(&a.get("count").and_then(|v| v.as_i64()).unwrap_or(0))
    });
    dialogs.truncate(6);
    json!({"ok": true, "total": total, "dialogs": dialogs, "error": Value::Null})
}

/// Создать задачу в Б24
pub async fn task_add(webhook: &str, title: &str, desc: &str, responsible: i64, deadline_ts: i64) -> Value {
    let base = webhook.trim().trim_end_matches('/');
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан"});
    }
    if title.trim().is_empty() {
        return json!({"ok": false, "error": "пустое название"});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let mut fields = json!({
        "TITLE": title.trim(),
        "DESCRIPTION": desc,
        "RESPONSIBLE_ID": responsible,
        "ACCOMPLICES": [responsible],
    });
    if deadline_ts > 0 {
        fields["DEADLINE"] = json!(deadline_ts);
    }
    let body = json!({"fields": fields, "params": {"REGISTER_SONET_EVENT": "Y"}});
    match c.post(format!("{}/tasks.task.add.json", base)).json(&body).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                if let Some(id) = v.pointer("/result/task/id").and_then(|x| x.as_str()) {
                    json!({"ok": true, "id": id})
                } else {
                    let msg = v.get("error_description").and_then(|x| x.as_str()).unwrap_or("не создалась");
                    json!({"ok": false, "error": msg})
                }
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}")}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}")}),
    }
}

/// Проверка подключения: вебхук Б24 отдаёт имя пользователя.
pub fn probe(webhook: String, user_id: i64) -> Value {
    let base = webhook.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан"});
    }
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e.to_string()}),
    };
    let body = json!({"ID": user_id, "select": ["ID", "NAME", "LAST_NAME", "EMAIL"]});
    match client.post(format!("{}/user.get.json", base)).json(&body).send() {
        Ok(r) => match r.json::<Value>() {
            Ok(v) => {
                if let Some(u) = v.get("result").and_then(|x| x.as_array()).and_then(|a| a.first()) {
                    let name = u.get("NAME").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let last = u.get("LAST_NAME").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let email = u.get("EMAIL").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    json!({"ok": true, "name": format!("{} {}", name, last).trim(), "email": email})
                } else {
                    let e = v.get("error_description").and_then(|x| x.as_str()).unwrap_or("пустой ответ");
                    json!({"ok": false, "error": e})
                }
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}")}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}")}),
    }
}

/// Почта по IMAP (imap crate): последние письма в INBOX.
pub async fn mail_data(cfg: &Value) -> Value {
    let host = cfg.get("host").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let port = cfg.get("port").and_then(|x| x.as_u64()).unwrap_or(993);
    let user = cfg.get("user").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let pass = cfg.get("pass").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let limit = cfg.get("limit").and_then(|x| x.as_u64()).unwrap_or(6) as usize;
    let tls = cfg.get("tls").and_then(|x| x.as_bool()).unwrap_or(true);
    if host.is_empty() || user.is_empty() {
        return json!({"ok": false, "error": "почта не настроена", "messages": [], "unread": 0});
    }
    tokio::task::spawn_blocking(move || mail_blocking(&host, port, &user, &pass, tls, limit))
        .await
        .unwrap_or_else(|e| json!({"ok": false, "error": format!("{e}"), "messages": [], "unread": 0}))
}

fn mail_blocking(host: &str, port: u64, user: &str, pass: &str, tls: bool, limit: usize) -> Value {
    use native_tls::TlsConnector;
    let tlsb = match TlsConnector::builder().build() {
        Ok(t) => t,
        Err(e) => return json!({"ok": false, "error": format!("TLS: {e}"), "messages": [], "unread": 0}),
    };
    let client = if tls {
        match imap::connect((host, port as u16), host, &tlsb) {
            Ok(c) => c,
            Err(e) => return json!({"ok": false, "error": format!("соединение: {e}"), "messages": [], "unread": 0}),
        }
    } else {
        match imap::connect_starttls((host, port as u16), host, &tlsb) {
            Ok(c) => c,
            Err(e) => return json!({"ok": false, "error": format!("соединение: {e}"), "messages": [], "unread": 0}),
        }
    };
    let mut sess = match client.login(user, pass) {
        Ok(s) => s,
        Err((e, _)) => return json!({"ok": false, "error": format!("вход: {e}"), "messages": [], "unread": 0}),
    };
    let _ = sess.select("INBOX");
    let mut out: Vec<Value> = Vec::new();
    let mut unread: i64 = 0;
    if let Ok(seqs) = sess.search("UNSEEN") {
        unread = seqs.len() as i64;
    }
    if let Ok(seqs) = sess.search("ALL") {
        let mut all: Vec<u32> = seqs.into_iter().collect();
        all.sort_unstable();
        let ids: Vec<u32> = all.into_iter().rev().take(limit).collect();
        for id in ids {
            if let Ok(fetch) = sess.fetch(format!("{}", id), "(ENVELOPE INTERNALDATE FLAGS)") {
                for m in fetch.iter() {
                    let env = match m.envelope() {
                        Some(e) => e,
                        None => continue,
                    };
                    let subject = env
                        .subject
                        .as_ref()
                        .map(|s| String::from_utf8_lossy(s).to_string())
                        .unwrap_or_default();
                    let subject = decode_mime(&subject);
                    let from = env
                        .from
                        .as_ref()
                        .and_then(|f| f.first())
                        .map(|a| {
                            let n = a
                                .name
                                .as_ref()
                                .map(|x| decode_mime(&String::from_utf8_lossy(x)))
                                .unwrap_or_default();
                            if n.is_empty() {
                                a.mailbox
                                    .as_ref()
                                    .map(|x| String::from_utf8_lossy(x).to_string())
                                    .unwrap_or_default()
                            } else {
                                n
                            }
                        })
                        .unwrap_or_default();
                    let seen = m
                        .flags()
                        .iter()
                        .any(|f| matches!(f, imap::types::Flag::Seen));
                    let date = m
                        .internal_date()
                        .map(|d| d.format("%d.%m %H:%M").to_string())
                        .unwrap_or_default();
                    out.push(json!({
                        "subject": subject.chars().take(120).collect::<String>(),
                        "from": from,
                        "date": date,
                        "seen": seen,
                    }));
                }
            }
        }
    }
    let _ = sess.logout();
    json!({"ok": true, "messages": out, "unread": unread, "error": Value::Null})
}

fn q_decode(payload: &str) -> Vec<u8> {
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

fn decode_bytes(b: &[u8], charset: &str) -> String {
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

fn decode_mime(s: &str) -> String {
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
