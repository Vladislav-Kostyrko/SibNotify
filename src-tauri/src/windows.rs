#[allow(unused_imports)]
use crate::config::{cfg_hide_in_fullscreen, config_dir_guess, save_cfg, Config};
use crate::state::AppState;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size};

pub(crate) fn place(app: &AppHandle) {
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
pub(crate) fn compute_blocked() -> bool {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState,
        QUNS_RUNNING_D3D_FULL_SCREEN,
    };

    unsafe {
        let state = match SHQueryUserNotificationState() {
            Ok(s) => s,
            Err(_) => return false,
        };
        // QUNS_BUSY (2) выставляется и обычными развёрнутыми приложениями/презентациями,
        // из-за него панель пропадала «сама». Реагируем только на настоящий D3D-фуллскрин (игры).
        state == QUNS_RUNNING_D3D_FULL_SCREEN && cfg_hide_in_fullscreen()
    }
}

#[cfg(not(windows))]
pub(crate) fn compute_blocked() -> bool {
    false
}

pub(crate) fn watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(700));
        let blocked = compute_blocked();
        let st = app.state::<AppState>();
        let was = st.blocked.swap(blocked, Ordering::SeqCst);
        if was != blocked {
            if blocked {
                if let Some(w) = app.get_webview_window("panel") {
                    let _ = w.hide();
                }
                if let Some(w) = app.get_webview_window("strip") {
                    let _ = w.hide();
                }
            } else {
                place(&app);
            }
            let _ = app.emit("blocked", blocked);
        }
        // самолечение: если не заблокировано и триггер включён, он должен быть на экране
        if !blocked {
            let strip_on = st.cfg.lock().unwrap().strip_enabled;
            if let Some(w) = app.get_webview_window("strip") {
                let vis = w.is_visible().unwrap_or(true);
                if strip_on && !vis {
                    let _ = w.show();
                } else if !strip_on && vis {
                    let _ = w.hide();
                }
            }
        }
    });
}

pub(crate) fn mon_rect(app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
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
pub(crate) fn drag_window(app: AppHandle, kind: &'static str) {
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
pub(crate) fn drag_window(app: AppHandle, kind: &'static str) {
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

pub(crate) fn st_reset_drag(app: &AppHandle) {
    let st = app.state::<AppState>();
    st.dragging.store(false, Ordering::SeqCst);
    let _ = app.emit("drag_end", ());
}

pub(crate) fn drag_kind(app: AppHandle, kind: &'static str) {
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

pub(crate) fn show_panel_inner(app: &AppHandle) {
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
