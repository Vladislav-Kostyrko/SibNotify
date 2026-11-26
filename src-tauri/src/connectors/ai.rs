use super::common::*;
use serde_json::{json, Value};

/// Остаток денег на счёте ИИ-провайдера. Универсальный ответ {ok, balance, currency, note}.

/// Достаём тело ответа и текст ошибки, чтобы 401/402 не превращались в «баланс 0».
async fn read_json(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .or_else(|| v.get("message"))
            .or_else(|| v.get("error_description"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let hint = match status.as_u16() {
            401 => "ключ не принят (401) — проверь, что скопировал целиком и он активен",
            402 => "кончились деньги или нужен платёж (402)",
            403 => "доступ запрещён (403)",
            429 => "слишком много запросов (429)",
            _ => "ошибка провайдера",
        };
        let tail = if msg.is_empty() { String::new() } else { format!(": {}", msg) };
        return Err(format!("{}{}", hint, tail));
    }
    Ok(v)
}

pub async fn ai_balance(provider: &str, key: &str) -> Value {
    let cl = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let p = provider.to_lowercase();
    let p = p.trim();

    match p {
        "deepseek" => {
            let r = cl
                .get("https://api.deepseek.com/user/balance")
                .header("Authorization", format!("Bearer {}", key))
                .send()
                .await;
            match r {
                Ok(resp) => match read_json(resp).await {
                    Ok(v) => {
                        let info = v
                            .get("balance_infos")
                            .and_then(|x| x.as_array())
                            .and_then(|a| a.first())
                            .cloned()
                            .unwrap_or(Value::Null);
                        let bal = info
                            .get("total_balance")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let cur = info
                            .get("currency")
                            .and_then(|x| x.as_str())
                            .unwrap_or("CNY");
                        json!({"ok": true, "provider": p, "balance": bal, "currency": cur, "note": ""})
                    }
                    Err(e) => json!({"ok": false, "provider": p, "error": e}),
                },
                Err(e) => json!({"ok": false, "provider": p, "error": format!("сеть: {e}")}),
            }
        }
        "openrouter" => {
            let r = cl
                .get("https://openrouter.ai/api/v1/credits")
                .header("Authorization", format!("Bearer {}", key))
                .send()
                .await;
            match r {
                Ok(resp) => match read_json(resp).await {
                    Ok(v) => {
                        let d = v.get("data").cloned().unwrap_or(Value::Null);
                        let total = d.get("total_credits").and_then(|x| x.as_f64()).unwrap_or(0.0);
                        let used = d.get("total_usage").and_then(|x| x.as_f64()).unwrap_or(0.0);
                        json!({"ok": true, "provider": p, "balance": total - used, "currency": "USD",
                               "used": used, "total": total, "note": ""})
                    }
                    Err(e) => json!({"ok": false, "provider": p, "error": e.to_string()}),
                },
                Err(e) => json!({"ok": false, "provider": p, "error": e.to_string()}),
            }
        }
        "moonshot" | "kimi" => {
            let r = cl
                .get("https://api.moonshot.cn/v1/users/me/balance")
                .header("Authorization", format!("Bearer {}", key))
                .send()
                .await;
            match r {
                Ok(resp) => match read_json(resp).await {
                    Ok(v) => {
                        let d = v.get("data").cloned().unwrap_or(Value::Null);
                        let bal = d
                            .get("available_balance")
                            .and_then(|x| x.as_f64())
                            .unwrap_or(0.0);
                        json!({"ok": true, "provider": p, "balance": bal, "currency": "CNY", "note": ""})
                    }
                    Err(e) => json!({"ok": false, "provider": p, "error": e.to_string()}),
                },
                Err(e) => json!({"ok": false, "provider": p, "error": e.to_string()}),
            }
        }
        "openai" => json!({"ok": false, "provider": p,
            "error": "OpenAI не отдаёт баланс по API — смотри platform.openai.com/usage"}),
        "anthropic" => json!({"ok": false, "provider": p,
            "error": "Anthropic: нужен admin-ключ для usage API"}),
        _ => json!({"ok": false, "error": format!("провайдер {} не поддержан", p)}),
    }
}
