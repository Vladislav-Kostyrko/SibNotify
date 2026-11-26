#[allow(unused_imports)]
use crate::config::config_hotkey_direct;
use crate::windows::show_panel_inner;
use tauri::AppHandle;

pub(crate) fn shortcut_from(spec: &str) -> Option<tauri_plugin_global_shortcut::Shortcut> {
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

pub(crate) fn apply_hotkey(app: &AppHandle, spec: &str) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if let Some(sc) = shortcut_from(spec) {
        let _ = gs.register(sc);
    }
}

pub(crate) fn on_shortcut(
    app: &AppHandle,
    _sc: &tauri_plugin_global_shortcut::Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
        show_panel_inner(app);
    }
}

pub(crate) fn build_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_global_shortcut::Builder;
    let spec = config_hotkey_direct();
    if let Some(sc) = shortcut_from(&spec) {
        if let Ok(b) = Builder::new().with_handler(on_shortcut).with_shortcut(sc) {
            return b.build();
        }
    }
    Builder::new().with_handler(on_shortcut).build()
}
