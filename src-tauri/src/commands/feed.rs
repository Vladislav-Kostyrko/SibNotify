use crate::connectors;
use crate::state::AppState;
use serde_json::Value;

#[tauri::command]
pub(crate) async fn tg(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (token, proxy) = {
        let c = st.cfg.lock().unwrap();
        (c.tg_token.clone(), c.proxy.clone())
    };
    Ok(connectors::tg_data(&token, &proxy).await)
}

#[tauri::command]
pub(crate) async fn weather_now(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (lat, lon, city, proxy, src) = {
        let c = st.cfg.lock().unwrap();
        (
            c.weather_lat,
            c.weather_lon,
            c.weather_city.clone(),
            c.proxy.clone(),
            c.weather_source.clone(),
        )
    };
    Ok(connectors::weather(lat, lon, &city, &proxy, &src).await)
}

#[tauri::command]
pub(crate) async fn ai_usage(st: tauri::State<'_, AppState>) -> Result<Value, String> {
    let (prov, key) = {
        let c = st.cfg.lock().unwrap();
        (c.ai_provider.clone(), c.ai_key.clone())
    };
    if key.trim().is_empty() {
        return Ok(serde_json::json!({"ok": false, "error": "ключ не указан"}));
    }
    Ok(connectors::ai_balance(&prov, &key).await)
}
