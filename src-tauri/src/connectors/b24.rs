#[allow(unused_imports)]
#[allow(unused_imports)]
use super::common::*;
use serde_json::{json, Value};
use std::time::Duration;

pub async fn b24_unread(webhook: &str) -> Value {
    let base = webhook.trim().trim_end_matches('/');
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан", "total": 0, "dialogs": []});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e, "total": 0, "dialogs": []}),
    };
    let counters: Value = match c.post(format!("{}/im.counters.get.json", base)).json(&json!({})).send().await {
        Ok(r) => r.json().await.unwrap_or(json!({})),
        Err(e) => return json!({"ok": false, "error": format!("сеть: {e}"), "total": 0, "dialogs": []}),
    };
    let total = counters
        .pointer("/result/TYPE/ALL")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let per_chat = counters
        .pointer("/result/CHAT")
        .cloned()
        .unwrap_or(json!({}));

    let recent: Value = match c
        .post(format!("{}/im.recent.get.json", base))
        .json(&json!({"SKIP_OPENLINES": "Y", "LIMIT": 15}))
        .send()
        .await
    {
        Ok(r) => r.json().await.unwrap_or(json!([])),
        Err(_) => json!([]),
    };

    let mut dialogs: Vec<Value> = Vec::new();
    if let Some(arr) = recent.get("result").and_then(|x| x.as_array()) {
        for it in arr {
            let chat_id = it.get("chat_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let cnt = per_chat
                .get(chat_id.to_string())
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if cnt <= 0 {
                continue;
            }
            let title = it.get("title").and_then(|v| v.as_str()).unwrap_or("чат");
            let msg = it.pointer("/message/text").and_then(|v| v.as_str()).unwrap_or("");
            let date = it.pointer("/message/date").and_then(|v| v.as_str()).unwrap_or("");
            dialogs.push(json!({
                "title": title,
                "text": clean_tags(msg).chars().take(120).collect::<String>(),
                "date": date,
                "count": cnt,
                "url": format!("{}/online/?IM_DIALOG={}", base.split("/rest/").next().unwrap_or(""), it.get("id").and_then(|v| v.as_str()).unwrap_or("")),
            }));
        }
    }
    dialogs.sort_by(|a, b| {
        b.get("count").and_then(|v| v.as_i64()).unwrap_or(0)
            .cmp(&a.get("count").and_then(|v| v.as_i64()).unwrap_or(0))
    });
    dialogs.truncate(6);
    json!({"ok": true, "total": total, "dialogs": dialogs, "error": Value::Null})
}

/// Создать задачу в Б24

pub async fn task_add(webhook: &str, title: &str, desc: &str, responsible: i64, deadline_ts: i64) -> Value {
    let base = webhook.trim().trim_end_matches('/');
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан"});
    }
    if title.trim().is_empty() {
        return json!({"ok": false, "error": "пустое название"});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let mut fields = json!({
        "TITLE": title.trim(),
        "DESCRIPTION": desc,
        "RESPONSIBLE_ID": responsible,
        "ACCOMPLICES": [responsible],
    });
    if deadline_ts > 0 {
        fields["DEADLINE"] = json!(deadline_ts);
    }
    let body = json!({"fields": fields, "params": {"REGISTER_SONET_EVENT": "Y"}});
    match c.post(format!("{}/tasks.task.add.json", base)).json(&body).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                if let Some(id) = v.pointer("/result/task/id").and_then(|x| x.as_str()) {
                    json!({"ok": true, "id": id})
                } else {
                    let msg = v.get("error_description").and_then(|x| x.as_str()).unwrap_or("не создалась");
                    json!({"ok": false, "error": msg})
                }
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}")}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}")}),
    }
}

/// Проверка подключения: вебхук Б24 отдаёт имя пользователя.

pub fn probe(webhook: String, user_id: i64) -> Value {
    let base = webhook.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан"});
    }
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e.to_string()}),
    };
    let body = json!({"ID": user_id, "select": ["ID", "NAME", "LAST_NAME", "EMAIL"]});
    match client.post(format!("{}/user.get.json", base)).json(&body).send() {
        Ok(r) => match r.json::<Value>() {
            Ok(v) => {
                if let Some(u) = v.get("result").and_then(|x| x.as_array()).and_then(|a| a.first()) {
                    let name = u.get("NAME").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let last = u.get("LAST_NAME").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let email = u.get("EMAIL").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    json!({"ok": true, "name": format!("{} {}", name, last).trim(), "email": email})
                } else {
                    let e = v.get("error_description").and_then(|x| x.as_str()).unwrap_or("пустой ответ");
                    json!({"ok": false, "error": e})
                }
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}")}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}")}),
    }
}

/// Закрыть задачу в Б24.
pub async fn task_done(webhook: &str, id: &str) -> Value {
    let base = webhook.trim().trim_end_matches('/');
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан"});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let body = json!({ "taskId": id });
    match c
        .post(format!("{}/tasks.task.complete.json", base))
        .json(&body)
        .send()
        .await
    {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                if v.get("result").is_some() {
                    json!({"ok": true})
                } else {
                    json!({"ok": false, "error": b24_err(&v)})
                }
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}")}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}")}),
    }
}

/// Отложить задачу: новый дедлайн через `minutes` минут.
pub async fn task_snooze(webhook: &str, id: &str, minutes: i64) -> Value {
    let base = webhook.trim().trim_end_matches('/');
    if base.is_empty() {
        return json!({"ok": false, "error": "вебхук не задан"});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e}),
    };
    let deadline = now_ts() + minutes.max(5) * 60;
    let body = json!({ "taskId": id, "fields": { "DEADLINE": deadline } });
    match c
        .post(format!("{}/tasks.task.update.json", base))
        .json(&body)
        .send()
        .await
    {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                if v.get("result").is_some() {
                    json!({"ok": true, "deadline": deadline})
                } else {
                    json!({"ok": false, "error": b24_err(&v)})
                }
            }
            Err(e) => json!({"ok": false, "error": format!("ответ: {e}")}),
        },
        Err(e) => json!({"ok": false, "error": format!("сеть: {e}")}),
    }
}

fn b24_err(v: &Value) -> String {
    v.get("error_description")
        .or_else(|| v.get("error"))
        .and_then(|x| x.as_str())
        .unwrap_or("Б24 отказал")
        .to_string()
}
