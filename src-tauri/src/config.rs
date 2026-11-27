#[allow(unused_imports)]
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub(crate) webhook: String,
    pub(crate) user_id: i64,
    pub(crate) refresh_secs: u64,
    pub(crate) panel_width: u32,
    pub(crate) strip_width: u32,
    pub(crate) monitor_index: usize,
    pub(crate) show_system: bool,
    pub(crate) show_tasks: bool,
    pub(crate) show_comments: bool,
    pub(crate) deadline_filter: String,
    pub(crate) tasks_limit: usize,
    pub(crate) comments_per_task: usize,
    pub(crate) opacity: u8,
    pub(crate) mm_url: String,
    pub(crate) mm_token: String,
    pub(crate) ics_urls: String,
    pub(crate) tg_token: String,
    pub(crate) ai_provider: String,
    pub(crate) ai_key: String,
    pub(crate) ai_hist: serde_json::Value,
    pub(crate) ds_token: String,
    pub(crate) weather_city: String,
    pub(crate) weather_lat: f64,
    pub(crate) weather_lon: f64,
    pub(crate) show_cal: bool,
    pub(crate) show_mm: bool,
    pub(crate) show_chats: bool,
    pub(crate) show_weather: bool,
    pub(crate) sound: bool,
    pub(crate) panel_side: String,
    pub(crate) blocks: Vec<Value>,
    #[serde(default)]
    pub(crate) mail: Value,
    pub(crate) strip_height: u32,
    /// прокси для внешних сервисов (погода, Telegram); пусто — напрямую
    #[serde(default)]
    pub(crate) proxy: String,
    /// источник погоды: gismeteo (RU, без ключа) или open-meteo
    #[serde(default)]
    pub(crate) weather_source: String,
    /// высота панели, % экрана
    #[serde(default = "d_panel_h")]
    pub(crate) panel_height_pct: u32,
    /// отступ панели сверху, %
    #[serde(default = "d_panel_top")]
    pub(crate) panel_top_pct: u32,
    /// длина полоски-триггера, px
    #[serde(default = "d_strip_len")]
    pub(crate) strip_len: u32,
    /// позиция полоски сверху, %
    #[serde(default = "d_strip_top")]
    pub(crate) strip_top_pct: u32,
    /// глобальный хоткей, например "ctrl+alt+n"
    #[serde(default = "d_hotkey")]
    pub(crate) hotkey: String,
    /// показывать полоску-триггер
    #[serde(default = "d_true")]
    pub(crate) strip_enabled: bool,
    /// тост-уведомления Windows
    #[serde(default = "d_true")]
    pub(crate) toast: bool,
    /// поля внешнего вида: фронт читает их из get_config, поэтому они обязаны
    /// быть в структуре — иначе после перезапуска всё сбрасывается к дефолтам
    #[serde(default = "d_font_size")]
    pub(crate) font_size: u32,
    #[serde(default = "d_accent")]
    pub(crate) accent: String,
    #[serde(default = "d_theme")]
    pub(crate) theme: String,
    #[serde(default = "d_radius")]
    pub(crate) radius: u32,
    #[serde(default = "d_panel_radius")]
    pub(crate) panel_radius: u32,
    #[serde(default = "d_card_opacity")]
    pub(crate) card_opacity: u8,
    #[serde(default = "d_true")]
    pub(crate) block_colors: bool,
    #[serde(default = "d_sound_vol")]
    pub(crate) sound_vol: u32,
    #[serde(default = "d_true")]
    pub(crate) blur: bool,
    #[serde(default)]
    pub(crate) vip: String,
    #[serde(default)]
    pub(crate) dnd: bool,

}

pub(crate) fn d_true() -> bool { true }

pub(crate) fn d_font_size() -> u32 { 15 }

pub(crate) fn d_accent() -> String { "#4aa3ff".to_string() }

pub(crate) fn d_theme() -> String { "dark".to_string() }

pub(crate) fn d_radius() -> u32 { 13 }

pub(crate) fn d_panel_radius() -> u32 { 16 }

pub(crate) fn d_card_opacity() -> u8 { 92 }

pub(crate) fn d_sound_vol() -> u32 { 70 }

pub(crate) fn d_panel_h() -> u32 { 88 }

pub(crate) fn d_panel_top() -> u32 { 0 }

pub(crate) fn d_strip_len() -> u32 { 150 }

pub(crate) fn d_strip_top() -> u32 { 42 }

pub(crate) fn d_hotkey() -> String { "ctrl+alt+n".to_string() }

impl Default for Config {
    fn default() -> Self {
        Self {
            webhook: String::new(),
            user_id: 0,
            refresh_secs: 60,
            panel_width: 400,
            strip_width: 6,
            monitor_index: 0,
            show_system: true,
            show_tasks: true,
            show_comments: true,
            deadline_filter: "all".to_string(),
            tasks_limit: 12,
            comments_per_task: 3,
            opacity: 92,
            mm_url: "https://mm.sibtrans.ru".to_string(),
            mm_token: String::new(),
            ics_urls: String::new(),
            tg_token: String::new(),
            ai_provider: "deepseek".to_string(),
            ai_key: String::new(),
            ai_hist: json!([]),
            ds_token: String::new(),
            weather_city: "Новосибирск".to_string(),
            weather_lat: 55.03,
            weather_lon: 82.92,
            show_cal: true,
            show_mm: true,
            show_chats: true,
            show_weather: true,
            sound: true,
            panel_side: "left".to_string(),
            blocks: Vec::new(),
            mail: json!({}),
            strip_height: 100,
            proxy: String::new(),
            weather_source: "gismeteo".to_string(),
            panel_height_pct: d_panel_h(),
            panel_top_pct: d_panel_top(),
            strip_len: d_strip_len(),
            strip_top_pct: d_strip_top(),
            hotkey: d_hotkey(),
            strip_enabled: true,
            toast: true,
            font_size: d_font_size(),
            accent: d_accent(),
            theme: d_theme(),
            radius: d_radius(),
            panel_radius: d_panel_radius(),
            card_opacity: d_card_opacity(),
            block_colors: true,
            sound_vol: d_sound_vol(),
            blur: true,
            vip: String::new(),
            dnd: false,
        }
    }
}

pub(crate) fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

pub(crate) fn load_config(app: &AppHandle) -> Config {
    let p = config_path(app);
    let raw = std::fs::read_to_string(&p).unwrap_or_default();
    match serde_json::from_str::<Config>(&raw) {
        Ok(c) => c,
        Err(e) => {
            if !raw.trim().is_empty() {
                let _ = std::fs::write(config_path(app).with_extension("bad.json"), &raw);
                eprintln!("config parse error: {e}");
            }
            Config::default()
        }
    }
}

pub(crate) fn save_cfg(app: &AppHandle, cfg: &Config) -> Result<(), String> {
    let p = config_path(app);
    // берём то, что уже лежит на диске, и накладываем свои поля поверх:
    // так не теряются ключи, которых Rust не знает (font_size, accent, theme и т.п.)
    let mut base = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let mine = serde_json::to_value(cfg).map_err(|e| e.to_string())?;
    if let Some(obj) = mine.as_object() {
        for (k, v) in obj {
            base.insert(k.clone(), v.clone());
        }
    }
    let data = serde_json::to_string_pretty(&serde_json::Value::Object(base))
        .map_err(|e| e.to_string())?;
    std::fs::write(p, data).map_err(|e| e.to_string())
}

pub(crate) fn cfg_hide_in_fullscreen() -> bool {
    let p = config_dir_guess().join("config.json");
    let txt = std::fs::read_to_string(p).unwrap_or_default();
    let v: Value = serde_json::from_str(&txt).unwrap_or(json!({}));
    v.get("hide_in_fullscreen")
        .and_then(|x| x.as_bool())
        .unwrap_or(true)
}

pub(crate) fn config_dir_guess() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if cfg!(windows) {
            return PathBuf::from(appdata).join("ru.sibtrans.notify");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if cfg!(target_os = "macos") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("ru.sibtrans.notify");
        }
        let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", home));
        return PathBuf::from(base).join("ru.sibtrans.notify");
    }
    PathBuf::from(".")
}

/// хоткей нужен до старта плагина — читаем конфиг напрямую с диска

pub(crate) fn config_hotkey_direct() -> String {
    let p = config_dir_guess().join("config.json");
    let txt = std::fs::read_to_string(p).unwrap_or_default();
    let v: Value = serde_json::from_str(&txt).unwrap_or(json!({}));
    v.get("hotkey")
        .and_then(|x| x.as_str())
        .unwrap_or("ctrl+alt+n")
        .to_string()
}
