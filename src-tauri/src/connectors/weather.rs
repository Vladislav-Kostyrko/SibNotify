#[allow(unused_imports)]
#[allow(unused_imports)]
use super::common::*;
use serde_json::{json, Value};

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
