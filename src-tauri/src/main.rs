#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod connectors;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{Disks, Networks, System};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
struct Config {
    webhook: String,
    user_id: i64,
    refresh_secs: u64,
    panel_width: u32,
    strip_width: u32,
    monitor_index: usize,
    show_system: bool,
    show_tasks: bool,
    show_comments: bool,
    deadline_filter: String,
    tasks_limit: usize,
    comments_per_task: usize,
    opacity: u8,
    mm_url: String,
    mm_token: String,
    ics_urls: String,
    tg_token: String,
    weather_city: String,
    weather_lat: f64,
    weather_lon: f64,
    show_cal: bool,
    show_mm: bool,
    show_chats: bool,
    show_weather: bool,
    sound: bool,
    panel_side: String,
    blocks: Vec<Value>,
    #[serde(default)]
    mail: Value,
    strip_height: u32,
    /// прокси для внешних сервисов (погода, Telegram); пусто — напрямую
    #[serde(default)]
    proxy: String,
    /// источник погоды: gismeteo (RU, без ключа) или open-meteo
    #[serde(default)]
    weather_source: String,
    /// высота панели, % экрана
    #[serde(default = "d_panel_h")]
    panel_height_pct: u32,
    /// отступ панели сверху, %
    #[serde(default = "d_panel_top")]
    panel_top_pct: u32,
    /// длина полоски-триггера, px
    #[serde(default = "d_strip_len")]
    strip_len: u32,
    /// позиция полоски сверху, %
    #[serde(default = "d_strip_top")]
    strip_top_pct: u32,
    /// глобальный хоткей, например "ctrl+alt+n"
    #[serde(default = "d_hotkey")]
    hotkey: String,
    /// показывать полоску-триггер
    #[serde(default = "d_true")]
    strip_enabled: bool,
    /// тост-уведомления Windows
    #[serde(default = "d_true")]
    toast: bool,
}

fn d_true() -> bool { true }

fn d_panel_h() -> u32 { 88 }
fn d_panel_top() -> u32 { 0 }
fn d_strip_len() -> u32 { 150 }
fn d_strip_top() -> u32 { 42 }
fn d_hotkey() -> String { "ctrl+alt+n".to_string() }

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
        }
    }
}

struct AppState {
    cfg: Mutex<Config>,
    sys: Mutex<System>,
    disks: Mutex<Disks>,
    nets: Mutex<Networks>,
    net_prev: Mutex<(u64, u64, Instant)>,
    chats: Mutex<std::collections::HashMap<i64, i64>>,
    users: Mutex<std::collections::HashMap<i64, String>>,
    blocked: AtomicBool,
    dragging: AtomicBool,
}

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

fn load_config(app: &AppHandle) -> Config {
    let p = config_path(app);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default()
}

fn save_cfg(app: &AppHandle, cfg: &Config) -> Result<(), String> {
    let p = config_path(app);
    let data = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(p, data).map_err(|e| e.to_string())
}

fn place(app: &AppHandle) {
    let cfg = app.state::<AppState>().cfg.lock().unwrap().clone();
    let monitors = app.available_monitors().unwrap_or_default();
    if monitors.is_empty() {
        return;
    }
    let idx = cfg.monitor_index.min(monitors.len() - 1);
    let m = &monitors[idx];
    let pos = *m.position();
    let size = *m.size();
    let scale = m.scale_factor();

    let strip_w = ((cfg.strip_width.clamp(3, 40) as f64) * scale) as u32;
    let panel_w = ((cfg.panel_width.clamp(280, 900) as f64) * scale) as u32;
    let screen_h = size.height as i32;

    // панель не на весь экран: высота и отступ в процентах
    let panel_pct = cfg.panel_height_pct.clamp(20, 100);
    let panel_h = ((screen_h as f64 * panel_pct as f64 / 100.0) as u32).max(200).min(size.height);
    let panel_y = pos.y + (screen_h * cfg.panel_top_pct.min(90) as i32 / 100);

    // триггер: короткая полоска вдоль края
    let strip_h = ((cfg.strip_len.clamp(40, 2000) as f64) * scale) as u32;
    let strip_h = strip_h.min(size.height);
    let free = (screen_h - strip_h as i32).max(0);
    let strip_y = pos.y + (free * cfg.strip_top_pct.min(100) as i32 / 100);

    let right = cfg.panel_side == "right";
    let strip_x = if right { pos.x + size.width as i32 - strip_w as i32 } else { pos.x };
    let panel_x = if right { pos.x + size.width as i32 - panel_w as i32 } else { pos.x };

    let blocked_now = app.state::<AppState>().blocked.load(Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("strip") {
        if cfg.strip_enabled {
            let _ = w.set_size(Size::Physical(PhysicalSize::new(strip_w, strip_h)));
            let _ = w.set_position(Position::Physical(PhysicalPosition::new(strip_x, strip_y)));
            if !blocked_now {
                let _ = w.show();
            }
        } else {
            let _ = w.hide();
        }
    }
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.set_size(Size::Physical(PhysicalSize::new(panel_w, panel_h)));
        let _ = w.set_position(Position::Physical(PhysicalPosition::new(panel_x, panel_y)));
    }
}

#[cfg(windows)]
fn compute_blocked() -> bool {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE,
        QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

    unsafe {
        let state = match SHQueryUserNotificationState() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if state == QUNS_BUSY
            || state == QUNS_RUNNING_D3D_FULL_SCREEN
            || state == QUNS_PRESENTATION_MODE
        {
            return true;
        }
        let fg: HWND = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(fg, &mut rect).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO::default();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
    }
}

#[cfg(not(windows))]
fn compute_blocked() -> bool {
    false
}

fn watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        let blocked = compute_blocked();
        let st = app.state::<AppState>();
        let was = st.blocked.swap(blocked, Ordering::SeqCst);
        if was == blocked {
            continue;
        }
        if blocked {
            if let Some(w) = app.get_webview_window("panel") {
                let _ = w.hide();
            }
            if let Some(w) = app.get_webview_window("strip") {
                let _ = w.hide();
            }
        } else {
            place(&app);
            if let Some(w) = app.get_webview_window("strip") {
                let _ = w.show();
            }
        }
        let _ = app.emit("blocked", blocked);
    });
}

fn mon_rect(app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    let idx = {
        let st = app.state::<AppState>();
        let i = st.cfg.lock().unwrap().monitor_index;
        i
    };
    let mons = app.available_monitors().ok()?;
    let m = mons.get(idx.min(mons.len().saturating_sub(1)))?;
    let p = m.position();
    let s = m.size();
    Some((p.x, p.y, s.width as i32, s.height as i32))
}

#[cfg(windows)]
fn drag_window(app: AppHandle, kind: &'static str) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let win = match app.get_webview_window(kind) {
        Some(w) => w,
        None => {
            st_reset_drag(&app);
            return;
        }
    };
    let (mx, my, _mw, mh) = match mon_rect(&app) {
        Some(v) => v,
        None => {
            st_reset_drag(&app);
            return;
        }
    };
    let wpos = win.outer_position().unwrap_or(PhysicalPosition::new(mx, my));
    let wsize = win.outer_size().unwrap_or(PhysicalSize::new(1, 1));
    let wh = wsize.height as i32;
    let wx = wpos.x;

    let mut cur = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cur);
    }
    let grab_dy = cur.y - wpos.y;

    loop {
        std::thread::sleep(Duration::from_millis(15));
        unsafe {
            let _ = GetCursorPos(&mut cur);
        }
        let target = (cur.y - grab_dy).clamp(my, (my + mh - wh).max(my));
        let _ = win.set_position(Position::Physical(PhysicalPosition::new(wx, target)));
        let pressed = (unsafe { GetAsyncKeyState(0x01) } as u16 & 0x8000) != 0;
        if !pressed {
            break;
        }
    }

    let final_y = win.outer_position().map(|p| p.y).unwrap_or(my);
    let (new_panel_pct, new_strip_pct, new_y) = {
        let st = app.state::<AppState>();
        let mut cfg = st.cfg.lock().unwrap();
        if kind == "strip" {
            let free = (mh - wh).max(1);
            cfg.strip_top_pct = (((final_y - my) * 100) / free).clamp(0, 100) as u32;
        } else {
            let pct = (((final_y - my) as f64 / mh as f64) * 100.0).round() as i64;
            cfg.panel_top_pct = pct.clamp(0, 90) as u32;
        }
        let _ = save_cfg(&app, &cfg);
        (cfg.panel_top_pct, cfg.strip_top_pct, final_y)
    };
    let _ = new_y;
    // сообщаем фронту новые проценты, иначе он при следующем сохранении цвета вернёт старые
    let _ = app.emit(
        "geom",
        json!({"panel_top_pct": new_panel_pct, "strip_top_pct": new_strip_pct}),
    );
    place(&app);
    st_reset_drag(&app);
}

#[cfg(not(windows))]
fn drag_window(app: AppHandle, kind: &'static str) {
    // на Linux/macOS тащим средствами ОС, а потом запоминаем процент позиции
    let win = match app.get_webview_window(kind) {
        Some(w) => w,
        None => {
            st_reset_drag(&app);
            return;
        }
    };
    if win.start_dragging().is_err() {
        st_reset_drag(&app);
        return;
    }
    let mut last = win.outer_position().ok();
    std::thread::sleep(Duration::from_millis(300));
    let mut stable = 0;
    for _ in 0..400 {
        std::thread::sleep(Duration::from_millis(120));
        let cur = win.outer_position().ok();
        if cur == last {
            stable += 1;
            if stable >= 3 {
                break;
            }
        } else {
            stable = 0;
        }
        last = cur;
    }
    if let (Some(pos), Some((_mx, my, _mw, mh))) = (last, mon_rect(&app)) {
        let wh = win.outer_size().map(|s| s.height as i32).unwrap_or(0);
        let st = app.state::<AppState>();
        let mut cfg = st.cfg.lock().unwrap();
        if kind == "strip" {
            let free = (mh - wh).max(1);
            cfg.strip_top_pct = (((pos.y - my) * 100) / free).clamp(0, 100) as u32;
        } else {
            let pct = (((pos.y - my) as f64 / mh as f64) * 100.0).round() as i64;
            cfg.panel_top_pct = pct.clamp(0, 90) as u32;
        }
        let _ = save_cfg(&app, &cfg);
    }
    place(&app);
    st_reset_drag(&app);
}

fn st_reset_drag(app: &AppHandle) {
    let st = app.state::<AppState>();
    st.dragging.store(false, Ordering::SeqCst);
    let _ = app.emit("drag_end", ());
}

fn drag_kind(app: AppHandle, kind: &'static str) {
    {
        let st = app.state::<AppState>();
        if st.dragging.swap(true, Ordering::SeqCst) {
            return;
        }
    }
    let h = app.clone();
    std::thread::spawn(move || {
        #[cfg(windows)]
        drag_window(h, kind);
        #[cfg(not(windows))]
        {
            let _ = kind;
            st_reset_drag(&h);
        }
    });
}

#[tauri::command]
fn drag_start(app: AppHandle) {
    drag_kind(app, "strip");
}

#[tauri::command]
fn drag_panel(app: AppHandle) {
    drag_kind(app, "panel");
}

#[tauri::command]
async fn monitors(app: AppHandle) -> Result<Value, String> {
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
async fn set_monitor(app: AppHandle, idx: usize) -> Result<(), String> {
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
fn get_config(st: tauri::State<AppState>) -> Config {
    st.cfg.lock().unwrap().clone()
}

#[tauri::command]
fn save_config(app: AppHandle, mut cfg: Config) -> Result<(), String> {
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

fn show_panel_inner(app: &AppHandle) {
    let st = app.state::<AppState>();
    if st.blocked.load(Ordering::SeqCst) {
        return;
    }
    place(app);
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit("panel_shown", ());
    }
}

#[tauri::command]
fn show_panel(app: AppHandle) {
    show_panel_inner(&app);
}

#[tauri::command]
fn notify(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_autostart(enabled: bool) -> Result<Value, String> {
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(e) => return Ok(json!({"ok": false, "error": e.to_string()})),
    };

    #[cfg(target_os = "windows")]
    {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let out = if enabled {
            std::process::Command::new("reg")
                .args([
                    "add",
                    key,
                    "/v",
                    "SibNotify",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{}\"", exe),
                    "/f",
                ])
                .output()
        } else {
            std::process::Command::new("reg")
                .args(["delete", key, "/v", "SibNotify", "/f"])
                .output()
        };
        return match out {
            Ok(o) => Ok(json!({"ok": o.status.success()})),
            Err(e) => Ok(json!({"ok": false, "error": e.to_string()})),
        };
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = format!("{}/.config/autostart", home);
        let file = format!("{}/sibnotify.desktop", dir);
        let res = if enabled {
            std::fs::create_dir_all(&dir).and_then(|_| {
                std::fs::write(
                    &file,
                    format!(
                        "[Desktop Entry]\nType=Application\nName=SibNotify\nComment=Sibtrans notification panel\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                        exe
                    ),
                )
            })
        } else {
            match std::fs::remove_file(&file) {
                Ok(_) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        };
        return match res {
            Ok(_) => Ok(json!({"ok": true})),
            Err(e) => Ok(json!({"ok": false, "error": e.to_string()})),
        };
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = format!("{}/Library/LaunchAgents", home);
        let file = format!("{}/ru.sibtrans.notify.plist", dir);
        let res = if enabled {
            std::fs::create_dir_all(&dir).and_then(|_| {
                std::fs::write(
                    &file,
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n  <key>Label</key><string>ru.sibtrans.notify</string>\n  <key>ProgramArguments</key><array><string>{}</string></array>\n  <key>RunAtLoad</key><true/>\n</dict></plist>\n",
                        exe
                    ),
                )
            })
        } else {
            match std::fs::remove_file(&file) {
                Ok(_) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        };
        return match res {
            Ok(_) => Ok(json!({"ok": true})),
            Err(e) => Ok(json!({"ok": false, "error": e.to_string()})),
        };
    }

    #[allow(unreachable_code)]
    Ok(json!({"ok": false, "error": "не поддерживается на этой ОС"}))
}

#[tauri::command]
async fn autostart_state() -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let out = std::process::Command::new("reg")
            .args(["query", key, "/v", "SibNotify"])
            .output();
        return Ok(json!({"on": out.map(|o| o.status.success()).unwrap_or(false)}));
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let p = format!("{}/.config/autostart/sibnotify.desktop", home);
        return Ok(json!({"on": std::path::Path::new(&p).exists()}));
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let p = format!("{}/Library/LaunchAgents/ru.sibtrans.notify.plist", home);
        return Ok(json!({"on": std::path::Path::new(&p).exists()}));
    }
    #[allow(unreachable_code)]
    Ok(json!({"on": false}))
}

#[tauri::command]
fn toggle_side(app: AppHandle) {
    {
        let st = app.state::<AppState>();
        let mut cfg = st.cfg.lock().unwrap();
        cfg.panel_side = if cfg.panel_side == "right" { "left".to_string() } else { "right".to_string() };
        let _ = save_cfg(&app, &cfg);
    }
    place(&app);
}

#[tauri::command]
async fn mail(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let m = st.cfg.lock().unwrap().mail.clone();
    Ok(connectors::mail_data(&m).await)
}

#[tauri::command]
fn probe(webhook: String, user_id: i64) -> Value {
    connectors::probe(webhook, user_id)
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn open_url(url: String) {
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

fn sys_procs_snapshot() -> Vec<(String, u64)> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut out: Vec<(String, u64)> = Vec::new();
    for (_, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_string();
        if name.is_empty() {
            continue;
        }
        out.push((name, p.memory()));
    }
    out
}

#[cfg(windows)]
fn battery_pct() -> Option<i64> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    unsafe {
        let mut st = SYSTEM_POWER_STATUS::default();
        if GetSystemPowerStatus(&mut st).is_ok() && st.BatteryLifePercent != 255 {
            return Some(st.BatteryLifePercent as i64);
        }
    }
    None
}

#[cfg(not(windows))]
fn battery_pct() -> Option<i64> {
    None
}

#[tauri::command]
fn sys_stats(st: tauri::State<AppState>) -> Value {
    let mut sys = st.sys.lock().unwrap();
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu = sys.global_cpu_usage();
    let total = sys.total_memory();
    let used = sys.used_memory();
    let procs = sys.processes().len();
    drop(sys);

    let mut disks = st.disks.lock().unwrap();
    disks.refresh(true);
    let mut disk = json!(null);
    for d in disks.iter() {
        let mp = d.mount_point().to_string_lossy().to_string();
        if mp.starts_with("C") || mp == "/" {
            disk = json!({
                "mount": mp,
                "free_gb": (d.available_space() as f64) / 1024.0 / 1024.0 / 1024.0,
                "total_gb": (d.total_space() as f64) / 1024.0 / 1024.0 / 1024.0,
            });
            break;
        }
    }
    drop(disks);

    let mut nets = st.nets.lock().unwrap();
    nets.refresh(true);
    let mut rx: u64 = 0;
    let mut tx: u64 = 0;
    for (_, n) in nets.iter() {
        rx += n.total_received();
        tx += n.total_transmitted();
    }
    drop(nets);
    let mut prev = st.net_prev.lock().unwrap();
    let now = Instant::now();
    let dt = now.duration_since(prev.2).as_secs_f64().max(0.001);
    let d_rx = (rx.saturating_sub(prev.0) as f64 / dt) as u64;
    let d_tx = (tx.saturating_sub(prev.1) as f64 / dt) as u64;
    *prev = (rx, tx, now);

    let mut procs_all: Vec<(String, u64)> = sys_procs_snapshot();
    procs_all.sort_by(|a, b| b.1.cmp(&a.1));
    let top: Vec<Value> = procs_all
        .iter()
        .take(3)
        .map(|(n, m)| json!({"name": n, "mb": (*m as f64 / 1048576.0).round() as i64}))
        .collect();

    json!({
        "top": top,
        "battery": battery_pct(),
        "cpu": cpu.round() as i64,
        "ram_used_gb": (used as f64 / 1073741824.0 * 10.0).round() / 10.0,
        "ram_total_gb": (total as f64 / 1073741824.0 * 10.0).round() / 10.0,
        "ram_pct": if total > 0 { (used * 100 / total) as i64 } else { 0 },
        "disk": disk,
        "net_down": d_rx,
        "net_up": d_tx,
        "procs": procs,
        "uptime_h": System::uptime() / 3600,
    })
}

fn portal_base(webhook: &str) -> Option<String> {
    let u = reqwest::Url::parse(webhook).ok()?;
    Some(format!("{}://{}", u.scheme(), u.host_str()?))
}

#[tauri::command]
async fn b24_data(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let cfg = st.cfg.lock().unwrap().clone();
    if cfg.webhook.trim().is_empty() {
        return Ok(json!({"ok": false, "error": "вебхук не задан — открой настройки", "tasks": [], "comments": []}));
    }
    let base = cfg.webhook.trim().trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let mut filter = json!({
        "RESPONSIBLE_ID": cfg.user_id,
        "REAL_STATUS": [2, 3, 4, 6]
    });
    if cfg.deadline_filter == "today" || cfg.deadline_filter == "week" {
        let days = if cfg.deadline_filter == "today" { 1 } else { 7 };
        let now = chrono_now();
        filter["<=DEADLINE"] = json!(now + days * 86400);
        filter[">=DEADLINE"] = json!(now - 86400);
    }

    let body = json!({
        "filter": filter,
        "select": ["ID", "TITLE", "DEADLINE", "STATUS", "PRIORITY", "GROUP_ID"],
        "order": {"DEADLINE": "asc"},
        "start": 0
    });

    let resp = client
        .post(format!("{}/tasks.task.list.json", base))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("сеть: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("ответ: {e}"))?;

    if resp.get("error").is_some() {
        let msg = resp
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or("ошибка REST")
            .to_string();
        return Ok(json!({"ok": false, "error": msg, "tasks": [], "comments": []}));
    }

    let portal = portal_base(&base).unwrap_or_default();
    let raw = resp
        .pointer("/result/tasks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tasks: Vec<Value> = Vec::new();
    let mut ids: Vec<i64> = Vec::new();
    for t in raw.iter().take(cfg.tasks_limit) {
        let id = t
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| t.get("id").and_then(|v| v.as_i64()))
            .unwrap_or(0);
        if id == 0 {
            continue;
        }
        ids.push(id);
        let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
        tasks.push(json!({
            "id": id,
            "title": t.get("title").and_then(|v| v.as_str()).unwrap_or(""),
            "deadline": t.get("deadline").and_then(|v| v.as_str()).unwrap_or(""),
            "status": status,
            "priority": t.get("priority").and_then(|v| v.as_str()).unwrap_or(""),
            "group_id": t.get("groupId").and_then(|v| v.as_str()).unwrap_or("0"),
            "url": format!("{}/company/tasks/task/{}/", portal, id),
        }));
    }

    // комментарии = чат задачи (im.chat.get по ENTITY_TYPE=TASKS_TASK)
    let mut comments: Vec<Value> = Vec::new();
    let mut authors: Vec<i64> = Vec::new();

    for id in ids.iter().take(10) {
        let cached = {
            let g = st.chats.lock().unwrap();
            g.get(id).copied()
        };
        let chat = match cached {
            Some(c) => Some(c),
            None => {
                let r = client
                    .post(format!("{}/im.chat.get.json", base))
                    .json(&json!({"ENTITY_TYPE": "TASKS_TASK", "ENTITY_ID": id}))
                    .send()
                    .await;
                let cid = match r {
                    Ok(resp) => resp
                        .json::<Value>()
                        .await
                        .ok()
                        .and_then(|v| v.pointer("/result/ID").and_then(|x| x.as_i64())),
                    Err(_) => None,
                };
                if let Some(c) = cid {
                    st.chats.lock().unwrap().insert(*id, c);
                }
                cid
            }
        };
        let chat = match chat {
            Some(c) => c,
            None => continue,
        };
        let mres = client
            .post(format!("{}/im.dialog.messages.get.json", base))
            .json(&json!({"DIALOG_ID": format!("chat{}", chat), "LIMIT": 15}))
            .send()
            .await;
        let msgs = match mres {
            Ok(r) => r
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v.pointer("/result/messages").and_then(|x| x.as_array()).cloned())
                .unwrap_or_default(),
            Err(_) => continue,
        };
        let mut per_task = 0;
        for m in msgs.iter() {
            if per_task >= cfg.comments_per_task {
                break;
            }
            let author = m.get("author_id").and_then(|x| x.as_i64()).unwrap_or(0);
            if author == 0 || author == cfg.user_id {
                continue;
            }
            let text = clean_bb(m.get("text").and_then(|x| x.as_str()).unwrap_or(""));
            if text.is_empty() {
                continue;
            }
            if !authors.contains(&author) {
                authors.push(author);
            }
            per_task += 1;
            comments.push(json!({
                "task_id": id,
                "author_id": author,
                "date": m.get("date").and_then(|x| x.as_str()).unwrap_or(""),
                "text": text,
                "task_url": format!("{}/company/tasks/task/{}/", portal, id),
            }));
        }
    }

    // имена авторов (кэш + один user.get)
    let mut names: std::collections::HashMap<i64, String> = {
        let g = st.users.lock().unwrap();
        g.iter().map(|(k, v)| (*k, v.clone())).collect()
    };
    let unknown: Vec<i64> = authors.iter().copied().filter(|a| !names.contains_key(a)).collect();
    if !unknown.is_empty() {
        let r = client
            .post(format!("{}/user.get.json", base))
            .json(&json!({"filter": {"ID": unknown}}))
            .send()
            .await;
        if let Ok(resp) = r {
            if let Ok(v) = resp.json::<Value>().await {
                if let Some(arr) = v.get("result").and_then(|x| x.as_array()) {
                    for u in arr.iter() {
                        let uid = u
                            .get("ID")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<i64>().ok())
                            .or_else(|| u.get("ID").and_then(|x| x.as_i64()))
                            .unwrap_or(0);
                        if uid == 0 {
                            continue;
                        }
                        let name = format!(
                            "{} {}",
                            u.get("NAME").and_then(|x| x.as_str()).unwrap_or(""),
                            u.get("LAST_NAME").and_then(|x| x.as_str()).unwrap_or("")
                        )
                        .trim()
                        .to_string();
                        if !name.is_empty() {
                            names.insert(uid, name.clone());
                            st.users.lock().unwrap().insert(uid, name);
                        }
                    }
                }
            }
        }
    }
    for c in comments.iter_mut() {
        let aid = c.get("author_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let name = names.get(&aid).cloned().unwrap_or_else(|| format!("#{}", aid));
        c["author"] = json!(name);
    }

    comments.sort_by(|a, b| {
        let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
        db.cmp(da)
    });
    comments.truncate(30);

    Ok(json!({"ok": true, "tasks": tasks, "comments": comments, "error": Value::Null}))
}

fn clean_bb(s: &str) -> String {
    let mut out = String::new();
    let mut depth_user = 0;
    let mut chars = s.chars().peekable();
    let mut buf = String::new();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut tag = String::new();
            for n in chars.by_ref() {
                if n == ']' {
                    break;
                }
                tag.push(n);
            }
            let low = tag.to_lowercase();
            if low.starts_with("user=") {
                depth_user += 1;
                continue;
            }
            if low == "/user" {
                depth_user = (depth_user - 1).max(0);
                continue;
            }
            if low.starts_with("url") || low.starts_with("/url") {
                continue;
            }
            if low == "br" {
                buf.push(' ');
                continue;
            }
            if ["p", "/p", "b", "/b", "i", "/i", "u", "/u", "*", "list", "/list"].contains(&low.as_str()) {
                buf.push(' ');
                continue;
            }
            buf.push(' ');
            continue;
        }
        if c == '\n' || c == '\r' || c == '\t' {
            buf.push(' ');
            continue;
        }
        buf.push(c);
    }
    out.push_str(&buf);
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let _ = depth_user;
    collapsed
}

#[tauri::command]
async fn mm(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (url, token) = {
        let c = st.cfg.lock().unwrap();
        (c.mm_url.clone(), c.mm_token.clone())
    };
    Ok(connectors::mm_data(&url, &token, 6).await)
}

/// Открыть окно входа в Mattermost (SSO через GitLab и т.п.).
#[tauri::command]
async fn mm_login(app: tauri::AppHandle, st: tauri::State<'_, AppState>) -> Result<Value, String> {
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
async fn mm_grab(app: tauri::AppHandle) -> Result<Value, String> {
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

#[tauri::command]
async fn cal(
    st: tauri::State<'_, AppState>,
    src: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    ics_one: Option<String>,
    tz: Option<i64>,
) -> Result<Value, String> {
    let cfg = st.cfg.lock().unwrap().clone();
    let src = src.unwrap_or_else(|| "both".to_string());
    let now = chrono_now();
    let from = from.unwrap_or(now - 3600);
    let to = to.unwrap_or(now + 7 * 86400).max(from + 3600);
    let tz = tz.unwrap_or(7 * 3600);
    let base = cfg.webhook.trim().trim_end_matches('/').to_string();
    let portal = portal_base(&base).unwrap_or_default();
    let mut ics: Vec<String> = Vec::new();
    if src != "b24" {
        if let Some(one) = ics_one {
            if !one.trim().is_empty() {
                ics.push(one.trim().to_string());
            }
        }
        for s in cfg.ics_urls.lines().map(|s| s.trim()) {
            if !s.is_empty() && !ics.iter().any(|x| x == s) {
                ics.push(s.to_string());
            }
        }
    }
    let b24_hook = if src == "ics" { String::new() } else { base };
    Ok(connectors::cal_data(&portal, &b24_hook, cfg.user_id, from, to, tz, &ics).await)
}

#[tauri::command]
async fn tg(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (token, proxy) = {
        let c = st.cfg.lock().unwrap();
        (c.tg_token.clone(), c.proxy.clone())
    };
    Ok(connectors::tg_data(&token, &proxy).await)
}

#[tauri::command]
async fn weather_now(st: tauri::State<'_, AppState>) -> Result<Value, String> {
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
async fn chats(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let hook = st.cfg.lock().unwrap().webhook.clone();
    Ok(connectors::b24_unread(&hook).await)
}

#[tauri::command]
async fn add_task(
    st: tauri::State<'_, AppState>,
    title: String,
    description: String,
    deadline: i64,
) -> Result<Value, String> {
    let (hook, uid) = {
        let c = st.cfg.lock().unwrap();
        (c.webhook.clone(), c.user_id)
    };
    Ok(connectors::task_add(&hook, &title, &description, uid, deadline).await)
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn strip_bb(s: &str) -> String {
    let mut out = s.to_string();
    for tag in ["[p]", "[/p]", "[br]", "[b]", "[/b]", "[i]", "[/i]", "[u]", "[/u]", "[list]", "[/list]", "[*]"] {
        out = out.replace(tag, " ");
    }
    while let Some(a) = out.find("[url=") {
        if let Some(b) = out[a..].find(']') {
            let end = a + b + 1;
            out.replace_range(a..end, "");
        } else {
            break;
        }
    }
    out.replace("[/url]", "").trim().to_string()
}

fn shortcut_from(spec: &str) -> Option<tauri_plugin_global_shortcut::Shortcut> {
    let mut parts: Vec<String> = Vec::new();
    for p in spec.split('+').map(|x| x.trim().to_lowercase()) {
        let v = match p.as_str() {
            "ctrl" | "control" => "Control".to_string(),
            "alt" | "option" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "win" | "super" | "meta" | "cmd" | "command" => "Super".to_string(),
            "" => continue,
            other => other.to_uppercase(),
        };
        parts.push(v);
    }
    if parts.is_empty() {
        return None;
    }
    parts.join("+").parse().ok()
}

fn apply_hotkey(app: &AppHandle, spec: &str) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if let Some(sc) = shortcut_from(spec) {
        let _ = gs.register(sc);
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = load_config(&handle);
            app.manage(AppState {
                cfg: Mutex::new(cfg),
                sys: Mutex::new(System::new()),
                disks: Mutex::new(Disks::new_with_refreshed_list()),
                nets: Mutex::new(Networks::new_with_refreshed_list()),
                net_prev: Mutex::new((0, 0, Instant::now())),
                chats: Mutex::new(std::collections::HashMap::new()),
                users: Mutex::new(std::collections::HashMap::new()),
                blocked: AtomicBool::new(false),
                dragging: AtomicBool::new(false),
            });

            place(&handle);

            // WebView2 по умолчанию рисует непрозрачный фон — из-за него в углу торчит «ровный» квадрат
            for label in ["panel", "strip"] {
                if let Some(w) = handle.get_webview_window(label) {
                    let _ = w.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
                }
            }

            {
                let st = handle.state::<AppState>();
                let mut s = st.sys.lock().unwrap();
                s.refresh_all();
            }

            if let Some(w) = handle.get_webview_window("strip") {
                let _ = w.show();
            }

            watcher(app.handle().clone());

            // иконка в трее с меню
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
                let show_i = MenuItem::with_id(app, "show", "Показать панель", true, None::<&str>)?;
                let refresh_i = MenuItem::with_id(app, "refresh", "Обновить", true, None::<&str>)?;
                let quit_i = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_i, &refresh_i, &quit_i])?;
                let mut builder = TrayIconBuilder::with_id("main")
                    .tooltip("SibNotify")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, ev| match ev.id().as_ref() {
                        "show" => show_panel_inner(app),
                        "refresh" => {
                            let _ = app.emit("refresh", ());
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_panel_inner(tray.app_handle());
                        }
                    });
                if let Some(icon) = app.default_window_icon() {
                    builder = builder.icon(icon.clone());
                }
                let _ = builder.build(app);
            }

            let hk = app
                .state::<AppState>()
                .cfg
                .lock()
                .unwrap()
                .hotkey
                .clone();
            apply_hotkey(app.handle(), &hk);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            show_panel,
            hide_panel,
            drag_start,
            drag_panel,
            monitors,
            set_monitor,
            notify,
            set_autostart,
            autostart_state,
            sys_stats,
            b24_data,
            mm,
            mm_login,
            mm_grab,
            cal,
            tg,
            weather_now,
            chats,
            add_task,
            toggle_side,
            probe,
            mail,
            open_url,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
