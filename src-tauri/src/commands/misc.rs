use serde_json::{json, Value};

#[tauri::command]
pub(crate) async fn set_autostart(enabled: bool) -> Result<Value, String> {
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
pub(crate) async fn autostart_state() -> Result<Value, String> {
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
