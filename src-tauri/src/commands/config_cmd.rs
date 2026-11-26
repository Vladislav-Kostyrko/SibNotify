use crate::config::{save_cfg, Config};
use crate::hotkey::apply_hotkey;
use crate::state::AppState;
use crate::windows::place;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub(crate) fn get_config(st: tauri::State<AppState>) -> Config {
    st.cfg.lock().unwrap().clone()
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
