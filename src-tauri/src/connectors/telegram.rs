#[allow(unused_imports)]
#[allow(unused_imports)]
use super::common::*;
use serde_json::{json, Value};

pub async fn tg_data(token: &str, proxy: &str) -> Value {
    if token.trim().is_empty() {
        return json!({"ok": false, "error": "токен бота не задан", "messages": []});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e, "messages": []}),
    };
    let base = svc_base("https://api.telegram.org", proxy, "tg");
    let url = format!(
        "{}/bot{}/getUpdates?limit=8&timeout=0&allowed_updates=%5B%22message%22%5D",
        base,
        token.trim()
    );
    match c.get(&url).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
                    let msg = v
                        .get("description")
                        .and_then(|x| x.as_str())
                        .unwrap_or("ошибка Telegram");
                    return json!({"ok": false, "error": msg, "messages": []});
                }
                let mut out = Vec::new();
                for u in v.get("result").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                    let m = match u.get("message") {
                        Some(m) => m,
                        None => continue,
                    };
                    let from = m
                        .pointer("/from/first_name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("?");
                    let text = m.get("text").and_then(|x| x.as_str()).unwrap_or("");
                    if text.is_empty() {
                        continue;
                    }
                    out.push(json!({
                        "from": from,
                        "text": text.chars().take(160).collect::<String>(),
                        "date": m.get("date").and_then(|x| x.as_i64()).unwrap_or(0),
                    }));
                }
                out.reverse();
                out.truncate(6);
                json!({"ok": true, "messages": out, "error": Value::Null})
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}"), "messages": []}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}"), "messages": []}),
    }
}
