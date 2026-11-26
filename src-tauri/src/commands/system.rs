use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Instant;
use sysinfo::System;

pub(crate) fn sys_procs_snapshot() -> Vec<(String, u64)> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut out: Vec<(String, u64)> = Vec::new();
    for (_, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_string();
        if name.is_empty() {
            continue;
        }
        out.push((name, p.memory()));
    }
    out
}

#[cfg(windows)]
pub(crate) fn battery_pct() -> Option<i64> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    unsafe {
        let mut st = SYSTEM_POWER_STATUS::default();
        if GetSystemPowerStatus(&mut st).is_ok() && st.BatteryLifePercent != 255 {
            return Some(st.BatteryLifePercent as i64);
        }
    }
    None
}

#[cfg(not(windows))]
pub(crate) fn battery_pct() -> Option<i64> {
    None
}

#[tauri::command]
pub(crate) fn sys_stats(st: tauri::State<AppState>) -> Value {
    let mut sys = st.sys.lock().unwrap();
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu = sys.global_cpu_usage();
    let total = sys.total_memory();
    let used = sys.used_memory();
    let procs = sys.processes().len();
    drop(sys);

    let mut disks = st.disks.lock().unwrap();
    disks.refresh(true);
    let mut disk = json!(null);
    for d in disks.iter() {
        let mp = d.mount_point().to_string_lossy().to_string();
        if mp.starts_with("C") || mp == "/" {
            disk = json!({
                "mount": mp,
                "free_gb": (d.available_space() as f64) / 1024.0 / 1024.0 / 1024.0,
                "total_gb": (d.total_space() as f64) / 1024.0 / 1024.0 / 1024.0,
            });
            break;
        }
    }
    drop(disks);

    let mut nets = st.nets.lock().unwrap();
    nets.refresh(true);
    let mut rx: u64 = 0;
    let mut tx: u64 = 0;
    for (_, n) in nets.iter() {
        rx += n.total_received();
        tx += n.total_transmitted();
    }
    drop(nets);
    let mut prev = st.net_prev.lock().unwrap();
    let now = Instant::now();
    let dt = now.duration_since(prev.2).as_secs_f64().max(0.001);
    let d_rx = (rx.saturating_sub(prev.0) as f64 / dt) as u64;
    let d_tx = (tx.saturating_sub(prev.1) as f64 / dt) as u64;
    *prev = (rx, tx, now);

    let mut procs_all: Vec<(String, u64)> = sys_procs_snapshot();
    procs_all.sort_by(|a, b| b.1.cmp(&a.1));
    let top: Vec<Value> = procs_all
        .iter()
        .take(3)
        .map(|(n, m)| json!({"name": n, "mb": (*m as f64 / 1048576.0).round() as i64}))
        .collect();

    json!({
        "top": top,
        "battery": battery_pct(),
        "cpu": cpu.round() as i64,
        "ram_used_gb": (used as f64 / 1073741824.0 * 10.0).round() / 10.0,
        "ram_total_gb": (total as f64 / 1073741824.0 * 10.0).round() / 10.0,
        "ram_pct": if total > 0 { (used * 100 / total) as i64 } else { 0 },
        "disk": disk,
        "net_down": d_rx,
        "net_up": d_tx,
        "procs": procs,
        "uptime_h": System::uptime() / 3600,
    })
}
