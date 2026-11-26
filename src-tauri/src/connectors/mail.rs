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
    let mut sess = match client.login(user, pass) {
        Ok(s) => s,
        Err((e, _)) => return json!({"ok": false, "error": format!("вход: {e}"), "messages": [], "unread": 0}),
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
            if let Ok(fetch) = sess.fetch(format!("{}", id), "(ENVELOPE INTERNALDATE FLAGS)") {
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
                    out.push(json!({
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
