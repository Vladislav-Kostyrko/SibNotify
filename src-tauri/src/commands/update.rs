use serde_json::{json, Value};
use std::time::Duration;

const REPO: &str = "Vladislav-Kostyrko/SibNotify";

fn ver(s: &str) -> (u64, u64, u64) {
    let s = s.trim().trim_start_matches('v');
    let mut it = s.split('.').map(|x| {
        x.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

#[tauri::command]
pub(crate) fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub(crate) async fn check_update() -> Result<Value, String> {
    let cl = reqwest::Client::builder()
        .user_agent("SibNotify-updater")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = cl
        .get(format!("https://api.github.com/repos/{}/releases/latest", REPO))
        .send()
        .await
        .map_err(|e| format!("сеть: {e}"))?;
    if !resp.status().is_success() {
        return Ok(json!({"ok": false, "error": format!("github ответил {}", resp.status().as_u16())}));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = v.get("tag_name").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let asset = v
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|a| {
            a.iter()
                .find(|x| {
                    x.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n.to_ascii_lowercase().ends_with(".exe"))
                        .unwrap_or(false)
                })
                .cloned()
        });
    let url = asset
        .as_ref()
        .and_then(|a| a.get("browser_download_url"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let size = asset
        .as_ref()
        .and_then(|a| a.get("size"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    let cur = env!("CARGO_PKG_VERSION").to_string();
    let newer = ver(&tag) > ver(&cur);
    Ok(json!({
        "ok": true,
        "current": cur,
        "latest": tag,
        "url": url,
        "size": size,
        "newer": newer && !url.is_empty(),
    }))
}

/// Скачиваем новую версию, подменяем exe батником и перезапускаемся.
#[tauri::command]
pub(crate) async fn apply_update(url: String) -> Result<Value, String> {
    if url.is_empty() {
        return Ok(json!({"ok": false, "error": "нет ссылки на файл"}));
    }
    let exe = std::env::current_exe().map_err(|e| format!("не нашёл exe: {e}"))?;
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let new_exe = dir.join("SibNotify.new.exe");

    let cl = reqwest::Client::builder()
        .user_agent("SibNotify-updater")
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = cl.get(&url).send().await.map_err(|e| format!("сеть: {e}"))?;
    if !resp.status().is_success() {
        return Ok(json!({"ok": false, "error": format!("файл не скачался ({})", resp.status().as_u16())}));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("скачивание: {e}"))?;
    if bytes.len() < 1_000_000 {
        return Ok(json!({"ok": false, "error": "файл подозрительно маленький"}));
    }
    std::fs::write(&new_exe, &bytes).map_err(|e| format!("запись: {e}"))?;

    let bat = std::env::temp_dir().join("sibnotify_update.bat");
    let mut script = String::new();
    script.push_str("@echo off\r\n");
    script.push_str("timeout /t 2 /nobreak >nul\r\n");
    script.push_str("taskkill /f /im SibNotify.exe >nul 2>&1\r\n");
    script.push_str("timeout /t 2 /nobreak >nul\r\n");
    script.push_str(&format!("move /y \"{}\" \"{}\" >nul\r\n", new_exe.display(), exe.display()));
    script.push_str(&format!("start \"\" \"{}\"\r\n", exe.display()));
    script.push_str(&format!("del \"{}\"\r\n", bat.display()));
    std::fs::write(&bat, script.as_bytes()).map_err(|e| format!("батник: {e}"))?;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &bat.to_string_lossy()])
        .spawn()
        .map_err(|e| format!("запуск: {e}"))?;
    Ok(json!({"ok": true, "bytes": bytes.len()}))
}
