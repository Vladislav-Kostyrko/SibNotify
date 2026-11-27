#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod connectors;
mod hotkey;
mod state;
mod windows;

use commands::b24::*;
use commands::config_cmd::*;
use commands::feed::*;
use commands::mail::*;
use commands::misc::*;
use commands::mm::*;
use commands::system::*;
use commands::ui::*;
use config::{load_config, Config};
use hotkey::build_shortcut_plugin;
use state::AppState;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{Disks, Networks, System};
use tauri::{Emitter, Manager};
use windows::{place, show_panel_inner, watcher};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // второй запуск не создаёт новую копию — просто показываем панель
            show_panel_inner(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(build_shortcut_plugin())
        .setup(|app| {
            let handle = app.handle().clone();
            // сначала регистрируем состояние: окна создаются до setup, и их JS
            // может дёрнуть команды раньше, чем мы дочитаем конфиг
            app.manage(AppState {
                cfg: Mutex::new(Config::default()),
                sys: Mutex::new(System::new()),
                disks: Mutex::new(Disks::new_with_refreshed_list()),
                nets: Mutex::new(Networks::new_with_refreshed_list()),
                net_prev: Mutex::new((0, 0, Instant::now())),
                chats: Mutex::new(std::collections::HashMap::new()),
                users: Mutex::new(std::collections::HashMap::new()),
                blocked: AtomicBool::new(false),
                dragging: AtomicBool::new(false),
                last_toast: Mutex::new(serde_json::Value::Null),
            });

            let cfg = load_config(&handle);
            *handle.state::<AppState>().cfg.lock().unwrap() = cfg.clone();

            // диагностика старта: пишем в %APPDATA%\sibnotify-diag.txt независимо от tauri-путей
            {
                let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
                let p = std::path::Path::new(&appdata).join("sibnotify-diag.txt");
                let cfgp = handle
                    .path()
                    .app_config_dir()
                    .map(|d| d.join("config.json").to_string_lossy().to_string())
                    .unwrap_or_else(|e| format!("app_config_dir ERR: {e}"));
                let raw = std::fs::read_to_string(&cfgp).unwrap_or_default();
                let parsed = serde_json::from_str::<serde_json::Value>(&raw)
                    .map(|v| v.get("blocks").map(|b| b.as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0))
                    .map_err(|e| e.to_string());
                let body = format!(
                    "exe={}\nconfig_path={}\nraw_len={}\nparse={:?}\nblocks_loaded={}\ncwd={:?}\n",
                    std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                    cfgp,
                    raw.len(),
                    parsed,
                    cfg.blocks.len(),
                    std::env::current_dir().ok(),
                );
                let _ = std::fs::write(&p, body);
            }


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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            export_config,
            import_config,
            show_panel,
            hide_panel,
            drag_start,
            drag_panel,
            monitors,
            set_monitor,
            notify,
            toast,
            hide_toast,
            toast_ready,
            ai_usage,
            ds_login,
            ds_grab,
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
            task_done,
            task_snooze,
            toggle_side,
            probe,
            mail,
            open_url,
            quit_app,
            js_log,
            sound_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
