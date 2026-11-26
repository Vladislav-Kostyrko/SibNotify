use crate::config::{save_cfg, Config};
use crate::hotkey::apply_hotkey;
use crate::state::AppState;
use crate::windows::place;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) fn get_config(app: AppHandle, st: tauri::State<AppState>) -> Config {
    let cfg = st.cfg.lock().unwrap().clone();
    // диагностика: если панель пустая — смотрим, что ушло во фронт
    if let Ok(dir) = app.path().app_config_dir() {
        let diag = serde_json::json!({
            "path": dir.join("config.json").to_string_lossy(),
            "blocks": cfg.blocks.len(),
            "panel_width": cfg.panel_width,
            "font_size_seen": false,
        });
        let _ = std::fs::write(dir.join("config.debug.json"), serde_json::to_string_pretty(&diag).unwrap_or_default());
    }
    cfg
}

#[tauri::command]
pub(crate) fn save_config(app: AppHandle, mut cfg: Config) -> Result<(), String> {
    // геометрию (позиции, монитор) владеет Rust: мышь тащит окно, а не форма настроек,
    // иначе при каждом сохранении цвета панель отпрыгивала назад
    {
        let st = app.state::<AppState>();
        let cur = st.cfg.lock().unwrap();
        cfg.panel_top_pct = cur.panel_top_pct;
        cfg.strip_top_pct = cur.strip_top_pct;
        cfg.monitor_index = cur.monitor_index;
    }
    save_cfg(&app, &cfg)?;
    {
        let st = app.state::<AppState>();
        *st.cfg.lock().unwrap() = cfg.clone();
    }
    apply_hotkey(&app, &cfg.hotkey);
    place(&app);
    Ok(())
}

/// Выгрузить конфиг в файл на рабочем столе.
#[tauri::command]
pub(crate) fn export_config(app: AppHandle, st: tauri::State<AppState>) -> Result<String, String> {
    let cfg = st.cfg.lock().unwrap().clone();
    let data = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    let mut path = app
        .path()
        .desktop_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| e.to_string())?;
    path.push("sibnotify-config.json");
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Импорт: JSON из буфера/файла. Перезаписывает конфиг и применяет.
#[tauri::command]
pub(crate) fn import_config(
    app: AppHandle,
    st: tauri::State<AppState>,
    json: String,
) -> Result<(), String> {
    let mut new_cfg: Config = serde_json::from_str(&json).map_err(|e| format!("не разобрал JSON: {e}"))?;
    {
        let cur = st.cfg.lock().unwrap();
        new_cfg.monitor_index = cur.monitor_index;
        new_cfg.panel_top_pct = cur.panel_top_pct;
        new_cfg.strip_top_pct = cur.strip_top_pct;
    }
    crate::config::save_cfg(&app, &new_cfg)?;
    *st.cfg.lock().unwrap() = new_cfg.clone();
    apply_hotkey(&app, &new_cfg.hotkey);
    place(&app);
    Ok(())
}
