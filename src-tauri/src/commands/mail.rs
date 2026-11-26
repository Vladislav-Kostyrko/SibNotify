use crate::connectors;
use crate::state::AppState;
use serde_json::Value;

#[tauri::command]
pub(crate) async fn mail(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let m = st.cfg.lock().unwrap().mail.clone();
    Ok(connectors::mail_data(&m).await)
}
