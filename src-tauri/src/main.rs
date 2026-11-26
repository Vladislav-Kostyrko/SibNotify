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
use config::load_config;
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
