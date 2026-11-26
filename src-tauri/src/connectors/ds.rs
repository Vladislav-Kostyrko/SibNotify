use super::common::*;
use serde_json::{json, Value};

fn auth(rb: reqwest::RequestBuilder, tok: &str) -> reqwest::RequestBuilder {
    rb.header("Authorization", format!("Bearer {}", tok))
        .header("Accept", "application/json")
        .header("x-client-platform", "web")
}

async fn get_json(cl: &reqwest::Client, url: &str, tok: &str) -> Result<Value, String> {
    let r = auth(cl.get(url), tok)
        .send()
        .await
        .map_err(|e| format!("сеть: {e}"))?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!("платформа ответила {}", status.as_u16()));
    }
    Ok(v)
}

/// Деньги, токены и траты за сегодня с платформы DeepSeek (нужен userToken веб-сессии).
pub async fn ds_usage(tok: &str, start: i64, end: i64, tz: i64, today: &str) -> Value {
    let cl = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let base = "https://platform.deepseek.com/api/v0";

    let summary = get_json(&cl, &format!("{}/users/get_user_summary", base), tok).await;
    let amount = get_json(
        &cl,
        &format!("{}/usage/by_api_key/amount?start={}&end={}&tz={}", base, start, end, tz),
        tok,
    )
    .await;
    let cost = get_json(
        &cl,
        &format!("{}/usage/by_api_key/cost?start={}&end={}&tz={}", base, start, end, tz),
        tok,
    )
    .await;

    // сырые ответы — рядом с конфигом, по ним донастраивается разбор
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = std::path::Path::new(&appdata).join("sibnotify-ds.json");
        let dump = json!({
            "summary": summary.as_ref().unwrap_or(&Value::Null),
            "amount": amount.as_ref().unwrap_or(&Value::Null),
            "cost": cost.as_ref().unwrap_or(&Value::Null),
        });
        let _ = std::fs::write(p, serde_json::to_string_pretty(&dump).unwrap_or_default());
    }

    if summary.is_err() && cost.is_err() {
        let e = summary.err().unwrap_or_default();
        return json!({"ok": false, "provider": "deepseek", "error": e});
    }

    let sum = summary.unwrap_or(Value::Null);
    let balance = find_num(&sum, &["total_balance", "balance", "available_balance"]).unwrap_or(0.0);
    let currency = find_str(&sum, &["currency"]).unwrap_or_else(|| "CNY".into());
    let cost_today = cost.as_ref().ok().and_then(|v| pick_today(v, today));
    let tokens_today = amount.as_ref().ok().and_then(|v| pick_today(v, today));

    json!({
        "ok": true,
        "provider": "deepseek-platform",
        "balance": balance,
        "currency": currency,
        "cost_today": cost_today,
        "tokens_today": tokens_today,
    })
}

fn find_num(v: &Value, keys: &[&str]) -> Option<f64> {
    match v {
        Value::Object(m) => {
            for k in keys {
                if let Some(x) = m.get(*k) {
                    if let Some(n) = x.as_f64() {
                        return Some(n);
                    }
                    if let Some(s) = x.as_str() {
                        if let Ok(n) = s.parse::<f64>() {
                            return Some(n);
                        }
                    }
                }
            }
            m.values().find_map(|x| find_num(x, keys))
        }
        Value::Array(a) => a.iter().find_map(|x| find_num(x, keys)),
        _ => None,
    }
}

fn find_str(v: &Value, keys: &[&str]) -> Option<String> {
    match v {
        Value::Object(m) => {
            for k in keys {
                if let Some(x) = m.get(*k).and_then(|x| x.as_str()) {
                    return Some(x.to_string());
                }
            }
            m.values().find_map(|x| find_str(x, keys))
        }
        Value::Array(a) => a.iter().find_map(|x| find_str(x, keys)),
        _ => None,
    }
}

/// Сумма значений за сегодняшний день по дневным бакетам.
fn pick_today(v: &Value, today: &str) -> Option<f64> {
    let mut best: Option<f64> = None;
    walk(v, today, &mut best);
    best
}

fn walk(v: &Value, today: &str, best: &mut Option<f64>) {
    match v {
        Value::Array(arr) => {
            let mut total = 0.0;
            let mut hit = false;
            for it in arr {
                if let Some(o) = it.as_object() {
                    let date = ["date", "day", "date_str", "dt", "time"]
                        .iter()
                        .find_map(|k| o.get(*k).and_then(|x| x.as_str()))
                        .unwrap_or("");
                    if date.starts_with(today) {
                        for (k, val) in o {
                            let kl = k.to_lowercase();
                            if !(kl.contains("amount")
                                || kl.contains("cost")
                                || kl.contains("total")
                                || kl.contains("token")
                                || kl.contains("value"))
                            {
                                continue;
                            }
                            let n = val
                                .as_f64()
                                .or_else(|| val.as_str().and_then(|s| s.parse::<f64>().ok()));
                            if let Some(n) = n {
                                total += n;
                                hit = true;
                            }
                        }
                    }
                }
            }
            if hit {
                *best = Some(best.map_or(total, |b| b.max(total)));
            }
            for it in arr {
                walk(it, today, best);
            }
        }
        Value::Object(m) => {
            for (_, x) in m {
                walk(x, today, best);
            }
        }
        _ => {}
    }
}
