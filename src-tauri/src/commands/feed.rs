use crate::connectors;
use crate::state::AppState;
use tauri::Manager;
use serde_json::Value;

#[tauri::command]
pub(crate) async fn tg(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (token, proxy) = {
        let c = st.cfg.lock().unwrap();
        (c.tg_token.clone(), c.proxy.clone())
    };
    Ok(connectors::tg_data(&token, &proxy).await)
}

#[tauri::command]
pub(crate) async fn weather_now(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (lat, lon, city, proxy, src) = {
        let c = st.cfg.lock().unwrap();
        (
            c.weather_lat,
            c.weather_lon,
            c.weather_city.clone(),
            c.proxy.clone(),
            c.weather_source.clone(),
        )
    };
    Ok(connectors::weather(lat, lon, &city, &proxy, &src).await)
}

#[tauri::command]
pub(crate) async fn ai_usage(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (prov, key, ds_tok) = {
        let c = st.cfg.lock().unwrap();
        (c.ai_provider.clone(), c.ai_key.clone(), c.ds_token.clone())
    };
    let today = chrono_like_today();

    if prov.eq_ignore_ascii_case("deepseek") && !ds_tok.trim().is_empty() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let tz = 7 * 3600; // Новосибирск
        let start = now - (now + tz) % 86400; // начало суток в локальном времени
        let end = start + 86400;
        let r = connectors::ds_usage(&ds_tok, start, end, tz, &today).await;
        if r.get("ok").and_then(|x| x.as_bool()) == Some(true) {
            return Ok(r);
        }
        // сессия платформы протухла — пробуем ключ, чтобы показать хоть остаток
    }

    if key.trim().is_empty() {
        return Ok(serde_json::json!({"ok": false, "error": "ключ не указан"}));
    }
    Ok(connectors::ai_balance(&prov, &key).await)
}

fn chrono_like_today() -> String {
    // дата по UTC+7 без chrono: считаем от unix-времени
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 7 * 3600;
    let days = now / 86400;
    let mut z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    z = 0;
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[tauri::command]
pub(crate) async fn ds_login(app: tauri::AppHandle) -> Result<Value, String> {
    if let Some(w) = app.get_webview_window("dslogin") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(serde_json::json!({"ok": true, "reused": true}));
    }
    let url = tauri::Url::parse("https://platform.deepseek.com/sign_in")
        .map_err(|e| e.to_string())?;
    match tauri::WebviewWindowBuilder::new(&app, "dslogin", tauri::WebviewUrl::External(url))
        .title("Вход в DeepSeek Platform")
        .inner_size(1020.0, 820.0)
        .build()
    {
        Ok(_) => Ok(serde_json::json!({"ok": true})),
        Err(e) => Ok(serde_json::json!({"ok": false, "error": e.to_string()})),
    }
}

/// Забираем userToken из localStorage окна платформы (через заголовок окна).
#[tauri::command]
pub(crate) async fn ds_grab(app: tauri::AppHandle, st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let w = match app.get_webview_window("dslogin") {
        Some(w) => w,
        None => return Ok(serde_json::json!({"ok": false, "error": "окно входа не открыто"})),
    };
    let _ = w.eval(
        "document.title = 'DS:' + encodeURIComponent(localStorage.getItem('userToken') || '')",
    );
    std::thread::sleep(std::time::Duration::from_millis(700));
    let title = w.title().unwrap_or_default();
    let tok = title.trim().strip_prefix("DS:").unwrap_or("").to_string();
    let tok = urldecode(&tok);
    if tok.is_empty() {
        return Ok(serde_json::json!({"ok": false, "error": "не нашёл токен — войди в окне и попробуй ещё раз"}));
    }
    {
        let mut c = st.cfg.lock().unwrap();
        c.ds_token = tok.clone();
        let _ = crate::config::save_cfg(&app, &c);
    }
    let _ = w.close();
    Ok(serde_json::json!({"ok": true, "len": tok.len()}))
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
