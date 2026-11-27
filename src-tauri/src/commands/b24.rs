use crate::connectors;
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

#[tauri::command]
pub(crate) fn probe(webhook: String, user_id: i64) -> Value {
    connectors::probe(webhook, user_id)
}

pub(crate) fn portal_base(webhook: &str) -> Option<String> {
    let u = reqwest::Url::parse(webhook).ok()?;
    Some(format!("{}://{}", u.scheme(), u.host_str()?))
}

#[tauri::command]
pub(crate) async fn b24_data(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let cfg = st.cfg.lock().unwrap().clone();
    if cfg.webhook.trim().is_empty() {
        return Ok(json!({"ok": false, "error": "вебхук не задан — открой настройки", "tasks": [], "comments": []}));
    }
    let base = cfg.webhook.trim().trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let mut filter = json!({
        "RESPONSIBLE_ID": cfg.user_id,
        "REAL_STATUS": [2, 3, 4, 6]
    });
    if cfg.deadline_filter == "today" || cfg.deadline_filter == "week" {
        let days = if cfg.deadline_filter == "today" { 1 } else { 7 };
        let now = chrono_now();
        filter["<=DEADLINE"] = json!(now + days * 86400);
        filter[">=DEADLINE"] = json!(now - 86400);
    }

    let body = json!({
        "filter": filter,
        "select": ["ID", "TITLE", "DEADLINE", "STATUS", "PRIORITY", "GROUP_ID"],
        "order": {"DEADLINE": "asc"},
        "start": 0
    });

    let resp = client
        .post(format!("{}/tasks.task.list.json", base))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("сеть: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("ответ: {e}"))?;

    if resp.get("error").is_some() {
        let msg = resp
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or("ошибка REST")
            .to_string();
        return Ok(json!({"ok": false, "error": msg, "tasks": [], "comments": []}));
    }

    let portal = portal_base(&base).unwrap_or_default();
    let raw = resp
        .pointer("/result/tasks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tasks: Vec<Value> = Vec::new();
    let mut ids: Vec<i64> = Vec::new();
    for t in raw.iter().take(cfg.tasks_limit) {
        let id = t
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| t.get("id").and_then(|v| v.as_i64()))
            .unwrap_or(0);
        if id == 0 {
            continue;
        }
        ids.push(id);
        let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
        tasks.push(json!({
            "id": id,
            "title": t.get("title").and_then(|v| v.as_str()).unwrap_or(""),
            "deadline": t.get("deadline").and_then(|v| v.as_str()).unwrap_or(""),
            "status": status,
            "priority": t.get("priority").and_then(|v| v.as_str()).unwrap_or(""),
            "group_id": t.get("groupId").and_then(|v| v.as_str()).unwrap_or("0"),
            "url": format!("{}/company/personal/user/{}/tasks/task/view/{}/", portal, cfg.user_id, id),
        }));
    }

    // комментарии = чат задачи (im.chat.get по ENTITY_TYPE=TASKS_TASK)
    let mut comments: Vec<Value> = Vec::new();
    let mut authors: Vec<i64> = Vec::new();

    for id in ids.iter().take(10) {
        let cached = {
            let g = st.chats.lock().unwrap();
            g.get(id).copied()
        };
        let chat = match cached {
            Some(c) => Some(c),
            None => {
                let r = client
                    .post(format!("{}/im.chat.get.json", base))
                    .json(&json!({"ENTITY_TYPE": "TASKS_TASK", "ENTITY_ID": id}))
                    .send()
                    .await;
                let cid = match r {
                    Ok(resp) => resp
                        .json::<Value>()
                        .await
                        .ok()
                        .and_then(|v| v.pointer("/result/ID").and_then(|x| x.as_i64())),
                    Err(_) => None,
                };
                if let Some(c) = cid {
                    st.chats.lock().unwrap().insert(*id, c);
                }
                cid
            }
        };
        let chat = match chat {
            Some(c) => c,
            None => continue,
        };
        let mres = client
            .post(format!("{}/im.dialog.messages.get.json", base))
            .json(&json!({"DIALOG_ID": format!("chat{}", chat), "LIMIT": 15}))
            .send()
            .await;
        let msgs = match mres {
            Ok(r) => r
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v.pointer("/result/messages").and_then(|x| x.as_array()).cloned())
                .unwrap_or_default(),
            Err(_) => continue,
        };
        let mut per_task = 0;
        for m in msgs.iter() {
            if per_task >= cfg.comments_per_task {
                break;
            }
            let author = m.get("author_id").and_then(|x| x.as_i64()).unwrap_or(0);
            if author == 0 || author == cfg.user_id {
                continue;
            }
            let text = clean_bb(m.get("text").and_then(|x| x.as_str()).unwrap_or(""));
            if text.is_empty() {
                continue;
            }
            if !authors.contains(&author) {
                authors.push(author);
            }
            per_task += 1;
            comments.push(json!({
                "task_id": id,
                "author_id": author,
                "date": m.get("date").and_then(|x| x.as_str()).unwrap_or(""),
                "text": text,
                "task_url": format!("{}/company/personal/user/{}/tasks/task/view/{}/", portal, cfg.user_id, id),
            }));
        }
    }

    // имена авторов (кэш + один user.get)
    let mut names: std::collections::HashMap<i64, String> = {
        let g = st.users.lock().unwrap();
        g.iter().map(|(k, v)| (*k, v.clone())).collect()
    };
    let unknown: Vec<i64> = authors.iter().copied().filter(|a| !names.contains_key(a)).collect();
    if !unknown.is_empty() {
        let r = client
            .post(format!("{}/user.get.json", base))
            .json(&json!({"filter": {"ID": unknown}}))
            .send()
            .await;
        if let Ok(resp) = r {
            if let Ok(v) = resp.json::<Value>().await {
                if let Some(arr) = v.get("result").and_then(|x| x.as_array()) {
                    for u in arr.iter() {
                        let uid = u
                            .get("ID")
                            .and_then(|x| x.as_str())
                            .and_then(|s| s.parse::<i64>().ok())
                            .or_else(|| u.get("ID").and_then(|x| x.as_i64()))
                            .unwrap_or(0);
                        if uid == 0 {
                            continue;
                        }
                        let name = format!(
                            "{} {}",
                            u.get("NAME").and_then(|x| x.as_str()).unwrap_or(""),
                            u.get("LAST_NAME").and_then(|x| x.as_str()).unwrap_or("")
                        )
                        .trim()
                        .to_string();
                        if !name.is_empty() {
                            names.insert(uid, name.clone());
                            st.users.lock().unwrap().insert(uid, name);
                        }
                    }
                }
            }
        }
    }
    for c in comments.iter_mut() {
        let aid = c.get("author_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let name = names.get(&aid).cloned().unwrap_or_else(|| format!("#{}", aid));
        c["author"] = json!(name);
    }

    comments.sort_by(|a, b| {
        let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
        db.cmp(da)
    });
    comments.truncate(30);

    Ok(json!({"ok": true, "tasks": tasks, "comments": comments, "error": Value::Null}))
}

pub(crate) fn clean_bb(s: &str) -> String {
    let mut out = String::new();
    let mut depth_user = 0;
    let mut chars = s.chars().peekable();
    let mut buf = String::new();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut tag = String::new();
            for n in chars.by_ref() {
                if n == ']' {
                    break;
                }
                tag.push(n);
            }
            let low = tag.to_lowercase();
            if low.starts_with("user=") {
                depth_user += 1;
                continue;
            }
            if low == "/user" {
                depth_user = (depth_user - 1).max(0);
                continue;
            }
            if low.starts_with("url") || low.starts_with("/url") {
                continue;
            }
            if low == "br" {
                buf.push(' ');
                continue;
            }
            if ["p", "/p", "b", "/b", "i", "/i", "u", "/u", "*", "list", "/list"].contains(&low.as_str()) {
                buf.push(' ');
                continue;
            }
            buf.push(' ');
            continue;
        }
        if c == '\n' || c == '\r' || c == '\t' {
            buf.push(' ');
            continue;
        }
        buf.push(c);
    }
    out.push_str(&buf);
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let _ = depth_user;
    collapsed
}

#[tauri::command]
pub(crate) async fn cal(
    st: tauri::State<'_, AppState>,
    src: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    ics_one: Option<String>,
    tz: Option<i64>,
) -> Result<Value, String> {
    let cfg = st.cfg.lock().unwrap().clone();
    let src = src.unwrap_or_else(|| "both".to_string());
    let now = chrono_now();
    let from = from.unwrap_or(now - 3600);
    let to = to.unwrap_or(now + 7 * 86400).max(from + 3600);
    let tz = tz.unwrap_or(7 * 3600);
    let base = cfg.webhook.trim().trim_end_matches('/').to_string();
    let portal = portal_base(&base).unwrap_or_default();
    let mut ics: Vec<String> = Vec::new();
    if src != "b24" {
        if let Some(one) = ics_one {
            if !one.trim().is_empty() {
                ics.push(one.trim().to_string());
            }
        }
        for s in cfg.ics_urls.lines().map(|s| s.trim()) {
            if !s.is_empty() && !ics.iter().any(|x| x == s) {
                ics.push(s.to_string());
            }
        }
    }
    let b24_hook = if src == "ics" { String::new() } else { base };
    Ok(connectors::cal_data(&portal, &b24_hook, cfg.user_id, from, to, tz, &ics).await)
}

#[tauri::command]
pub(crate) async fn chats(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let hook = st.cfg.lock().unwrap().webhook.clone();
    Ok(connectors::b24_unread(&hook).await)
}

#[tauri::command]
pub(crate) async fn add_task(
    st: tauri::State<'_, AppState>,
    title: String,
    description: String,
    deadline: i64,
) -> Result<Value, String> {
    let (hook, uid) = {
        let c = st.cfg.lock().unwrap();
        (c.webhook.clone(), c.user_id)
    };
    Ok(connectors::task_add(&hook, &title, &description, uid, deadline).await)
}

pub(crate) fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub(crate) async fn task_done(st: tauri::State<'_, AppState>, id: String) -> Result<Value, String> {
    let hook = st.cfg.lock().unwrap().webhook.clone();
    Ok(connectors::task_done(&hook, &id).await)
}

#[tauri::command]
pub(crate) async fn task_snooze(
    st: tauri::State<'_, AppState>,
    id: String,
    minutes: Option<i64>,
) -> Result<Value, String> {
    let hook = st.cfg.lock().unwrap().webhook.clone();
    Ok(connectors::task_snooze(&hook, &id, minutes.unwrap_or(60)).await)
}
