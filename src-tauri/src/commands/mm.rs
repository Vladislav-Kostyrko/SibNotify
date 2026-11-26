use crate::connectors;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::Manager;

#[tauri::command]
pub(crate) async fn mm(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (url, token) = {
        let c = st.cfg.lock().unwrap();
        (c.mm_url.clone(), c.mm_token.clone())
    };
    Ok(connectors::mm_data(&url, &token, 6).await)
}

/// Открыть окно входа в Mattermost (SSO через GitLab и т.п.).

#[tauri::command]
pub(crate) async fn mm_login(app: tauri::AppHandle, st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let base = st.cfg.lock().unwrap().mm_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Ok(json!({"ok": false, "error": "адрес Mattermost не задан"}));
    }
    if let Some(w) = app.get_webview_window("mmlogin") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(json!({"ok": true, "reused": true}));
    }
    let parsed: tauri::Url = match base.parse() {
        Ok(u) => u,
        Err(_) => return Ok(json!({"ok": false, "error": "неверный адрес Mattermost"})),
    };
    match tauri::WebviewWindowBuilder::new(&app, "mmlogin", tauri::WebviewUrl::External(parsed))
        .title("Вход в Mattermost")
        .inner_size(1020.0, 800.0)
        .build()
    {
        Ok(_) => Ok(json!({"ok": true})),
        Err(e) => {
            // метка могла остаться от закрытого окна — снести все окна с этой меткой и попробовать ещё раз
            for (label, w) in app.webview_windows() {
                if label == "mmlogin" {
                    let _ = w.destroy();
                }
            }
            let parsed2: tauri::Url = base.parse().unwrap_or(tauri::Url::parse("about:blank").unwrap());
            match tauri::WebviewWindowBuilder::new(&app, "mmlogin", tauri::WebviewUrl::External(parsed2))
                .title("Вход в Mattermost")
                .inner_size(1020.0, 800.0)
                .build()
            {
                Ok(_) => Ok(json!({"ok": true})),
                Err(e2) => Ok(json!({"ok": false, "error": format!("{} / {}", e, e2)})),
            }
        }
    }
}

/// Забрать сессионный токен из окна входа.

#[tauri::command]
pub(crate) async fn mm_grab(app: tauri::AppHandle) -> Result<Value, String> {
    let w = match app.get_webview_window("mmlogin") {
        Some(w) => w,
        None => return Ok(json!({"ok": false, "error": "окно входа не открыто — нажми «Войти в Mattermost»"})),
    };
    let cookies = match w.cookies() {
        Ok(c) => c,
        Err(e) => return Ok(json!({"ok": false, "error": e.to_string()})),
    };
    let mut token = String::new();
    let mut uid = String::new();
    for c in cookies {
        let n = c.name().to_string();
        if n.eq_ignore_ascii_case("MMAUTHTOKEN") {
            token = c.value().to_string();
        } else if n.eq_ignore_ascii_case("MMUSERID") {
            uid = c.value().to_string();
        }
    }
    if token.is_empty() {
        return Ok(json!({"ok": false, "error": "токен не найден: сначала войди в открывшемся окне"}));
    }
    let _ = w.close();
    Ok(json!({"ok": true, "token": token, "user_id": uid}))
}
