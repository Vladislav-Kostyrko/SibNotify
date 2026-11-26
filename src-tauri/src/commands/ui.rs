use crate::config::save_cfg;
use crate::state::AppState;
use crate::windows::{drag_kind, place, show_panel_inner};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) fn drag_start(app: AppHandle) {
    drag_kind(app, "strip");
}

#[tauri::command]
pub(crate) fn drag_panel(app: AppHandle) {
    drag_kind(app, "panel");
}

#[tauri::command]
pub(crate) async fn monitors(app: AppHandle) -> Result<Value, String> {
    let list = app.available_monitors().unwrap_or_default();
    let arr: Vec<Value> = list
        .iter()
        .enumerate()
        .map(|(i, m)| {
            json!({
                "index": i,
                "w": m.size().width,
                "h": m.size().height,
                "x": m.position().x,
                "y": m.position().y,
                "scale": m.scale_factor(),
                "name": m.name().cloned().unwrap_or_default(),
            })
        })
        .collect();
    Ok(json!(arr))
}

#[tauri::command]
pub(crate) async fn set_monitor(app: AppHandle, idx: usize) -> Result<(), String> {
    {
        let st = app.state::<AppState>();
        let mut cfg = st.cfg.lock().unwrap();
        cfg.monitor_index = idx;
        let _ = save_cfg(&app, &cfg);
    }
    place(&app);
    Ok(())
}

#[tauri::command]
pub(crate) fn show_panel(app: AppHandle) {
    show_panel_inner(&app);
}

#[tauri::command]
pub(crate) fn notify(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn toggle_side(app: AppHandle) {
    {
        let st = app.state::<AppState>();
        let mut cfg = st.cfg.lock().unwrap();
        cfg.panel_side = if cfg.panel_side == "right" { "left".to_string() } else { "right".to_string() };
        let _ = save_cfg(&app, &cfg);
    }
    place(&app);
}

#[tauri::command]
pub(crate) fn hide_panel(app: AppHandle) {
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.hide();
    }
}

#[tauri::command]
pub(crate) fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub(crate) fn open_url(url: String) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
