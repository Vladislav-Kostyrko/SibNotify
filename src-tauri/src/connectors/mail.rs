#[allow(unused_imports)]
#[allow(unused_imports)]
use super::common::*;
use serde_json::{json, Value};

pub async fn mail_data(cfg: &Value) -> Value {
    let host = cfg.get("host").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let port = cfg.get("port").and_then(|x| x.as_u64()).unwrap_or(993);
    let user = cfg.get("user").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let pass = cfg.get("pass").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let limit = cfg.get("limit").and_then(|x| x.as_u64()).unwrap_or(6) as usize;
    let tls = cfg.get("tls").and_then(|x| x.as_bool()).unwrap_or(true);
    if host.is_empty() || user.is_empty() {
        return json!({"ok": false, "error": "почта не настроена", "messages": [], "unread": 0});
    }
    tokio::task::spawn_blocking(move || mail_blocking(&host, port, &user, &pass, tls, limit))
        .await
        .unwrap_or_else(|e| json!({"ok": false, "error": format!("{e}"), "messages": [], "unread": 0}))
}

fn mail_blocking(host: &str, port: u64, user: &str, pass: &str, tls: bool, limit: usize) -> Value {
    use native_tls::TlsConnector;
    let tlsb = match TlsConnector::builder().build() {
        Ok(t) => t,
        Err(e) => return json!({"ok": false, "error": format!("TLS: {e}"), "messages": [], "unread": 0}),
    };
    let client = if tls {
        match imap::connect((host, port as u16), host, &tlsb) {
            Ok(c) => c,
            Err(e) => return json!({"ok": false, "error": format!("соединение: {e}"), "messages": [], "unread": 0}),
        }
    } else {
        match imap::connect_starttls((host, port as u16), host, &tlsb) {
            Ok(c) => c,
            Err(e) => return json!({"ok": false, "error": format!("соединение: {e}"), "messages": [], "unread": 0}),
        }
    };
    // Яндекс рвёт соединение при частых переподключениях — пробуем дважды
    let mut sess = match client.login(user, pass) {
        Ok(s) => s,
        Err((e, _)) => {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let tlsb2 = match TlsConnector::builder().build() {
                Ok(t) => t,
                Err(e2) => {
                    return json!({"ok": false, "error": format!("TLS: {e2}"), "messages": [], "unread": 0})
                }
            };
            let client2 = if tls {
                imap::connect((host, port as u16), host, &tlsb2)
            } else {
                imap::connect_starttls((host, port as u16), host, &tlsb2)
            };
            let c2 = match client2 {
                Ok(c) => c,
                Err(e2) => {
                    return json!({"ok": false, "error": format!("вход: {e}; повтор: {e2}"), "messages": [], "unread": 0})
                }
            };
            match c2.login(user, pass) {
                Ok(s2) => s2,
                Err((e2, _)) => {
                    return json!({"ok": false, "error": format!("вход: {e} (повтор: {e2})"), "messages": [], "unread": 0})
                }
            }
        }
    };
    let _ = sess.select("INBOX");
    let mut out: Vec<Value> = Vec::new();
    let mut unread: i64 = 0;
    if let Ok(seqs) = sess.search("UNSEEN") {
        unread = seqs.len() as i64;
    }
    if let Ok(seqs) = sess.search("ALL") {
        let mut all: Vec<u32> = seqs.into_iter().collect();
        all.sort_unstable();
        let ids: Vec<u32> = all.into_iter().rev().take(limit).collect();
        for id in ids {
            if let Ok(fetch) = sess.fetch(format!("{}", id), "(ENVELOPE INTERNALDATE FLAGS BODY.PEEK[TEXT]<0.900>)") {
                for m in fetch.iter() {
                    let env = match m.envelope() {
                        Some(e) => e,
                        None => continue,
                    };
                    let subject = env
                        .subject
                        .as_ref()
                        .map(|s| String::from_utf8_lossy(s).to_string())
                        .unwrap_or_default();
                    let subject = decode_mime(&subject);
                    let from = env
                        .from
                        .as_ref()
                        .and_then(|f| f.first())
                        .map(|a| {
                            let n = a
                                .name
                                .as_ref()
                                .map(|x| decode_mime(&String::from_utf8_lossy(x)))
                                .unwrap_or_default();
                            if n.is_empty() {
                                a.mailbox
                                    .as_ref()
                                    .map(|x| String::from_utf8_lossy(x).to_string())
                                    .unwrap_or_default()
                            } else {
                                n
                            }
                        })
                        .unwrap_or_default();
                    let seen = m
                        .flags()
                        .iter()
                        .any(|f| matches!(f, imap::types::Flag::Seen));
                    let date = m
                        .internal_date()
                        .map(|d| d.format("%d.%m %H:%M").to_string())
                        .unwrap_or_default();
                    let raw_body = m
                        .text()
                        .map(|t| String::from_utf8_lossy(t).to_string())
                        .or_else(|| m.body().map(|b| String::from_utf8_lossy(b).to_string()))
                        .unwrap_or_default();
                    out.push(json!({
                        "preview": mail_preview(&raw_body),
                        "subject": subject.chars().take(120).collect::<String>(),
                        "from": from,
                        "date": date,
                        "seen": seen,
                    }));
                }
            }
        }
    }
    let _ = sess.logout();
    json!({"ok": true, "messages": out, "unread": unread, "error": Value::Null})
}

/// Первые строки письма без html-тегов, цитат и мусора — для предпросмотра в панели.
fn mail_preview(raw: &str) -> String {
    let mut t = String::with_capacity(raw.len().min(400));
    let mut in_tag = false;
    let mut body_part = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => { in_tag = false; t.push(' '); }
            c if !in_tag => t.push(c),
            _ => {}
        }
    }
    let t = t
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    let mut out = String::new();
    for line in t.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('>') || l.starts_with("--") {
            continue;
        }
        out.push_str(l);
        out.push(' ');
        if out.chars().count() > 220 {
            break;
        }
        body_part = true;
    }
    let _ = body_part;
    out.trim().chars().take(220).collect()
}
