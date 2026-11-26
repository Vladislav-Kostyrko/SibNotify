#[allow(unused_imports)]
#[allow(unused_imports)]
use super::common::*;
use serde_json::{json, Value};

pub async fn mm_data(url: &str, token: &str, limit: usize) -> Value {
    let base = url.trim().trim_end_matches('/').to_string();
    if base.is_empty() || token.trim().is_empty() {
        return json!({"ok": false, "error": "ММ не настроен", "channels": [], "total": 0});
    }
    let c = match client() {
        Ok(c) => c,
        Err(e) => return json!({"ok": false, "error": e, "channels": [], "total": 0}),
    };
    let auth = format!("Bearer {}", token.trim());

    let get_json = |path: String| {
        let c = c.clone();
        let auth = auth.clone();
        let base = base.clone();
        async move {
            match c
                .get(format!("{}{}", base, path))
                .header("Authorization", &auth)
                .send()
                .await
            {
                Ok(r) => r.json::<Value>().await.ok(),
                Err(_) => None,
            }
        }
    };

    let my_id = match get_json("/api/v4/users/me".to_string()).await {
        Some(v) => v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        None => String::new(),
    };

    let teams: Value = match get_json("/api/v4/users/me/teams".to_string()).await {
        Some(v) => v,
        None => return json!({"ok": false, "error": "ММ не ответил", "channels": [], "total": 0}),
    };
    if teams.get("message").is_some() {
        let msg = teams.get("message").and_then(|v| v.as_str()).unwrap_or("ошибка ММ");
        return json!({"ok": false, "error": msg, "channels": [], "total": 0});
    }
    let teams = teams.as_array().cloned().unwrap_or_default();
    if teams.is_empty() {
        return json!({"ok": false, "error": "нет команд в ММ", "channels": [], "total": 0});
    }

    // кандидаты: канал, непрочитанное, последний просмотр
    struct Cand {
        name: String,
        cname: String,
        cid: String,
        unread: i64,
        mentions: i64,
        last_viewed: i64,
    }
    let mut cands: Vec<Cand> = Vec::new();
    let mut mentions_total: i64 = 0;

    for t in teams.iter().take(2) {
        let tid = t.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let _tname = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if tid.is_empty() {
            continue;
        }
        let chans = match get_json(format!("/api/v4/users/me/teams/{}/channels", tid)).await {
            Some(v) => v,
            None => continue,
        };
        let members = match get_json(format!("/api/v4/users/me/teams/{}/channels/members", tid)).await {
            Some(v) => v,
            None => continue,
        };

        // cid -> (прочитано, упоминания, последний просмотр, замучен)
        let mut seen: std::collections::HashMap<String, (i64, i64, i64, bool)> =
            std::collections::HashMap::new();
        if let Some(arr) = members.as_array() {
            for m in arr {
                let cid = m.get("channel_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if cid.is_empty() {
                    continue;
                }
                let read = m.get("msg_count").and_then(|v| v.as_i64()).unwrap_or(0);
                let mt = m.get("mention_count").and_then(|v| v.as_i64()).unwrap_or(0);
                let lv = m.get("last_viewed_at").and_then(|v| v.as_i64()).unwrap_or(0);
                // MM считает канал замьюченным, если mark_unread == "mention"
                let muted = m
                    .pointer("/notify_props/mark_unread")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "mention")
                    .unwrap_or(false);
                seen.insert(cid, (read, mt, lv, muted));
            }
        }

        if let Some(arr) = chans.as_array() {
            for ch in arr {
                let cid = ch.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if cid.is_empty() {
                    continue;
                }
                let (read, mt, lv, muted) = match seen.get(&cid) {
                    Some(x) => *x,
                    None => continue,
                };
                if muted {
                    continue; // замученный канал — ничего не присылаем
                }
                let total = ch.get("total_msg_count").and_then(|v| v.as_i64()).unwrap_or(0);
                let unread = (total - read).max(0);
                if unread == 0 && mt == 0 {
                    continue;
                }
                let ctype = ch.get("type").and_then(|v| v.as_str()).unwrap_or("O");
                let cname = ch.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let api_name = ch.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let disp = ch.get("display_name").and_then(|v| v.as_str()).unwrap_or("");
                let mut name = if !disp.trim().is_empty() {
                    disp.to_string()
                } else if !api_name.trim().is_empty() {
                    api_name.to_string()
                } else {
                    "канал".to_string()
                };
                if ctype == "D" {
                    // личка: имя второго участника из "uid1__uid2"
                    if let Some(other) = api_name.split("__").find(|x| !x.is_empty() && *x != my_id) {
                        if let Some(u) = get_json(format!("/api/v4/users/{}", other)).await {
                            if let Some(un) = u.get("username").and_then(|x| x.as_str()) {
                                name = un.to_string();
                            }
                        }
                    }
                }
                cands.push(Cand {
                    name,
                    cname,
                    cid,
                    unread,
                    mentions: mt,
                    last_viewed: lv,
                });
            }
        }
    }

    cands.sort_by(|a, b| b.mentions.cmp(&a.mentions).then(b.unread.cmp(&a.unread)));
    cands.truncate(limit.max(1) * 3);

    // только НОВЫЕ сообщения: посты после last_viewed_at
    let mut out: Vec<(i64, Value)> = Vec::new();
    for cand in cands.iter() {
        mentions_total += cand.mentions;
        let path = if cand.last_viewed > 0 {
            format!(
                "/api/v4/channels/{}/posts?since={}&per_page=8",
                cand.cid, cand.last_viewed
            )
        } else {
            format!("/api/v4/channels/{}/posts?per_page=3", cand.cid)
        };
        let v = match get_json(path).await {
            Some(v) => v,
            None => continue,
        };
        let order = v.get("order").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        let posts = v.get("posts").cloned().unwrap_or(json!({}));
        let mut msgs: Vec<Value> = Vec::new();
        let mut newest: i64 = 0;
        for pid in order.iter().take(8) {
            let pid = match pid.as_str() {
                Some(s) => s,
                None => continue,
            };
            let p = match posts.get(pid) {
                Some(p) => p,
                None => continue,
            };
            let text = clean_tags(p.get("message").and_then(|x| x.as_str()).unwrap_or(""));
            if text.is_empty() {
                continue;
            }
            let ts = p.get("create_at").and_then(|x| x.as_i64()).unwrap_or(0);
            if ts > newest {
                newest = ts;
            }
            let uid = p.get("user_id").and_then(|x| x.as_str()).unwrap_or("");
            let mut who = String::new();
            if !uid.is_empty() {
                if let Some(u) = get_json(format!("/api/v4/users/{}", uid)).await {
                    who = u
                        .get("username")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
            msgs.push(json!({
                "who": who,
                "text": text.chars().take(180).collect::<String>(),
                "at": ts_hhmm(ts / 1000),
            }));
            if msgs.len() >= 2 {
                break;
            }
        }
        if msgs.is_empty() {
            continue; // в канале новые только системные/пустые — не показываем
        }
        out.push((
            newest,
            json!({
                "name": cand.name,
                "kind": "mm",
                "unread": if cand.unread > 999 { 999 } else { cand.unread },
                "mentions": cand.mentions,
                "msgs": msgs,
                "text": "",
                "url": format!("{}/ptbr/channels/{}", base, cand.cname),
            }),
        ));
    }

    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.truncate(limit.max(1));
    let channels: Vec<Value> = out.into_iter().map(|(_, v)| v).collect();
    let total: i64 = channels
        .iter()
        .map(|v| v.get("unread").and_then(|x| x.as_i64()).unwrap_or(0))
        .sum();
    json!({"ok": true, "channels": channels, "total": total, "mentions": mentions_total, "error": Value::Null})
}
